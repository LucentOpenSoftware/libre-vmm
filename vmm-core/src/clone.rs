//! VM cloning — full and linked clone support.
//!
//! Full clone: Copies the entire disk image, creates a new VM with new UUID/MAC.
//! Linked clone: Creates a new qcow2 with the original as a backing file (copy-on-write).

use crate::config::{VmConfig, VmConfigIo};
use crate::connection::HypervisorConnection;
use crate::error::{VmmError, VmmResult};
use std::process::Command;
use tracing::{info, warn};
use uuid::Uuid;

/// Clone type selection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CloneType {
    /// Independent copy of the entire disk image
    Full,
    /// Copy-on-write clone using backing file (fast, saves space)
    Linked,
}

impl std::fmt::Display for CloneType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloneType::Full => write!(f, "Full Clone"),
            CloneType::Linked => write!(f, "Linked Clone"),
        }
    }
}

/// Progress callback for clone operations.
pub type CloneProgressFn = Box<dyn Fn(CloneProgress) + Send>;

/// Clone progress information.
#[derive(Debug, Clone)]
pub struct CloneProgress {
    pub stage: String,
    pub percent: f32,
    pub done: bool,
    pub error: Option<String>,
}

/// Clone a VM, creating a new independent VM from the source.
pub fn clone_vm(
    conn: &HypervisorConnection,
    source_config: &VmConfig,
    new_name: &str,
    clone_type: &CloneType,
) -> VmmResult<VmConfig> {
    info!(
        "Cloning VM '{}' as '{}' ({})",
        source_config.name, new_name, clone_type
    );

    // Create new config with fresh UUID
    let new_id = Uuid::new_v4();
    let new_disk_path = format!("{}/{}.qcow2", VmConfig::default_vm_dir(), new_id);

    // SECURITY (CWE-88): Validate that the new name won't start with '-' after sanitization,
    // which could cause argument injection in downstream virsh/qemu-img invocations.
    let sanitized_name = crate::config::sanitize_vm_name(new_name);
    if sanitized_name.starts_with('-') {
        return Err(VmmError::CloneError(
            "VM name must not start with '-' (argument injection risk)".to_string(),
        ));
    }

    // Clone the disk image
    match clone_type {
        CloneType::Full => {
            full_clone_disk(&source_config.disk_path, &new_disk_path)?;
        },
        CloneType::Linked => {
            linked_clone_disk(&source_config.disk_path, &new_disk_path)?;
        },
    }

    // Fix permissions on new disk
    fix_disk_permissions(&new_disk_path);

    // Build new config based on source
    let mut new_config = source_config.clone();
    new_config.id = new_id;
    new_config.name = sanitized_name;
    new_config.disk_path = new_disk_path.clone();
    // Clear network MACs so libvirt generates new ones
    for nic in &mut new_config.network_interfaces {
        nic.mac.clear();
    }
    // Don't autostart clones by default
    new_config.autostart = false;

    // SECURITY (CWE-459): Clean up orphaned disk on partial failure.
    // If disk copy succeeded but VM creation fails, remove the cloned disk
    // to prevent orphaned files accumulating on disk.
    if let Err(e) = conn.create_vm_from_existing(&new_config) {
        warn!(
            "VM creation failed after disk clone, cleaning up orphaned disk: {}",
            new_disk_path
        );
        if let Err(rm_err) = std::fs::remove_file(&new_disk_path) {
            warn!(
                "Failed to clean up orphaned disk '{}': {}",
                new_disk_path, rm_err
            );
        }
        return Err(e);
    }

    info!(
        "VM '{}' cloned as '{}' successfully",
        source_config.name, new_config.name
    );
    Ok(new_config)
}

/// Full clone: copy the entire disk image using qemu-img convert.
fn full_clone_disk(src: &str, dst: &str) -> VmmResult<()> {
    crate::disk::validate_disk_path(src)?;
    crate::disk::validate_disk_path(dst)?;

    // SECURITY (CWE-367): Canonicalize source path to prevent TOCTOU symlink race.
    // Between validate_disk_path (which checks for symlinks) and the qemu-img call,
    // an attacker could replace the file with a symlink to /dev/sda or /etc/shadow.
    // Canonicalize resolves the real path atomically with the kernel.
    let canonical_src = std::fs::canonicalize(src).map_err(|e| {
        VmmError::DiskError(format!("Failed to resolve source path '{}': {}", src, e))
    })?;
    let canonical_src_str = canonical_src
        .to_str()
        .ok_or_else(|| VmmError::DiskError("Source path contains invalid UTF-8".to_string()))?;

    info!("Full clone: {} -> {}", canonical_src_str, dst);

    // Ensure destination directory exists
    if let Some(parent) = std::path::Path::new(dst).parent() {
        std::fs::create_dir_all(parent)?;
    }

    // SECURITY (CWE-88): Use "--" to separate options from positional arguments.
    // Without it, a path starting with "-" is interpreted as a flag by qemu-img.
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = Command::new("qemu-img")
        .args(["convert", "-O", "qcow2", "-p", "--", canonical_src_str, dst])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::DiskError(format!("qemu-img not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::DiskError(format!(
            "Full clone failed: {}",
            stderr
        )));
    }

    Ok(())
}

/// Linked clone: create a new qcow2 with the source as backing file.
fn linked_clone_disk(src: &str, dst: &str) -> VmmResult<()> {
    crate::disk::validate_disk_path(src)?;
    crate::disk::validate_disk_path(dst)?;

    // SECURITY: Canonicalize the backing file path to prevent symlink attacks (CWE-59).
    // A symlink at `src` could point to an arbitrary file (e.g., /dev/sda),
    // and qemu-img would use it as a backing file, exposing raw device contents.
    let canonical_src = std::fs::canonicalize(src).map_err(|e| {
        VmmError::DiskError(format!("Failed to resolve source path '{}': {}", src, e))
    })?;
    let canonical_src_str = canonical_src
        .to_str()
        .ok_or_else(|| VmmError::DiskError("Source path contains invalid UTF-8".to_string()))?;

    info!(
        "Linked clone: {} -> {} (backing: {})",
        dst, dst, canonical_src_str
    );

    // Ensure destination directory exists
    if let Some(parent) = std::path::Path::new(dst).parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Get virtual size of source using the canonical path to avoid a second TOCTOU window
    let info = crate::disk::disk_info(canonical_src_str)?;
    let size_bytes = info.virtual_size;

    // SECURITY (CWE-190): Sanity-check virtual_size to prevent overflow or unreasonable values.
    // qemu-img create accepts size in bytes; a zero or ludicrously large value is suspicious.
    if size_bytes == 0 || size_bytes > 64 * 1024 * 1024 * 1024 * 1024 {
        // 64 TiB upper bound — far beyond any legitimate VM disk
        return Err(VmmError::DiskError(format!(
            "Source disk virtual size is invalid or out of range: {} bytes",
            size_bytes
        )));
    }

    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = Command::new("qemu-img")
        .args([
            "create",
            "-f",
            "qcow2",
            "-F",
            "qcow2",
            "-b",
            canonical_src_str,
            dst,
            &format!("{}", size_bytes),
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::DiskError(format!("qemu-img not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::DiskError(format!(
            "Linked clone failed: {}",
            stderr
        )));
    }

    Ok(())
}

/// Fix file permissions so libvirt's qemu user can access the disk image.
/// SECURITY: Uses 0o664 + ACL instead of world-writable 0o666.
fn fix_disk_permissions(disk_path: &str) {
    use std::os::unix::fs::PermissionsExt;

    // SECURITY (CWE-59): Use symlink_metadata + refuse symlinks to prevent
    // permission changes on arbitrary files via symlink race.
    match std::fs::symlink_metadata(disk_path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            warn!("Refusing to fix permissions on symlink: {}", disk_path);
            return;
        },
        Ok(meta) => {
            let mut perms = meta.permissions();
            perms.set_mode(0o664);
            let _ = std::fs::set_permissions(disk_path, perms);
        },
        Err(_) => return,
    }

    // SECURITY (CWE-88): Use "--" to prevent disk_path from being interpreted as a flag.
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
    let _ = std::process::Command::new("setfacl")
        .args(["-m", "u:libvirt-qemu:rw", "--", disk_path])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
}
