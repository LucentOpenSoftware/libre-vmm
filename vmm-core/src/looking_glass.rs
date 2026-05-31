//! Looking Glass integration for near-zero-latency multi-GPU passthrough display.
//!
//! Looking Glass uses a shared memory (IVSHMEM) device to transfer framebuffer
//! data from a GPU-passthrough VM to the host, bypassing network display protocols.
//! This provides <1ms latency compared to VNC/SPICE's ~16-50ms.
//!
//! ## Wave 16.A1 (Windows port foundation)
//! The pure `LookingGlassConfig` data type moved to `vmm-types::looking_glass`;
//! it is re-exported here so existing `use vmm_core::looking_glass::LookingGlassConfig`
//! imports keep working. The client discovery / shm / launch code remains here.

use std::path::{Path, PathBuf};
use std::process::Command;

pub use vmm_types::looking_glass::LookingGlassConfig;

/// Find the Looking Glass client binary.
pub fn find_client() -> Option<PathBuf> {
    let candidates = [
        "/usr/bin/looking-glass-client",
        "/usr/local/bin/looking-glass-client",
        "/opt/looking-glass/bin/looking-glass-client",
    ];

    for path in candidates {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }

    // Try PATH
    if let Ok(output) = Command::new("which").arg("looking-glass-client").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }

    None
}

/// Check if Looking Glass is available on the system.
pub fn is_available() -> bool {
    find_client().is_some()
}

/// Create the IVSHMEM shared memory file at /dev/shm/looking-glass.
pub fn create_shm_file(size_mib: u32) -> Result<(), String> {
    let shm_path = Path::new("/dev/shm/looking-glass");

    // Calculate size in bytes (must be power of 2)
    let size_bytes = (size_mib as u64) * 1024 * 1024;

    // Create or truncate the file
    let output = Command::new("truncate")
        .args([
            "-s",
            &size_bytes.to_string(),
            &shm_path.display().to_string(),
        ])
        .output()
        .map_err(|e| format!("Failed to create SHM file: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "truncate failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Set permissions (read/write for owner and group)
    let _ = Command::new("chmod")
        .args(["0660", &shm_path.display().to_string()])
        .output();

    Ok(())
}

/// Generate the libvirt XML snippet for IVSHMEM device.
pub fn ivshmem_xml(size_mib: u32) -> String {
    format!(
        r#"    <shmem name='looking-glass'>
      <model type='ivshmem-plain'/>
      <size unit='M'>{}</size>
    </shmem>
"#,
        size_mib
    )
}

/// Launch the Looking Glass client.
pub fn launch_client(config: &LookingGlassConfig) -> Result<std::process::Child, String> {
    let client = config
        .client_path
        .as_ref()
        .map(PathBuf::from)
        .or_else(find_client)
        .ok_or("Looking Glass client not found")?;

    Command::new(client)
        .args([
            "-f",
            "/dev/shm/looking-glass", // SHM file
            "-F",                     // borderless fullscreen
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to launch Looking Glass: {}", e))
}
