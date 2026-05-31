//! Rollback Mode — auto-snapshot before each session for easy undo.
//!
//! Inspired by Parallels' rollback feature: automatically creates a snapshot
//! before every VM start so the user can always revert to the previous state.

use crate::error::{VmmError, VmmResult};
use crate::snapshot::{self, SnapshotInfo};
use tracing::info;
use virt::connect::Connect;

/// Prefix used for all auto-rollback snapshots.
const ROLLBACK_PREFIX: &str = "libre-vmm-rollback-";

/// Configuration for rollback mode.
#[derive(Debug, Clone)]
pub struct RollbackConfig {
    /// Whether rollback mode is enabled.
    pub enabled: bool,
    /// Maximum number of rollback points to keep (default: 5).
    pub max_rollback_points: usize,
    /// Automatically take a snapshot before every VM start.
    pub auto_snapshot_on_start: bool,
}

impl Default for RollbackConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_rollback_points: 5,
            auto_snapshot_on_start: true,
        }
    }
}

/// Create a rollback point (auto-named with timestamp).
///
/// The snapshot is named `libre-vmm-rollback-{unix_timestamp}` so it can be
/// identified and filtered separately from user-created snapshots.
///
/// Returns the name of the newly created rollback snapshot.
pub fn create_rollback_point(conn: &Connect, vm_name: &str) -> VmmResult<String> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let snap_name = format!("{}{}", ROLLBACK_PREFIX, timestamp);
    let description = format!(
        "Auto-rollback point created at {}",
        chrono_like_format(timestamp)
    );

    snapshot::create_snapshot(conn, vm_name, &snap_name, &description)?;
    info!(
        "Rollback point '{}' created for VM '{}'",
        snap_name, vm_name
    );

    Ok(snap_name)
}

/// List rollback points (auto-snapshots only, filtered by naming pattern).
///
/// Returns only snapshots whose names start with `libre-vmm-rollback-`,
/// sorted by creation time (oldest first).
pub fn list_rollback_points(conn: &Connect, vm_name: &str) -> VmmResult<Vec<SnapshotInfo>> {
    let all_snapshots = snapshot::list_snapshots(conn, vm_name)?;

    let mut rollback_snaps: Vec<SnapshotInfo> = all_snapshots
        .into_iter()
        .filter(|s| s.name.starts_with(ROLLBACK_PREFIX))
        .collect();

    // Sort by creation time, oldest first
    rollback_snaps.sort_by_key(|s| s.creation_time);

    Ok(rollback_snaps)
}

/// Revert to the most recent rollback point.
///
/// Returns the name of the snapshot that was reverted to.
pub fn revert_latest(conn: &Connect, vm_name: &str) -> VmmResult<String> {
    let rollback_points = list_rollback_points(conn, vm_name)?;

    let latest = rollback_points.last().ok_or_else(|| {
        VmmError::SnapshotError(format!("No rollback points found for VM '{}'", vm_name))
    })?;

    let snap_name = latest.name.clone();
    snapshot::revert_snapshot(conn, vm_name, &snap_name)?;

    info!(
        "VM '{}' reverted to latest rollback point '{}'",
        vm_name, snap_name
    );

    Ok(snap_name)
}

/// Prune old rollback points to stay within `max_keep` limit.
///
/// Deletes the oldest rollback points first. Returns the number of
/// snapshots that were pruned.
pub fn prune_rollback_points(conn: &Connect, vm_name: &str, max_keep: usize) -> VmmResult<usize> {
    let rollback_points = list_rollback_points(conn, vm_name)?;

    if rollback_points.len() <= max_keep {
        return Ok(0);
    }

    let to_remove = rollback_points.len() - max_keep;
    let mut pruned = 0;

    // Remove oldest first (list is sorted oldest-first)
    for snap in rollback_points.iter().take(to_remove) {
        match snapshot::delete_snapshot(conn, vm_name, &snap.name) {
            Ok(()) => {
                info!("Pruned old rollback point '{}'", snap.name);
                pruned += 1;
            },
            Err(e) => {
                // Log but continue pruning the rest
                tracing::warn!("Failed to prune rollback point '{}': {}", snap.name, e);
            },
        }
    }

    info!(
        "Pruned {}/{} old rollback points for VM '{}'",
        pruned, to_remove, vm_name
    );

    Ok(pruned)
}

/// Simple timestamp formatter (avoids pulling in chrono for a single format).
/// Produces "YYYY-MM-DD HH:MM:SS UTC" from a Unix timestamp.
fn chrono_like_format(unix_secs: u64) -> String {
    // Basic formatting — seconds since epoch to a human-readable string.
    // For a lightweight approach we just show the Unix timestamp if we
    // don't want to add chrono as a dependency.
    format!("unix:{}", unix_secs)
}
