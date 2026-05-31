//! Scheduled Auto-Snapshots (AutoProtect) — automatic periodic snapshots.
//!
//! VMware-style AutoProtect: automatically create snapshots at configured
//! intervals and prune old ones based on retention policy.
//!
//! ## Wave 16.A1 (Windows port foundation)
//! The pure `AutoSnapshotConfig` data type moved to `vmm-types::auto_snapshot`;
//! it is re-exported here so existing `use vmm_core::auto_snapshot::AutoSnapshotConfig`
//! imports keep working. The scheduler / libvirt code remains in this file.

use crate::config::VmConfigIo;
use crate::error::{VmmError, VmmResult};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tracing::{error, info, warn};

pub use vmm_types::auto_snapshot::AutoSnapshotConfig;

/// Shared state for the auto-snapshot scheduler.
pub struct AutoSnapshotScheduler {
    running: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl AutoSnapshotScheduler {
    /// Start the auto-snapshot scheduler in a background thread.
    /// Checks every 5 minutes whether any VM needs an auto-snapshot.
    pub fn start(uri: String) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let thread = std::thread::Builder::new()
            .name("auto-snapshot".into())
            .spawn(move || {
                scheduler_loop(&uri, &running_clone);
            })
            .ok();

        Self { running, thread }
    }

    /// Stop the scheduler.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(jh) = self.thread.take() {
            let _ = jh.join();
        }
    }
}

impl Drop for AutoSnapshotScheduler {
    fn drop(&mut self) {
        self.stop();
    }
}

fn scheduler_loop(uri: &str, running: &AtomicBool) {
    info!("Auto-snapshot scheduler started");

    while running.load(Ordering::Relaxed) {
        // Sleep in 10-second intervals (fewer wakeups than 1s) while still
        // responding to shutdown within ~10s.
        for _ in 0..30 {
            if !running.load(Ordering::Relaxed) {
                info!("Auto-snapshot scheduler stopped");
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs(10));
        }

        // Load all VM configs and check which need auto-snapshots
        if let Err(e) = check_and_snapshot(uri) {
            error!("Auto-snapshot check failed: {}", e);
        }
    }

    info!("Auto-snapshot scheduler stopped");
}

fn check_and_snapshot(uri: &str) -> VmmResult<()> {
    let config_dir = crate::config::VmConfig::config_dir();
    let entries = std::fs::read_dir(&config_dir)
        .map_err(|e| VmmError::Other(format!("Cannot read config dir: {}", e)))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str::<crate::config::VmConfig>(&data) {
                    if config.auto_snapshot.enabled {
                        check_vm_snapshot(&config, uri);
                    }
                }
            }
        }
    }

    Ok(())
}

fn check_vm_snapshot(config: &crate::config::VmConfig, uri: &str) {
    let vm_name = &config.name;
    let interval =
        std::time::Duration::from_secs(config.auto_snapshot.interval_hours as u64 * 3600);

    // Check last auto-snapshot time by listing snapshots
    let mut conn = match virt::connect::Connect::open(Some(uri)) {
        Ok(c) => c,
        Err(e) => {
            warn!(
                "Auto-snapshot: cannot connect to libvirt for '{}': {}",
                vm_name, e
            );
            return;
        },
    };

    let snapshots = match crate::snapshot::list_snapshots(&conn, vm_name) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Find latest auto-snapshot
    let auto_snaps: Vec<_> = snapshots
        .iter()
        .filter(|s| s.name.starts_with("auto-"))
        .collect();

    let needs_snapshot = if let Some(latest) = auto_snaps.iter().max_by_key(|s| s.creation_time) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        (now - latest.creation_time) as u64 > interval.as_secs()
    } else {
        true // No auto-snapshots yet
    };

    if !needs_snapshot {
        return;
    }

    // Create auto-snapshot
    let now = chrono::Local::now();
    let snap_name = format!("auto-{}", now.format("%Y%m%d-%H%M%S"));
    let description = format!(
        "AutoProtect snapshot (every {}h)",
        config.auto_snapshot.interval_hours
    );

    match crate::snapshot::create_snapshot(&conn, vm_name, &snap_name, &description) {
        Ok(()) => {
            info!("Auto-snapshot '{}' created for VM '{}'", snap_name, vm_name);
            // Prune old auto-snapshots beyond retention
            prune_auto_snapshots(&conn, vm_name, config.auto_snapshot.retention, &auto_snaps);
        },
        Err(e) => {
            warn!("Auto-snapshot failed for '{}': {}", vm_name, e);
        },
    }

    let _ = conn.close();
}

fn prune_auto_snapshots(
    conn: &virt::connect::Connect,
    vm_name: &str,
    retention: u32,
    existing: &[&crate::snapshot::SnapshotInfo],
) {
    // +1 because we just created a new one
    let total = existing.len() + 1;
    if total <= retention as usize {
        return;
    }

    // Sort by creation time, delete oldest
    let mut sorted: Vec<_> = existing.to_vec();
    sorted.sort_by(|a, b| a.creation_time.cmp(&b.creation_time));

    let to_delete = total - retention as usize;
    for snap in sorted.iter().take(to_delete) {
        match crate::snapshot::delete_snapshot(conn, vm_name, &snap.name) {
            Ok(()) => info!("Pruned old auto-snapshot '{}' for '{}'", snap.name, vm_name),
            Err(e) => warn!("Failed to prune snapshot '{}': {}", snap.name, e),
        }
    }
}
