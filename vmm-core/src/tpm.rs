//! TPM 2.0 support via swtpm.
//!
//! Borrowed from Proxmox/libvirt's swtpm integration pattern.
//! swtpm provides software-based TPM emulation for QEMU VMs.
//! Essential for Windows 11 which requires TPM 2.0.
//!
//! Strategy: wire `swtpm` binary now, rewrite in Rust later (security-critical).
//!
//! ## Wave 16.A1 (Windows port foundation)
//! The pure `TpmVersion` enum moved to `vmm-types::tpm`; it is re-exported
//! here so existing `use vmm_core::tpm::TpmVersion` imports keep working.
//! The swtpm process / state-directory code remains in this file.

use crate::error::{VmmError, VmmResult};
use std::path::PathBuf;
use tracing::info;

pub use vmm_types::tpm::TpmVersion;

/// TPM state for a VM. Manages the swtpm state directory and process lifecycle.
pub struct TpmState {
    /// Directory where swtpm stores its state files
    pub state_dir: PathBuf,
    /// TPM version in use
    pub version: TpmVersion,
    /// Socket path for QEMU connection
    pub socket_path: PathBuf,
}

impl TpmState {
    /// Create a new TPM state directory for a VM.
    pub fn new(vm_id: &uuid::Uuid, version: TpmVersion) -> VmmResult<Self> {
        let base = tpm_state_base_dir();
        let state_dir = base.join(vm_id.to_string());
        let socket_path = state_dir.join("swtpm-sock");

        std::fs::create_dir_all(&state_dir)?;
        // SECURITY: Set TPM state directory to owner-only (CWE-276).
        // TPM state contains endorsement keys, platform certificates, and NVRAM data.
        // Default permissions (0o755) would expose these secrets to other local users.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700));
        }

        Ok(Self {
            state_dir,
            version,
            socket_path,
        })
    }

    /// Load an existing TPM state for a VM.
    pub fn load(vm_id: &uuid::Uuid, version: TpmVersion) -> Option<Self> {
        let base = tpm_state_base_dir();
        let state_dir = base.join(vm_id.to_string());

        if state_dir.exists() {
            let socket_path = state_dir.join("swtpm-sock");
            Some(Self {
                state_dir,
                version,
                socket_path,
            })
        } else {
            None
        }
    }

    /// Delete the TPM state directory.
    pub fn delete(&self) -> VmmResult<()> {
        if self.state_dir.exists() {
            std::fs::remove_dir_all(&self.state_dir)?;
            info!("TPM state deleted: {}", self.state_dir.display());
        }
        Ok(())
    }
}

/// Check if swtpm is installed and available on the system.
pub fn swtpm_available() -> bool {
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
    std::process::Command::new("swtpm")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Get the swtpm version string.
pub fn swtpm_version() -> Option<String> {
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
    let output = std::process::Command::new("swtpm")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;

    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout);
        // Parse "TPM emulator version X.Y.Z..." or similar
        Some(version.trim().to_string())
    } else {
        None
    }
}

/// Initialize a new TPM state (creates the EK and platform certs).
/// This is run once when TPM is first enabled for a VM.
pub fn initialize_tpm_state(state: &TpmState) -> VmmResult<()> {
    let tpm2_flag = match state.version {
        TpmVersion::V1_2 => "--tpm-state",
        TpmVersion::V2_0 => "--tpm-state",
    };

    let tpm_version_flag = match state.version {
        TpmVersion::V1_2 => "--tpm",
        TpmVersion::V2_0 => "--tpm2",
    };

    // swtpm_setup: initialize the TPM state directory with certificates
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to swtpm_setup.
    // swtpm handles TPM endorsement keys and platform certificates; it must not
    // inherit file descriptors pointing to other sensitive resources.
    let status = std::process::Command::new("swtpm_setup")
        .arg(tpm_version_flag)
        .arg(tpm2_flag)
        .arg(state.state_dir.to_str().ok_or_else(|| {
            VmmError::Other(format!(
                "TPM state directory path contains invalid UTF-8: {:?}",
                state.state_dir
            ))
        })?)
        .arg("--createek")
        .arg("--create-ek-cert")
        .arg("--create-platform-cert")
        .arg("--lock-nvram")
        .arg("--not-overwrite")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        // SECURITY: Use .output() instead of .status() to capture stderr for error reporting (CWE-252)
        .output()
        .map_err(|e| VmmError::Other(format!("Failed to run swtpm_setup: {}", e)))?;

    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        // Clean up partial state on failure to prevent broken TPM state
        let _ = std::fs::remove_dir_all(&state.state_dir);
        return Err(VmmError::Other(format!(
            "swtpm_setup failed (exit {:?}): {}",
            status.status.code(),
            stderr.trim()
        )));
    }

    info!(
        "TPM {} state initialized at {}",
        state.version,
        state.state_dir.display()
    );
    Ok(())
}

/// Generate the libvirt XML for the TPM device.
/// This creates a `<tpm>` element that tells QEMU to use swtpm via a socket.
pub fn tpm_device_xml(state: &TpmState) -> String {
    let version = match state.version {
        TpmVersion::V1_2 => "1.2",
        TpmVersion::V2_0 => "2.0",
    };

    format!(
        r#"    <tpm model='tpm-crb'>
      <backend type='emulator' version='{version}'>
        <active_pcr_banks>
          <sha256/>
        </active_pcr_banks>
      </backend>
    </tpm>
"#,
        version = version,
    )
}

/// Base directory for all TPM state.
fn tpm_state_base_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".local/share/libre-vmm/tpm")
}

/// Check whether a VM has existing TPM state on disk.
pub fn has_tpm_state(vm_id: &uuid::Uuid) -> bool {
    let base = tpm_state_base_dir();
    base.join(vm_id.to_string()).exists()
}

/// Get a summary of TPM status for display.
pub fn tpm_status_summary() -> String {
    if swtpm_available() {
        let ver = swtpm_version().unwrap_or_else(|| "unknown".to_string());
        format!("swtpm available ({})", ver)
    } else {
        "swtpm not installed — TPM emulation unavailable".to_string()
    }
}
