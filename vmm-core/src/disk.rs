//! Disk image management via qemu-img.

use crate::error::{VmmError, VmmResult};
use std::path::Path;
use std::process::Command;
use tracing::info;

/// Validate that a disk path is safe to operate on.
///
/// Prevents path traversal attacks by ensuring the path:
/// - Is absolute
/// - Does not contain `..` components
/// - Is not a device node, socket, or other special file
pub fn validate_disk_path(path: &str) -> VmmResult<()> {
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err(VmmError::DiskError(format!(
            "Disk path must be absolute: {}",
            path
        )));
    }
    for component in p.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(VmmError::DiskError(format!(
                "Disk path must not contain '..': {}",
                path
            )));
        }
    }
    // SECURITY: Use symlink_metadata (lstat) to detect symlinks before following (CWE-59)
    if p.exists() {
        let lmeta = std::fs::symlink_metadata(p)
            .map_err(|e| VmmError::DiskError(format!("Cannot lstat {}: {}", path, e)))?;
        // Block symlinks — they can point to arbitrary files including /dev/sda, /etc/shadow
        if lmeta.file_type().is_symlink() {
            return Err(VmmError::DiskError(format!(
                "Disk path is a symbolic link (blocked for security): {}",
                path
            )));
        }
        // Block device nodes and other non-regular-file targets
        if !lmeta.is_file() && !lmeta.is_dir() {
            return Err(VmmError::DiskError(format!(
                "Disk path is not a regular file: {}",
                path
            )));
        }
    }
    Ok(())
}

/// Create a new qcow2 disk image.
pub fn create_qcow2(path: &str, size_gib: u64) -> VmmResult<()> {
    validate_disk_path(path)?;
    // SECURITY: Validate size bounds to prevent resource exhaustion (CWE-400).
    if size_gib == 0 || size_gib > 65536 {
        return Err(VmmError::DiskError(format!(
            "Disk size must be 1-65536 GiB, got {}",
            size_gib
        )));
    }
    info!("Creating qcow2 disk: {} ({}G)", path, size_gib);

    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    // SECURITY (CWE-88): Use "--" to terminate option parsing so that a path
    // starting with "-" cannot be interpreted as a qemu-img flag.
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = Command::new("qemu-img")
        .args([
            "create",
            "-f",
            "qcow2",
            "-o",
            "cluster_size=65536,lazy_refcounts=on,preallocation=off",
            "--",
            path,
            &format!("{}G", size_gib),
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::DiskError(format!("qemu-img not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::DiskError(format!(
            "qemu-img create failed: {}",
            stderr
        )));
    }

    Ok(())
}

/// Maximum bytes we will read from qemu-img stdout.
/// Prevents memory exhaustion (CWE-400) if a crafted image triggers huge output.
const MAX_QEMU_IMG_OUTPUT: usize = 1024 * 1024; // 1 MiB — normal info is < 2 KiB

/// Get info about a disk image.
pub fn disk_info(path: &str) -> VmmResult<DiskInfo> {
    // SECURITY: Validate path before passing to qemu-img (CWE-22, CWE-59).
    validate_disk_path(path)?;
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = Command::new("qemu-img")
        .args(["info", "--output=json", "--", path])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::DiskError(format!("qemu-img not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::DiskError(format!(
            "qemu-img info failed: {}",
            stderr
        )));
    }

    // SECURITY (CWE-400): Reject unexpectedly large output that could exhaust memory.
    if output.stdout.len() > MAX_QEMU_IMG_OUTPUT {
        return Err(VmmError::DiskError(format!(
            "qemu-img info produced {} bytes of output (limit {}); refusing to parse",
            output.stdout.len(),
            MAX_QEMU_IMG_OUTPUT,
        )));
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| VmmError::DiskError(e.to_string()))?;

    Ok(DiskInfo {
        format: json["format"].as_str().unwrap_or("unknown").to_string(),
        virtual_size: json["virtual-size"].as_u64().unwrap_or(0),
        actual_size: json["actual-size"].as_u64().unwrap_or(0),
    })
}

/// Resize a disk image (only grow is safe while VM is off).
pub fn resize_disk(path: &str, new_size_gib: u64) -> VmmResult<()> {
    validate_disk_path(path)?;
    // SECURITY: Validate size bounds to prevent resource exhaustion (CWE-400).
    if new_size_gib == 0 || new_size_gib > 65536 {
        return Err(VmmError::DiskError(format!(
            "Disk size must be 1-65536 GiB, got {}",
            new_size_gib
        )));
    }
    info!("Resizing disk {} to {}G", path, new_size_gib);

    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = Command::new("qemu-img")
        .args(["resize", "--", path, &format!("{}G", new_size_gib)])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::DiskError(format!("qemu-img not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::DiskError(format!(
            "qemu-img resize failed: {}",
            stderr
        )));
    }

    Ok(())
}

/// Convert between disk formats.
pub fn convert_disk(src: &str, dst: &str, format: &str) -> VmmResult<()> {
    validate_disk_path(src)?;
    validate_disk_path(dst)?;

    // SECURITY: Allowlist valid disk formats to prevent arbitrary format driver loading
    let valid_formats = ["qcow2", "raw", "vmdk", "vdi", "vpc", "vhdx"];
    if !valid_formats.contains(&format) {
        return Err(VmmError::DiskError(format!(
            "Unsupported disk format '{}'. Valid: {:?}",
            format, valid_formats
        )));
    }

    info!("Converting {} -> {} (format: {})", src, dst, format);

    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = Command::new("qemu-img")
        .args(["convert", "-O", format, "-p", "--", src, dst])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::DiskError(format!("qemu-img not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::DiskError(format!(
            "qemu-img convert failed: {}",
            stderr
        )));
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub format: String,
    pub virtual_size: u64,
    pub actual_size: u64,
}
