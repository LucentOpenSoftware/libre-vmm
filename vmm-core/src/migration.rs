//! Live Migration — migrate running VMs between hypervisor hosts.
//!
//! Uses `virsh migrate` for reliable cross-host migration with progress tracking.
//! Supports live (online), offline, and peer-to-peer migration modes.

use crate::error::{VmmError, VmmResult};
use crate::remote::RemoteHost;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

/// Migration type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MigrationType {
    /// Live migration — VM stays running during transfer (minimal downtime).
    Live,
    /// Offline migration — VM is paused, transferred, then resumed on target.
    Offline,
    /// Peer-to-peer — source hypervisor manages the migration directly
    /// (the management client doesn't need to stay connected).
    PeerToPeer,
}

impl std::fmt::Display for MigrationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationType::Live => write!(f, "Live"),
            MigrationType::Offline => write!(f, "Offline"),
            MigrationType::PeerToPeer => write!(f, "Peer-to-peer"),
        }
    }
}

/// Migration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationOptions {
    /// Type of migration to perform.
    pub migration_type: MigrationType,
    /// Maximum bandwidth in MiB/s (0 = unlimited).
    pub bandwidth_mib: u64,
    /// Whether to use compressed migration (reduces bandwidth at CPU cost).
    pub compressed: bool,
    /// Whether to copy storage (non-shared storage migration).
    /// If false, storage must be shared between source and destination.
    pub copy_storage: bool,
    /// Whether to make the migration persistent on the destination
    /// (i.e., define the domain permanently, not just transient).
    pub persistent: bool,
    /// Whether to undefine the domain on the source after migration.
    pub undefine_source: bool,
    /// Auto-converge: slow down guest vCPUs to help migration converge.
    pub auto_converge: bool,
    /// Post-copy migration: switch to destination before memory transfer completes.
    /// Riskier but faster for memory-heavy VMs.
    pub postcopy: bool,
}

impl Default for MigrationOptions {
    fn default() -> Self {
        Self {
            migration_type: MigrationType::Live,
            bandwidth_mib: 0,
            compressed: false,
            copy_storage: false,
            persistent: true,
            undefine_source: true,
            auto_converge: false,
            postcopy: false,
        }
    }
}

/// Migration progress info.
#[derive(Debug, Clone, Default)]
pub struct MigrationProgress {
    /// Current phase description.
    pub phase: String,
    /// Percentage complete (0-100), -1 if unknown.
    pub percent: i32,
    /// Data transferred so far (bytes).
    pub data_transferred: u64,
    /// Data remaining (bytes), 0 if unknown.
    pub data_remaining: u64,
    /// Memory transfer rate (bytes/sec), 0 if unknown.
    pub memory_bps: u64,
    /// Elapsed time in seconds.
    pub elapsed_secs: u64,
    /// Whether migration is complete.
    pub completed: bool,
    /// Error message if migration failed.
    pub error: Option<String>,
    /// Whether migration was cancelled.
    pub cancelled: bool,
}

/// Shared migration state for cross-thread progress tracking.
pub type SharedMigrationProgress = Arc<Mutex<MigrationProgress>>;

/// Result of starting a migration: progress handle + thread join handle.
/// SECURITY: CWE-404 (Improper Resource Shutdown) — The JoinHandle must be
/// stored by the caller to ensure the migration thread is joined on shutdown.
/// Dropping the JoinHandle detaches the thread, preventing orderly cleanup.
pub struct MigrationHandle {
    /// Shared progress for tracking migration state from the GUI.
    pub progress: SharedMigrationProgress,
    /// Join handle for the migration thread. Must be stored for cleanup (CWE-404).
    pub join_handle: Option<std::thread::JoinHandle<()>>,
}

/// Maximum migration timeout in seconds (4 hours).
/// Prevents unbounded hangs if virsh migrate never returns (CWE-400).
const MIGRATION_TIMEOUT_SECS: u64 = 4 * 60 * 60;

/// Validate a VM name for safe use in virsh commands.
/// Delegates to `config::validate_vm_name` for full validation, but also
/// rejects names that could cause argument injection (CWE-88, CWE-78).
fn validate_migration_vm_name(vm_name: &str) -> Result<(), String> {
    if let Some(err) = crate::config::validate_vm_name(vm_name) {
        return Err(format!("Invalid VM name: {}", err));
    }
    Ok(())
}

/// Validate a libvirt connection URI for safe use in virsh -c / migrate commands.
/// Rejects URIs that could inject shell metacharacters or virsh flags (CWE-88, CWE-78).
fn validate_dest_uri(uri: &str) -> Result<(), String> {
    if uri.is_empty() {
        return Err("Destination URI cannot be empty".to_string());
    }
    if uri.len() > 1024 {
        return Err("Destination URI too long (max 1024)".to_string());
    }
    // Must not start with a hyphen (virsh would treat as a flag)
    if uri.starts_with('-') {
        return Err(
            "Destination URI must not start with '-' (argument injection risk)".to_string(),
        );
    }
    // Only allow characters valid in libvirt URIs: scheme, host, path components.
    // Valid libvirt URIs: qemu+ssh://user@host/system, qemu:///system, etc.
    // Block shell metacharacters: ; | & $ ` \ ' " > < ! { } ( ) newlines
    if uri.chars().any(|c| ";|&$`\\'\"<>!{}()\n\r\t".contains(c)) {
        return Err(format!(
            "Destination URI contains disallowed characters: {}",
            uri
        ));
    }
    // Must look like a URI (contain ://)
    if !uri.contains("://") {
        return Err("Destination URI must contain '://' (expected libvirt URI format)".to_string());
    }
    Ok(())
}

/// Check if a destination host is reachable and ready for migration.
pub fn preflight_check(host: &RemoteHost) -> VmmResult<MigrationPreflight> {
    let mut result = MigrationPreflight::default();

    // Test SSH connectivity
    match host.test_ssh() {
        Ok(hostname) => {
            result.ssh_ok = true;
            result.remote_hostname = hostname;
        },
        Err(e) => {
            result.ssh_error = Some(format!("SSH failed: {}", e));
            return Ok(result);
        },
    }

    // Test libvirt connectivity
    match host.test_libvirt() {
        Ok(_) => {
            result.libvirt_ok = true;
        },
        Err(e) => {
            result.libvirt_error = Some(format!("Libvirt failed: {}", e));
            return Ok(result);
        },
    }

    // Check destination capabilities
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let uri = host.connection_uri();
    let output = std::process::Command::new("virsh")
        .args(["-c", &uri, "capabilities"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::Other(format!("Failed to query capabilities: {}", e)))?;

    if output.status.success() {
        let caps = String::from_utf8_lossy(&output.stdout);
        result.has_kvm = caps.contains("kvm");
        result.capabilities_ok = true;
    }

    // Check free memory on destination
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
    let output = std::process::Command::new("virsh")
        .args(["-c", &uri, "nodememstats"])
        .stdin(std::process::Stdio::null())
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("free") {
                    if let Some(val) = line.split_whitespace().nth(1) {
                        result.free_memory_mib = val.parse().unwrap_or(0) / 1024;
                    }
                }
            }
        }
    }

    Ok(result)
}

/// Preflight check results.
#[derive(Debug, Clone, Default)]
pub struct MigrationPreflight {
    pub ssh_ok: bool,
    pub ssh_error: Option<String>,
    pub libvirt_ok: bool,
    pub libvirt_error: Option<String>,
    pub capabilities_ok: bool,
    pub has_kvm: bool,
    pub remote_hostname: String,
    pub free_memory_mib: u64,
}

impl MigrationPreflight {
    /// Whether all preflight checks passed.
    pub fn all_ok(&self) -> bool {
        self.ssh_ok && self.libvirt_ok && self.capabilities_ok && self.has_kvm
    }

    /// Get a summary of the preflight status.
    pub fn summary(&self) -> Vec<(String, bool, String)> {
        let mut items = Vec::new();
        items.push((
            "SSH Connectivity".to_string(),
            self.ssh_ok,
            if self.ssh_ok {
                format!("Connected to {}", self.remote_hostname)
            } else {
                self.ssh_error
                    .clone()
                    .unwrap_or_else(|| "Not tested".to_string())
            },
        ));
        items.push((
            "Libvirt Connection".to_string(),
            self.libvirt_ok,
            if self.libvirt_ok {
                "Connected".to_string()
            } else {
                self.libvirt_error
                    .clone()
                    .unwrap_or_else(|| "Not tested".to_string())
            },
        ));
        items.push((
            "KVM Support".to_string(),
            self.has_kvm,
            if self.has_kvm {
                "Available"
            } else {
                "Not detected"
            }
            .to_string(),
        ));
        items.push((
            "Free Memory".to_string(),
            self.free_memory_mib > 0,
            if self.free_memory_mib > 0 {
                format!("{} MiB available", self.free_memory_mib)
            } else {
                "Unknown".to_string()
            },
        ));
        items
    }
}

/// Start a migration in a background thread.
/// Returns a `MigrationHandle` with both the progress tracker and the thread's JoinHandle.
///
/// SECURITY: CWE-404 (Improper Resource Shutdown) — Previously returned only
/// `SharedMigrationProgress` and dropped the JoinHandle, creating a detached thread
/// with no cleanup on shutdown. The caller must now store the returned
/// `MigrationHandle` and join the thread during shutdown.
///
/// CWE-362 (Race Condition) — The migration thread and poll thread both access
/// the shared progress mutex; the mutex serializes access correctly.
pub fn migrate_vm(
    vm_name: &str,
    dest_host: &RemoteHost,
    options: &MigrationOptions,
) -> MigrationHandle {
    // SECURITY: Full VM name validation (CWE-88, CWE-78).
    // Delegates to config::validate_vm_name to reject names with shell metacharacters,
    // leading hyphens (flag injection), or other unsafe patterns.
    if let Err(err) = validate_migration_vm_name(vm_name) {
        let progress = Arc::new(Mutex::new(MigrationProgress {
            phase: "Migration failed".to_string(),
            error: Some(err),
            completed: true,
            ..Default::default()
        }));
        return MigrationHandle {
            progress,
            join_handle: None,
        };
    }

    // SECURITY: Validate destination URI before passing to virsh (CWE-88, CWE-78).
    let dest_uri = dest_host.connection_uri();
    if let Err(err) = validate_dest_uri(&dest_uri) {
        let progress = Arc::new(Mutex::new(MigrationProgress {
            phase: "Migration failed".to_string(),
            error: Some(err),
            completed: true,
            ..Default::default()
        }));
        return MigrationHandle {
            progress,
            join_handle: None,
        };
    }

    let progress = Arc::new(Mutex::new(MigrationProgress {
        phase: "Preparing migration...".to_string(),
        percent: 0,
        ..Default::default()
    }));

    let progress_clone = Arc::clone(&progress);
    let vm_name = vm_name.to_string();
    let options = options.clone();

    // SECURITY: CWE-404 — Store JoinHandle instead of dropping it.
    // Named thread for debuggability.
    let join_handle = std::thread::Builder::new()
        .name(format!("migration-{}", vm_name))
        .spawn(move || {
            run_migration(&vm_name, &dest_uri, &options, &progress_clone);
        });

    match join_handle {
        Ok(jh) => MigrationHandle {
            progress,
            join_handle: Some(jh),
        },
        Err(e) => {
            tracing::error!("Failed to spawn migration thread: {}", e);
            let mut p = progress.lock().unwrap_or_else(|e| {
                tracing::error!("Migration progress mutex poisoned (CWE-662)");
                e.into_inner()
            });
            p.error = Some(format!("Thread spawn failed: {}", e));
            p.completed = true;
            drop(p);
            MigrationHandle {
                progress,
                join_handle: None,
            }
        },
    }
}

/// Execute the migration (runs in background thread).
fn run_migration(
    vm_name: &str,
    dest_uri: &str,
    options: &MigrationOptions,
    progress: &SharedMigrationProgress,
) {
    let start_time = std::time::Instant::now();

    // Update phase
    {
        let mut p = progress.lock().unwrap_or_else(|e| {
            tracing::error!("Migration progress mutex poisoned (CWE-662)");
            e.into_inner()
        });
        p.phase = "Building migration command...".to_string();
        p.percent = 5;
    }

    // Build virsh migrate command
    // Use &str for static flag literals; only dynamic values need String conversion.
    let mut args: Vec<String> = Vec::new();
    args.push("migrate".into());

    // Migration flags
    match options.migration_type {
        MigrationType::Live => args.push("--live".into()),
        MigrationType::Offline => args.push("--offline".into()),
        MigrationType::PeerToPeer => {
            args.push("--live".into());
            args.push("--p2p".into());
        },
    }

    if options.persistent {
        args.push("--persistent".into());
    }
    if options.undefine_source {
        args.push("--undefinesource".into());
    }
    if options.compressed {
        args.push("--compressed".into());
    }
    if options.copy_storage {
        args.push("--copy-storage-all".into());
    }
    if options.auto_converge {
        args.push("--auto-converge".into());
    }
    if options.postcopy {
        args.push("--postcopy".into());
        args.push("--postcopy-after-precopy".into());
    }

    // Verbose for progress output
    args.push("--verbose".into());

    // VM name and destination (dynamic values)
    args.push(vm_name.to_string());
    args.push(dest_uri.to_string());

    // Bandwidth limit
    if options.bandwidth_mib > 0 {
        args.push("--bandwidth".into());
        args.push(options.bandwidth_mib.to_string());
    }

    // SECURITY: Set virsh-level timeout to bound migration duration (CWE-400).
    // This tells libvirt to abort the migration if it hasn't converged within the limit.
    args.push("--timeout".into());
    args.push(MIGRATION_TIMEOUT_SECS.to_string());

    info!("Starting migration: virsh {}", args.join(" "));

    {
        let mut p = progress.lock().unwrap_or_else(|e| {
            tracing::error!("Migration progress mutex poisoned (CWE-662)");
            e.into_inner()
        });
        p.phase = "Initiating migration...".to_string();
        p.percent = 10;
    }

    // Start virsh migrate with stderr piped for progress output
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to long-lived migration process.
    let child = std::process::Command::new("virsh")
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let child = match child {
        Ok(c) => c,
        Err(e) => {
            let mut p = progress.lock().unwrap_or_else(|e| {
                tracing::error!("Migration progress mutex poisoned (CWE-662)");
                e.into_inner()
            });
            p.error = Some(format!("Failed to start virsh: {}", e));
            p.completed = true;
            return;
        },
    };

    // Monitor the child process
    // virsh migrate --verbose prints progress on stderr
    {
        let mut p = progress.lock().unwrap_or_else(|e| {
            tracing::error!("Migration progress mutex poisoned (CWE-662)");
            e.into_inner()
        });
        p.phase = "Migration in progress...".to_string();
        p.percent = 20;
    }

    // Poll migration job stats while process is running
    let vm = vm_name.to_string();
    let progress_poller = Arc::clone(progress);
    let poll_thread = std::thread::spawn(move || {
        poll_migration_stats(&vm, &progress_poller);
    });

    // Wait for the migration to complete
    let output = child.wait_with_output();

    // Stop the poller
    {
        let mut p = progress.lock().unwrap_or_else(|e| {
            tracing::error!("Migration progress mutex poisoned (CWE-662)");
            e.into_inner()
        });
        p.completed = true;
    }
    let _ = poll_thread.join();

    match output {
        Ok(output) => {
            let elapsed = start_time.elapsed().as_secs();
            let mut p = progress.lock().unwrap_or_else(|e| {
                tracing::error!("Migration progress mutex poisoned (CWE-662)");
                e.into_inner()
            });
            p.elapsed_secs = elapsed;

            if output.status.success() {
                p.phase = "Migration completed successfully!".to_string();
                p.percent = 100;
                info!("VM '{}' migrated to {} in {}s", vm_name, dest_uri, elapsed);
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let err_msg = if !stderr.is_empty() {
                    stderr.to_string()
                } else {
                    stdout.to_string()
                };
                p.error = Some(format!("Migration failed: {}", err_msg.trim()));
                p.phase = "Migration failed".to_string();
                warn!("Migration of '{}' failed: {}", vm_name, err_msg.trim());
            }
        },
        Err(e) => {
            let mut p = progress.lock().unwrap_or_else(|e| {
                tracing::error!("Migration progress mutex poisoned (CWE-662)");
                e.into_inner()
            });
            p.error = Some(format!("Failed to wait for migration: {}", e));
            p.completed = true;
        },
    }
}

/// Poll migration job statistics via `virsh domjobinfo`.
/// Also enforces a process-level timeout as a safety net (CWE-400).
fn poll_migration_stats(vm_name: &str, progress: &SharedMigrationProgress) {
    let poll_start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(MIGRATION_TIMEOUT_SECS + 300); // 5 min grace

    loop {
        // Check if migration is done
        {
            let p = progress.lock().unwrap_or_else(|e| {
                tracing::error!("Migration progress mutex poisoned (CWE-662)");
                e.into_inner()
            });
            if p.completed {
                break;
            }
        }

        // SECURITY: Process-level timeout as safety net in case virsh --timeout
        // doesn't fire (e.g., virsh bug or hung process) (CWE-400).
        if poll_start.elapsed() > timeout {
            warn!(
                "Migration of '{}' exceeded process timeout ({}s + 300s grace), aborting",
                vm_name, MIGRATION_TIMEOUT_SECS
            );
            // Attempt to abort via domjobabort
            // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
            let _ = std::process::Command::new("virsh")
                .args(["domjobabort", vm_name])
                .stdin(std::process::Stdio::null())
                .output();
            let mut p = progress.lock().unwrap_or_else(|e| {
                tracing::error!("Migration progress mutex poisoned (CWE-662)");
                e.into_inner()
            });
            p.error = Some(format!(
                "Migration timed out after {} seconds",
                poll_start.elapsed().as_secs()
            ));
            p.completed = true;
            break;
        }

        std::thread::sleep(std::time::Duration::from_secs(2));

        // Query job info
        // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
        let output = std::process::Command::new("virsh")
            .args(["domjobinfo", vm_name])
            .stdin(std::process::Stdio::null())
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                parse_domjobinfo(&stdout, progress);
            }
        }
    }
}

/// Parse virsh domjobinfo output into progress.
fn parse_domjobinfo(output: &str, progress: &SharedMigrationProgress) {
    let mut p = progress.lock().unwrap_or_else(|e| {
        tracing::error!("Migration progress mutex poisoned (CWE-662)");
        e.into_inner()
    });

    for line in output.lines() {
        let line = line.trim();

        if line.starts_with("Job type:") {
            let job_type = line.split(':').nth(1).unwrap_or("").trim();
            if job_type == "None" || job_type == "Completed" {
                return;
            }
        }

        if line.starts_with("Data processed:") {
            if let Some(val) = extract_bytes_value(line) {
                p.data_transferred = val;
            }
        }

        if line.starts_with("Data remaining:") {
            if let Some(val) = extract_bytes_value(line) {
                p.data_remaining = val;
            }
        }

        if line.starts_with("Memory bandwidth:") {
            if let Some(val) = extract_bytes_value(line) {
                p.memory_bps = val;
            }
        }
    }

    // Calculate percentage from data transferred/remaining
    let total = p.data_transferred + p.data_remaining;
    if total > 0 {
        // SECURITY: CWE-190 — Clamp ratio to 0.0..1.0 before casting to i32
        // to prevent overflow if data_transferred exceeds total (corrupted stats).
        let ratio = (p.data_transferred as f64 / total as f64).clamp(0.0, 1.0);
        let pct = (ratio * 80.0) as i32 + 20;
        p.percent = pct.min(99); // Cap at 99 until actually done
        p.phase = format!(
            "Migrating... ({} / {} transferred)",
            format_bytes(p.data_transferred),
            format_bytes(total),
        );
    }
}

/// Extract a byte value from a domjobinfo line like "Data processed:     123.456 MiB"
fn extract_bytes_value(line: &str) -> Option<u64> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() >= 3 {
        let val: f64 = parts[parts.len() - 2].parse().ok()?;
        let unit = parts[parts.len() - 1];
        // SECURITY: Validate float value is positive and finite before conversion (CWE-190).
        // Convert the float to an integer part first, then use checked_mul to detect
        // overflow instead of relying on loose float caps.
        //
        // Max safe values per unit (u64::MAX = 18_446_744_073_709_551_615):
        //   - B:   val ≤ u64::MAX                      (~1.8e19)
        //   - KiB: val ≤ u64::MAX / 1024               (~1.8e16)
        //   - MiB: val ≤ u64::MAX / 1_048_576           (~1.76e13)
        //   - GiB: val ≤ u64::MAX / 1_073_741_824       (~1.72e10)
        //
        // Practical cap: 1 PiB (1_048_576 GiB) is already beyond any real migration.
        // We cap all units at 1 PiB equivalent to reject absurd values early.
        const MAX_BYTES: u64 = 1 << 50; // 1 PiB (CWE-190: cap to prevent overflow)

        if !val.is_finite() || val < 0.0 {
            return None;
        }

        let multiplier: u64 = match unit {
            "B" => 1,
            "KiB" => 1024,
            "MiB" => 1_048_576,
            "GiB" => 1_073_741_824,
            _ => return None,
        };

        // Reject values that would overflow u64 when cast from f64.
        // f64 can represent integers exactly up to 2^53; beyond that precision is lost.
        // We cap at MAX_BYTES / multiplier to guarantee the product fits in u64.
        let max_val = (MAX_BYTES / multiplier) as f64;
        if val > max_val {
            return None; // exceeds 1 PiB cap (CWE-190)
        }

        // Safe to truncate: val is positive, finite, and within u64 range.
        let int_val = val as u64;

        // SECURITY: Use checked_mul to catch any remaining overflow (CWE-190).
        int_val.checked_mul(multiplier)
    } else {
        None
    }
}

/// Format bytes into human-readable string.
fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Cancel an in-progress migration.
pub fn cancel_migration(vm_name: &str) -> VmmResult<()> {
    // SECURITY: Full VM name validation (CWE-88, CWE-78).
    if let Err(err) = validate_migration_vm_name(vm_name) {
        return Err(VmmError::Other(err));
    }
    info!("Cancelling migration of VM '{}'", vm_name);

    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
    let output = std::process::Command::new("virsh")
        .args(["domjobabort", vm_name])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::Other(format!("Failed to cancel migration: {}", e)))?;

    if output.status.success() {
        info!("Migration cancelled for '{}'", vm_name);
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(VmmError::Other(format!(
            "Failed to cancel migration: {}",
            stderr.trim()
        )))
    }
}

/// Get a list of remote hosts that support migration for a given VM.
pub fn compatible_hosts(_vm_name: &str, hosts: &[RemoteHost]) -> Vec<(usize, String, bool)> {
    hosts
        .iter()
        .enumerate()
        .map(|(i, host)| {
            let reachable = host.test_ssh().is_ok();
            (i, host.name.clone(), reachable)
        })
        .collect()
}
