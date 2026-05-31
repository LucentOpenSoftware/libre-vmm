//! Host shutdown inhibitor — auto-suspend VMs when host shuts down/reboots.
//!
//! Uses systemd-inhibit to delay shutdown, then suspends all running VMs
//! before allowing the shutdown to proceed.

use crate::error::{VmmError, VmmResult};
use std::process::{Child, Command, Stdio};
use tracing::{error, info, warn};

/// Manages a systemd logind shutdown inhibitor lock.
/// While the lock is held, systemd will delay shutdown to allow VM cleanup.
pub struct HostInhibitor {
    /// The inhibitor process (holds the lock via fd inheritance).
    process: Option<Child>,
}

impl HostInhibitor {
    /// Acquire a shutdown inhibitor lock via systemd-inhibit.
    /// The lock is held as long as the child process is alive.
    pub fn acquire() -> VmmResult<Self> {
        // systemd-inhibit --what=shutdown --who="Libre VMM" --why="Suspending VMs"
        //   --mode=delay cat
        // "cat" blocks forever, keeping the inhibitor lock active.
        // We kill it when we want to release.
        let child = Command::new("systemd-inhibit")
            .args([
                "--what=shutdown:sleep",
                "--who=Libre VMM",
                "--why=Suspending running virtual machines",
                "--mode=delay",
                "--",
                "cat",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                VmmError::Other(format!(
                    "Failed to acquire shutdown inhibitor: {}. \
                 systemd-inhibit may not be available.",
                    e
                ))
            })?;

        info!("Acquired systemd shutdown inhibitor (pid={})", child.id());
        Ok(Self {
            process: Some(child),
        })
    }

    /// Release the inhibitor lock (allow shutdown to proceed).
    pub fn release(&mut self) {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
            info!("Released systemd shutdown inhibitor");
        }
    }

    /// Check if the inhibitor is still active.
    pub fn is_active(&mut self) -> bool {
        if let Some(ref mut child) = self.process {
            match child.try_wait() {
                Ok(Some(_)) => {
                    // Process exited — inhibitor is gone
                    self.process = None;
                    false
                },
                Ok(None) => true, // Still running
                Err(_) => false,
            }
        } else {
            false
        }
    }
}

impl Drop for HostInhibitor {
    fn drop(&mut self) {
        self.release();
    }
}

/// Suspend all running VMs to disk for safe host shutdown.
/// Returns the number of VMs suspended.
pub fn suspend_all_running_vms() -> VmmResult<usize> {
    // List running domains
    let output = Command::new("virsh")
        .args(["list", "--name", "--state-running"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| VmmError::Other(format!("Failed to list running VMs: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let vms: Vec<&str> = stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    if vms.is_empty() {
        info!("No running VMs to suspend");
        return Ok(0);
    }

    let mut suspended = 0;
    for vm_name in &vms {
        // SECURITY: CWE-78 — VM names come from virsh list output, which is trusted.
        // Still use -- separator for defense in depth.
        info!("Suspending VM '{}' for host shutdown...", vm_name);
        let result = Command::new("virsh")
            .args(["managedsave", "--", vm_name])
            .stdin(Stdio::null())
            .output();

        match result {
            Ok(out) if out.status.success() => {
                info!("VM '{}' suspended successfully", vm_name);
                suspended += 1;
            },
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                warn!("Failed to suspend VM '{}': {}", vm_name, stderr);
            },
            Err(e) => {
                error!("Failed to run virsh managedsave for '{}': {}", vm_name, e);
            },
        }
    }

    info!("Suspended {}/{} running VMs", suspended, vms.len());
    Ok(suspended)
}
