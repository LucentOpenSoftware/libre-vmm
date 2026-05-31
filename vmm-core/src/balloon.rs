//! Memory ballooning — dynamic memory adjustment via virtio-balloon.
//!
//! Queries and controls the VM's memory balloon using `virsh dommemstat`
//! and `virsh setmem`, allowing live memory adjustment without reboot.

use crate::error::{VmmError, VmmResult};
use std::process::{Command, Stdio};
use tracing::info;

/// Balloon memory statistics from the guest.
#[derive(Debug, Clone, Default)]
pub struct BalloonStats {
    /// Current memory allocation in MiB.
    pub current_mib: u64,
    /// Configured maximum memory in MiB.
    pub maximum_mib: u64,
    /// Available (unused) memory in the guest, in MiB.
    pub available_mib: u64,
    /// Swap-in bytes.
    pub swap_in_bytes: u64,
    /// Swap-out bytes.
    pub swap_out_bytes: u64,
    /// Whether the balloon driver is responding.
    pub driver_available: bool,
}

/// SECURITY (CWE-20): Validate VM name before passing to virsh commands.
/// Prevents argument injection via crafted domain names.
fn validate_vm_name(name: &str) -> VmmResult<()> {
    if name.is_empty() {
        return Err(VmmError::Other("VM name cannot be empty".to_string()));
    }
    if name.len() > 255 {
        return Err(VmmError::Other(
            "VM name too long (max 255 chars)".to_string(),
        ));
    }
    if name.starts_with('-') {
        return Err(VmmError::Other(
            "VM name must not start with '-' (argument injection risk, CWE-88)".to_string(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ')
    {
        return Err(VmmError::Other(format!(
            "Invalid VM name '{}': only alphanumeric, hyphen, underscore, and period allowed (CWE-20)",
            name
        )));
    }
    Ok(())
}

/// Query balloon statistics for a running VM.
///
/// Wraps `virsh dommemstat <vm_name>` and parses the key-value output.
pub fn query_balloon_stats(vm_name: &str) -> VmmResult<BalloonStats> {
    // SECURITY (CWE-20): Validate VM name before use in virsh commands
    validate_vm_name(vm_name)?;
    // SECURITY: CWE-78 — Use `--` to separate VM name from flags
    let output = Command::new("virsh")
        .args(["dommemstat", "--", vm_name])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("virsh not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::Other(format!("dommemstat failed: {}", stderr)));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut stats = BalloonStats::default();

    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(key), Some(val_str)) = (parts.next(), parts.next()) {
            let value: u64 = val_str.parse().unwrap_or(0);
            match key {
                "actual" => stats.current_mib = value / 1024, // KiB -> MiB
                "balloon" => stats.maximum_mib = value / 1024,
                "available" | "usable" => stats.available_mib = value / 1024,
                "swap_in" => stats.swap_in_bytes = value * 1024,
                "swap_out" => stats.swap_out_bytes = value * 1024,
                _ => {},
            }
        }
    }

    stats.driver_available = stats.current_mib > 0;

    // Also get configured max from virsh dominfo
    if stats.maximum_mib == 0 {
        if let Ok(info_out) = Command::new("virsh")
            .args(["dominfo", "--", vm_name])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
        {
            let info_str = String::from_utf8_lossy(&info_out.stdout);
            for line in info_str.lines() {
                if line.starts_with("Max memory:") {
                    if let Some(val) = line.split_whitespace().nth(2) {
                        stats.maximum_mib = val.parse::<u64>().unwrap_or(0) / 1024;
                    }
                }
            }
        }
    }

    Ok(stats)
}

/// Set the VM's current memory allocation via balloon.
///
/// SECURITY: CWE-20 — Validates target is within safe bounds.
pub fn set_balloon_memory(vm_name: &str, target_mib: u64, max_mib: u64) -> VmmResult<()> {
    // SECURITY (CWE-20): Validate VM name before use in virsh commands
    validate_vm_name(vm_name)?;
    // Clamp to safe range
    let target = target_mib.max(128).min(max_mib);
    let target_kib = target * 1024;

    info!("Setting balloon memory for '{}' to {} MiB", vm_name, target);

    let output = Command::new("virsh")
        .args(["setmem", "--", vm_name, &target_kib.to_string(), "--live"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("virsh not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::Other(format!("setmem failed: {}", stderr)));
    }

    Ok(())
}
