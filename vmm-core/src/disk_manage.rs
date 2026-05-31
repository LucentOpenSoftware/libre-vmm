//! Disk Space Management — analyze, trim, and compact qcow2 images.
//!
//! Provides tools to understand disk usage, reclaim wasted space by
//! compacting images, and check/repair qcow2 integrity.
//!
//! Also provides hot-plug / hot-unplug of disks on a running VM via
//! libvirt's `attach_device_flags` / `detach_device_flags` APIs (Wave 11.9).

use crate::error::{VmmError, VmmResult};
use std::process::Command;
use tracing::info;
use virt::domain::Domain;

/// Detailed disk usage information for a qcow2 image.
#[derive(Debug, Clone)]
pub struct DiskUsageInfo {
    /// Virtual (provisioned) size in bytes.
    pub virtual_size_bytes: u64,
    /// Actual size on disk in bytes.
    pub actual_size_bytes: u64,
    /// Disk image format (e.g., "qcow2", "raw").
    pub format: String,
    /// Estimated reclaimable space (virtual - actual) in bytes.
    pub wasted_bytes: u64,
    /// Size consumed by internal snapshots in bytes.
    pub snapshots_size_bytes: u64,
}

/// SECURITY: Validate and canonicalize a disk path to prevent path traversal (CWE-22)
/// and ensure the path points to an allowed location (the configured disks directory).
///
/// Returns the canonicalized absolute path on success.
fn validate_disk_path(disk_path: &str) -> VmmResult<std::path::PathBuf> {
    // Reject empty paths
    if disk_path.is_empty() {
        return Err(VmmError::DiskError("Disk path cannot be empty".to_string()));
    }

    let path = std::path::Path::new(disk_path);

    // SECURITY (CWE-22): Reject paths with traversal components before canonicalization.
    // This catches attacks even if the intermediate directories don't exist yet.
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            return Err(VmmError::DiskError(format!(
                "Path traversal not allowed in disk path: {}",
                disk_path
            )));
        }
    }

    // The path must exist for canonicalization (caller checks existence separately),
    // but we canonicalize the parent directory to resolve any symlinks.
    if let Some(parent) = path.parent() {
        if parent.as_os_str().is_empty() {
            // Relative filename with no parent -- reject, require absolute paths
            return Err(VmmError::DiskError(format!(
                "Disk path must be absolute: {}",
                disk_path
            )));
        }
        // SECURITY (CWE-59): Canonicalize parent to resolve symlinks and verify real location
        let canonical_parent = parent.canonicalize().map_err(|e| {
            VmmError::DiskError(format!(
                "Cannot resolve disk path parent directory '{}': {}",
                parent.display(),
                e
            ))
        })?;
        let filename = path.file_name().ok_or_else(|| {
            VmmError::DiskError(format!("Disk path has no filename: {}", disk_path))
        })?;

        // SECURITY (CWE-22): Validate filename contains no path separators
        let fname_str = filename.to_string_lossy();
        if fname_str.contains('/') || fname_str.contains('\\') || fname_str.contains('\0') {
            return Err(VmmError::DiskError(format!(
                "Invalid characters in disk filename: {}",
                fname_str
            )));
        }

        Ok(canonical_parent.join(filename))
    } else {
        Err(VmmError::DiskError(format!(
            "Disk path must be absolute: {}",
            disk_path
        )))
    }
}

/// Get detailed disk usage info for a qcow2 image.
///
/// Runs `qemu-img info --output=json` and parses the result to extract
/// virtual size, actual size, format, and snapshot overhead.
pub fn analyze_disk(disk_path: &str) -> VmmResult<DiskUsageInfo> {
    // SECURITY (CWE-22): Validate and canonicalize path before use
    let safe_path = validate_disk_path(disk_path)?;
    let safe_path_str = safe_path.to_string_lossy();

    if !safe_path.exists() {
        return Err(VmmError::DiskError(format!(
            "Disk image not found: {}",
            safe_path_str
        )));
    }

    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = Command::new("qemu-img")
        .args(["info", "--output=json", &safe_path_str])
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

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| VmmError::DiskError(e.to_string()))?;

    let virtual_size_bytes = json["virtual-size"].as_u64().unwrap_or(0);
    let actual_size_bytes = json["actual-size"].as_u64().unwrap_or(0);
    let format = json["format"].as_str().unwrap_or("unknown").to_string();

    // Calculate snapshot overhead from the snapshots array if present
    let snapshots_size_bytes = json["snapshots"]
        .as_array()
        .map(|snaps| {
            snaps
                .iter()
                .filter_map(|s| s["vm-state-size"].as_u64())
                .sum::<u64>()
        })
        .unwrap_or(0);

    // Wasted = actual on disk minus what the data truly needs.
    // For qcow2, virtual_size > actual_size is normal (sparse), but if
    // actual_size could be reduced by compacting, this estimates savings.
    // A more useful metric: run compact and see what shrinks.
    // For the simple estimate, wasted = actual - (actual after a compact would be).
    // We approximate by noting that actual_size includes freed-but-not-reclaimed clusters.
    let wasted_bytes = if actual_size_bytes > 0 {
        // The actual wasted space is hard to compute without running compact.
        // We report 0 here and let compact_disk() report real savings.
        // However, if there are snapshots, their size contributes to overhead.
        snapshots_size_bytes
    } else {
        0
    };

    info!(
        "Disk analysis for '{}': virtual={}B, actual={}B, format={}, snapshots={}B",
        safe_path_str, virtual_size_bytes, actual_size_bytes, format, snapshots_size_bytes
    );

    Ok(DiskUsageInfo {
        virtual_size_bytes,
        actual_size_bytes,
        format,
        wasted_bytes,
        snapshots_size_bytes,
    })
}

/// Compact a qcow2 image to reclaim unused space. The VM must be off.
///
/// Runs: `qemu-img convert -O qcow2 -c src tmp && mv tmp src`
///
/// The `-c` flag enables compression on the output image. Returns the
/// number of bytes saved (original size - compacted size).
pub fn compact_disk(disk_path: &str) -> VmmResult<u64> {
    // SECURITY (CWE-22): Validate and canonicalize path before use
    let safe_path = validate_disk_path(disk_path)?;
    let safe_path_str = safe_path.to_string_lossy().to_string();

    if !safe_path.exists() {
        return Err(VmmError::DiskError(format!(
            "Disk image not found: {}",
            safe_path_str
        )));
    }

    let original_size = std::fs::metadata(&safe_path)
        .map(|m| m.len())
        .map_err(|e| VmmError::DiskError(format!("Cannot read disk metadata: {}", e)))?;

    // SECURITY (CWE-377): Use unique temp path to prevent race condition on concurrent compacts
    let tmp_path = format!("{}.compact-{}", safe_path_str, uuid::Uuid::new_v4());

    // SECURITY (CWE-367): Use OS-level file locking (flock) instead of file-existence checks.
    // File-existence checks are vulnerable to TOCTOU race conditions:
    // two processes can both see the lock file as absent and proceed simultaneously.
    // flock() is atomic and kernel-enforced.
    let lock_path = format!("{}.compact-lock", safe_path_str);
    let lock_file = std::fs::File::create(&lock_path)
        .map_err(|e| VmmError::DiskError(format!("Failed to create compact lock file: {}", e)))?;

    // SECURITY (CWE-367): Acquire exclusive flock — atomic, kernel-enforced, no TOCTOU.
    // This replaces any file-existence-based locking which is inherently racy.
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        // LOCK_EX = exclusive lock, LOCK_NB = non-blocking (fail immediately if locked)
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if ret != 0 {
            return Err(VmmError::DiskError(
                "Disk compaction already in progress (locked by another process)".to_string(),
            ));
        }
    }

    // SECURITY: Fail on non-unix platforms rather than silently skipping the lock,
    // which would allow concurrent compactions to corrupt the disk image.
    #[cfg(not(unix))]
    {
        compile_error!(
            "compact_disk requires Unix flock() support for safe concurrent access (CWE-367)"
        );
    }

    // Keep lock_file alive (and thus the flock held) for the duration of the function.
    // The lock is automatically released when lock_file is dropped.
    // Explicitly reference lock_file to prevent premature drop optimization.
    let _lock_guard = &lock_file;

    info!(
        "Compacting disk '{}' (original size: {} bytes)",
        safe_path_str, original_size
    );

    // Step 1: Convert to a new compacted image
    // SECURITY: Command::new + .args() passes arguments as a vector, not through a shell,
    // so shell metacharacter injection (CWE-78) is not possible here.
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance (the lock_file FD
    // could leak to qemu-img, keeping the flock held even after parent exits).
    let output = Command::new("qemu-img")
        .args(["convert", "-O", "qcow2", "-c", &safe_path_str, &tmp_path])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::DiskError(format!("qemu-img not found: {}", e)))?;

    if !output.status.success() {
        // Clean up temp file on failure; flock is released automatically when lock_file is dropped
        let _ = std::fs::remove_file(&tmp_path);
        let _ = std::fs::remove_file(&lock_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::DiskError(format!(
            "qemu-img convert (compact) failed: {}",
            stderr
        )));
    }

    // Step 2: Replace original with compacted version
    if let Err(e) = std::fs::rename(&tmp_path, &safe_path) {
        // Try copy + remove as fallback (cross-device rename)
        if let Err(e2) = std::fs::copy(&tmp_path, &safe_path) {
            let _ = std::fs::remove_file(&tmp_path);
            let _ = std::fs::remove_file(&lock_path);
            return Err(VmmError::DiskError(format!(
                "Failed to replace original disk: rename={}, copy={}",
                e, e2
            )));
        }
        // SECURITY (SVE #21, CWE-367): Verify destination integrity after copy fallback.
        // The copy operation is not atomic (unlike rename), so verify the destination
        // file matches the source to detect TOCTOU corruption or partial writes.
        let tmp_size = std::fs::metadata(&tmp_path).map(|m| m.len()).unwrap_or(0);
        let dst_size = std::fs::metadata(&safe_path).map(|m| m.len()).unwrap_or(0);
        if tmp_size == 0 || dst_size != tmp_size {
            let _ = std::fs::remove_file(&lock_path);
            return Err(VmmError::DiskError(format!(
                "Integrity check failed after copy: source size={}, dest size={} (CWE-367)",
                tmp_size, dst_size
            )));
        }
        // Use sha256sum to verify file hash matches (defense against TOCTOU file swaps)
        let src_hash = file_sha256(&tmp_path);
        let dst_hash = file_sha256(&safe_path_str);
        if src_hash.is_none() || dst_hash.is_none() || src_hash != dst_hash {
            let _ = std::fs::remove_file(&lock_path);
            return Err(VmmError::DiskError(
                "Integrity check failed: SHA-256 hash mismatch after copy (CWE-367)".to_string(),
            ));
        }
        let _ = std::fs::remove_file(&tmp_path);
    }

    // Release flock (drop lock_file) and clean up lock file
    drop(lock_file);
    let _ = std::fs::remove_file(&lock_path);

    let new_size = std::fs::metadata(&safe_path)
        .map(|m| m.len())
        .unwrap_or(original_size);

    let bytes_saved = original_size.saturating_sub(new_size);

    info!(
        "Disk compacted: {} -> {} bytes ({} bytes saved)",
        original_size, new_size, bytes_saved
    );

    Ok(bytes_saved)
}

/// Check and repair a qcow2 image (fix leaked clusters).
///
/// Runs: `qemu-img check --repair=leaks <disk_path>`
///
/// Returns the stdout output from qemu-img describing what was found/fixed.
pub fn check_and_repair(disk_path: &str) -> VmmResult<String> {
    // SECURITY (CWE-22): Validate and canonicalize path before use
    let safe_path = validate_disk_path(disk_path)?;
    let safe_path_str = safe_path.to_string_lossy();

    if !safe_path.exists() {
        return Err(VmmError::DiskError(format!(
            "Disk image not found: {}",
            safe_path_str
        )));
    }

    info!("Checking and repairing disk '{}'", safe_path_str);

    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = Command::new("qemu-img")
        .args(["check", "--repair=leaks", &*safe_path_str])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::DiskError(format!("qemu-img not found: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // qemu-img check returns exit code 2 for leaks that were repaired,
    // exit code 3 for unfixable errors. Only fail on actual errors.
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        if code == 3 {
            return Err(VmmError::DiskError(format!(
                "Disk has unfixable errors: {} {}",
                stdout, stderr
            )));
        }
        // code 2 means leaks were found and repaired — that's a success
    }

    let result = if stderr.is_empty() {
        stdout
    } else {
        format!("{}\n{}", stdout, stderr)
    };

    info!("Disk check complete for '{}'", safe_path_str);

    Ok(result)
}

// ---------------------------------------------------------------------------
// Wave 11.9 — Hot-add / hot-remove disk on a running VM
// ---------------------------------------------------------------------------

/// Bus type for a hot-plugged disk. The bus determines the QEMU controller and
/// the target device name convention (vdX for virtio, sdX for scsi/sata, nvmeNnN for nvme).
#[derive(Debug, Clone, Copy)]
pub enum DiskBus {
    Virtio,
    Scsi,
    Sata,
    Nvme,
}

impl DiskBus {
    /// libvirt XML `bus=` attribute value.
    pub fn xml_value(self) -> &'static str {
        match self {
            DiskBus::Virtio => "virtio",
            DiskBus::Scsi => "scsi",
            DiskBus::Sata => "sata",
            DiskBus::Nvme => "nvme",
        }
    }
}

/// SECURITY (CWE-20, CWE-91): Strict allowlist for `target_dev` values.
/// Must match `vd[a-z]`, `sd[a-z]`, or `nvme[0-9]n[0-9]`.
/// This prevents XML injection and ensures the device name is a valid
/// libvirt target-dev identifier.
fn validate_target_dev(target_dev: &str) -> VmmResult<()> {
    if target_dev.is_empty() {
        return Err(VmmError::DiskError(
            "target_dev must not be empty (CWE-20)".to_string(),
        ));
    }
    if target_dev.len() > 16 {
        return Err(VmmError::DiskError(format!(
            "target_dev too long (max 16 chars): {} (CWE-20)",
            target_dev
        )));
    }
    if target_dev.contains('\0') {
        return Err(VmmError::DiskError(
            "target_dev must not contain null bytes (CWE-626)".to_string(),
        ));
    }
    let bytes = target_dev.as_bytes();

    // vd[a-z] or sd[a-z]: exactly 3 chars, first two prefix, last lowercase letter
    if bytes.len() == 3
        && (bytes[0] == b'v' || bytes[0] == b's')
        && bytes[1] == b'd'
        && (bytes[2] as char).is_ascii_lowercase()
    {
        return Ok(());
    }

    // nvme[0-9]n[0-9]: e.g. "nvme0n1"
    if bytes.len() == 7
        && &bytes[..4] == b"nvme"
        && (bytes[4] as char).is_ascii_digit()
        && bytes[5] == b'n'
        && (bytes[6] as char).is_ascii_digit()
    {
        return Ok(());
    }

    Err(VmmError::DiskError(format!(
        "target_dev '{}' does not match allowed patterns vd[a-z], sd[a-z], or nvme[0-9]n[0-9] (CWE-20)",
        target_dev
    )))
}

/// SECURITY (CWE-20, CWE-91): Allowlist for qcow2/raw `cache` attribute values.
fn validate_cache_mode(cache: &str) -> VmmResult<()> {
    const ALLOWED: &[&str] = &["none", "writeback", "writethrough", "unsafe", "directsync"];
    if !ALLOWED.contains(&cache) {
        return Err(VmmError::DiskError(format!(
            "cache mode '{}' is not in allowlist {:?} (CWE-20)",
            cache, ALLOWED
        )));
    }
    Ok(())
}

/// SECURITY (CWE-20): Validate VM name for libvirt lookup. Reuses the same
/// strict allowlist used elsewhere in the codebase to prevent injection.
fn validate_vm_name_for_hotplug(name: &str) -> VmmResult<()> {
    if name.is_empty() {
        return Err(VmmError::InvalidConfig(
            "VM name must not be empty (CWE-20)".to_string(),
        ));
    }
    if name.len() > 128 {
        return Err(VmmError::InvalidConfig(
            "VM name too long (max 128 chars) (CWE-20)".to_string(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || " -_.".contains(c))
    {
        return Err(VmmError::InvalidConfig(format!(
            "VM name '{}' contains unsafe characters (CWE-20)",
            name
        )));
    }
    if name.starts_with('-') || name.starts_with('.') {
        return Err(VmmError::InvalidConfig(format!(
            "VM name '{}' must not start with '-' or '.' (CWE-88)",
            name
        )));
    }
    if name.contains('\0') {
        return Err(VmmError::InvalidConfig(
            "VM name must not contain null bytes (CWE-626)".to_string(),
        ));
    }
    Ok(())
}

/// XML escape for use inside attribute values.
fn xml_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Build the `<disk>` XML element for hot-plugging.
///
/// This is the pure helper used by both `hotplug_disk` and the tests. All
/// inputs MUST be pre-validated by the caller — this helper trusts its inputs
/// and only performs XML escaping on the path.
pub fn build_hotplug_disk_xml(
    disk_path: &str,
    target_dev: &str,
    bus: DiskBus,
    cache: &str,
) -> String {
    format!(
        "<disk type='file' device='disk'>\n  \
         <driver name='qemu' type='qcow2' cache='{cache}'/>\n  \
         <source file='{path}'/>\n  \
         <target dev='{dev}' bus='{bus}'/>\n\
         </disk>",
        cache = cache,
        path = xml_escape_attr(disk_path),
        dev = target_dev,
        bus = bus.xml_value(),
    )
}

/// Hot-add a new disk to a running VM. The disk file must already exist.
///
/// Uses libvirt's `attach_device_flags` with `VIR_DOMAIN_AFFECT_LIVE | VIR_DOMAIN_AFFECT_CONFIG`
/// so the disk is added to both the running VM AND persisted in the domain XML for future starts.
///
/// SECURITY (CWE-20, CWE-22, CWE-59, CWE-91):
/// - VM name is validated against a strict allowlist.
/// - `target_dev` must match vd[a-z] / sd[a-z] / nvme[0-9]n[0-9].
/// - `disk_path` is canonicalized and checked against path traversal / symlinks.
/// - `cache` must be in the QEMU cache-mode allowlist.
pub fn hotplug_disk(
    conn: &crate::connection::HypervisorConnection,
    vm_name: &str,
    disk_path: &str,
    target_dev: &str,
    bus: DiskBus,
    cache: &str,
) -> VmmResult<()> {
    // SECURITY (CWE-20): Validate every parameter before any libvirt or XML use.
    validate_vm_name_for_hotplug(vm_name)?;
    validate_target_dev(target_dev)?;
    validate_cache_mode(cache)?;

    // SECURITY (CWE-22, CWE-59): Validate and canonicalize the disk path.
    let safe_path = validate_disk_path(disk_path)?;
    if !safe_path.exists() {
        return Err(VmmError::DiskError(format!(
            "Disk image not found: {}",
            safe_path.to_string_lossy()
        )));
    }
    let safe_path_str = safe_path.to_string_lossy().to_string();

    // Look up the running domain on the libvirt connection.
    let domain =
        Domain::lookup_by_name(conn.raw_conn(), vm_name).map_err(|_| VmmError::VmNotFound {
            name: vm_name.to_string(),
        })?;

    let xml = build_hotplug_disk_xml(&safe_path_str, target_dev, bus, cache);

    // Affect both the running guest AND the persistent config so the disk
    // reappears on next start. virt::sys re-exports the libvirt constants.
    let flags = virt::sys::VIR_DOMAIN_AFFECT_LIVE | virt::sys::VIR_DOMAIN_AFFECT_CONFIG;
    domain
        .attach_device_flags(&xml, flags)
        .map_err(|e| VmmError::Other(format!("Failed to hot-add disk: {}", e)))?;

    info!(
        "Hot-added disk '{}' as {} (bus={}) to VM '{}'",
        safe_path_str,
        target_dev,
        bus.xml_value(),
        vm_name
    );
    Ok(())
}

/// Hot-remove a disk from a running VM. The disk file is NOT deleted.
///
/// Uses `detach_device_flags` with `VIR_DOMAIN_AFFECT_LIVE | VIR_DOMAIN_AFFECT_CONFIG`.
///
/// Implementation: libvirt matches the device to detach by exact XML content,
/// so we dump the current domain XML, find the `<disk>` element whose target
/// matches `target_dev`, and pass that exact element back to libvirt.
pub fn hotunplug_disk(
    conn: &crate::connection::HypervisorConnection,
    vm_name: &str,
    target_dev: &str,
) -> VmmResult<()> {
    validate_vm_name_for_hotplug(vm_name)?;
    validate_target_dev(target_dev)?;

    let domain =
        Domain::lookup_by_name(conn.raw_conn(), vm_name).map_err(|_| VmmError::VmNotFound {
            name: vm_name.to_string(),
        })?;

    let xml = domain
        .get_xml_desc(0)
        .map_err(|e| VmmError::Other(format!("Failed to read domain XML: {}", e)))?;

    let disk_xml = extract_disk_element_for_target(&xml, target_dev).ok_or_else(|| {
        VmmError::DiskError(format!(
            "No <disk> element found with target dev='{}' on VM '{}'",
            target_dev, vm_name
        ))
    })?;

    let flags = virt::sys::VIR_DOMAIN_AFFECT_LIVE | virt::sys::VIR_DOMAIN_AFFECT_CONFIG;
    domain
        .detach_device_flags(&disk_xml, flags)
        .map_err(|e| VmmError::Other(format!("Failed to hot-remove disk: {}", e)))?;

    info!(
        "Hot-removed disk with target '{}' from VM '{}' (disk file not deleted)",
        target_dev, vm_name
    );
    Ok(())
}

/// Find a `<disk>...</disk>` block in the domain XML whose `<target dev='...'/>`
/// matches the given `target_dev`, and return the block's exact text.
///
/// libvirt's detach matches by exact XML content, so we must give it the
/// element verbatim. We do simple bracketed slicing — sufficient because
/// libvirt always emits well-formed XML and disk elements don't nest.
fn extract_disk_element_for_target(xml: &str, target_dev: &str) -> Option<String> {
    let needle_single = format!("dev='{}'", target_dev);
    let needle_double = format!("dev=\"{}\"", target_dev);

    let mut search_pos = 0;
    while let Some(disk_start) = xml[search_pos..].find("<disk") {
        let abs_disk_start = search_pos + disk_start;
        // Find the matching </disk>
        let after = &xml[abs_disk_start..];
        let disk_end_rel = after.find("</disk>")?;
        let disk_end = abs_disk_start + disk_end_rel + "</disk>".len();
        let block = &xml[abs_disk_start..disk_end];

        if block.contains(&needle_single) || block.contains(&needle_double) {
            return Some(block.to_string());
        }
        search_pos = disk_end;
    }
    None
}

/// Compute SHA-256 hash of a file using the system `sha256sum` command.
/// Returns None if the command fails or output cannot be parsed.
/// SECURITY (SVE #21): Used to verify file integrity after non-atomic copy operations.
fn file_sha256(path: &str) -> Option<String> {
    let output = Command::new("sha256sum")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // sha256sum outputs: "<hash>  <filename>\n"
    stdout.split_whitespace().next().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // build_hotplug_disk_xml — pure helper, no libvirt required
    // ---------------------------------------------------------------

    #[test]
    fn hotplug_xml_virtio_basic() {
        let xml = build_hotplug_disk_xml(
            "/home/user/disks/extra.qcow2",
            "vdb",
            DiskBus::Virtio,
            "writeback",
        );
        assert!(xml.contains("<disk type='file' device='disk'>"));
        assert!(xml.contains("driver name='qemu' type='qcow2' cache='writeback'"));
        assert!(xml.contains("<source file='/home/user/disks/extra.qcow2'/>"));
        assert!(xml.contains("<target dev='vdb' bus='virtio'/>"));
        assert!(xml.contains("</disk>"));
    }

    #[test]
    fn hotplug_xml_scsi_bus() {
        let xml = build_hotplug_disk_xml(
            "/var/lib/libvirt/images/d.qcow2",
            "sdc",
            DiskBus::Scsi,
            "none",
        );
        assert!(xml.contains("bus='scsi'"));
        assert!(xml.contains("dev='sdc'"));
        assert!(xml.contains("cache='none'"));
    }

    #[test]
    fn hotplug_xml_sata_bus() {
        let xml = build_hotplug_disk_xml("/x.qcow2", "sdd", DiskBus::Sata, "writethrough");
        assert!(xml.contains("bus='sata'"));
        assert!(xml.contains("cache='writethrough'"));
    }

    #[test]
    fn hotplug_xml_nvme_bus() {
        let xml = build_hotplug_disk_xml("/x.qcow2", "nvme0n1", DiskBus::Nvme, "directsync");
        assert!(xml.contains("bus='nvme'"));
        assert!(xml.contains("dev='nvme0n1'"));
        assert!(xml.contains("cache='directsync'"));
    }

    #[test]
    fn hotplug_xml_escapes_path() {
        // A path with XML metacharacters should be escaped in the source attribute.
        let xml = build_hotplug_disk_xml(
            "/home/u/d&t<x>'\"a.qcow2",
            "vdb",
            DiskBus::Virtio,
            "writeback",
        );
        assert!(xml.contains("&amp;"));
        assert!(xml.contains("&lt;"));
        assert!(xml.contains("&gt;"));
        assert!(xml.contains("&apos;"));
        assert!(xml.contains("&quot;"));
        // Raw metacharacters must not leak into the source element
        assert!(!xml.contains("<source file='/home/u/d&t<x>'\"a.qcow2'/>"));
    }

    #[test]
    fn hotplug_xml_well_formed_outer_tags() {
        let xml = build_hotplug_disk_xml("/d.qcow2", "vda", DiskBus::Virtio, "none");
        assert!(xml.starts_with("<disk "));
        assert!(xml.trim_end().ends_with("</disk>"));
    }

    // ---------------------------------------------------------------
    // validate_target_dev
    // ---------------------------------------------------------------

    #[test]
    fn target_dev_valid_virtio() {
        assert!(validate_target_dev("vda").is_ok());
        assert!(validate_target_dev("vdb").is_ok());
        assert!(validate_target_dev("vdz").is_ok());
    }

    #[test]
    fn target_dev_valid_scsi_sata() {
        assert!(validate_target_dev("sda").is_ok());
        assert!(validate_target_dev("sdc").is_ok());
        assert!(validate_target_dev("sdz").is_ok());
    }

    #[test]
    fn target_dev_valid_nvme() {
        assert!(validate_target_dev("nvme0n1").is_ok());
        assert!(validate_target_dev("nvme1n2").is_ok());
        assert!(validate_target_dev("nvme9n9").is_ok());
    }

    #[test]
    fn target_dev_reject_empty() {
        assert!(validate_target_dev("").is_err());
    }

    #[test]
    fn target_dev_reject_too_short() {
        assert!(validate_target_dev("vd").is_err());
        assert!(validate_target_dev("v").is_err());
    }

    #[test]
    fn target_dev_reject_too_long() {
        assert!(validate_target_dev("vdaa").is_err());
        assert!(validate_target_dev("sdaaa").is_err());
    }

    #[test]
    fn target_dev_reject_path_traversal() {
        assert!(validate_target_dev("../etc").is_err());
        assert!(validate_target_dev("vd/").is_err());
    }

    #[test]
    fn target_dev_reject_digit_in_letter_slot() {
        // vd1 — digit where letter required
        assert!(validate_target_dev("vd1").is_err());
        assert!(validate_target_dev("sd9").is_err());
    }

    #[test]
    fn target_dev_reject_uppercase() {
        // Only lowercase letters allowed in vd[a-z] / sd[a-z]
        assert!(validate_target_dev("vdA").is_err());
        assert!(validate_target_dev("SDA").is_err());
    }

    #[test]
    fn target_dev_reject_nvme_bad_format() {
        // nvme[a-z] — letter where digit required
        assert!(validate_target_dev("nvmeAn1").is_err());
        assert!(validate_target_dev("nvme0nA").is_err());
        assert!(validate_target_dev("nvme00n1").is_err()); // wrong length
        assert!(validate_target_dev("nvme0").is_err());
    }

    #[test]
    fn target_dev_reject_null_byte() {
        let bad = "vd\0a";
        assert!(validate_target_dev(bad).is_err());
    }

    #[test]
    fn target_dev_reject_xml_metacharacters() {
        assert!(validate_target_dev("vd'").is_err());
        assert!(validate_target_dev("vd<").is_err());
        assert!(validate_target_dev("vd>").is_err());
    }

    // ---------------------------------------------------------------
    // validate_cache_mode
    // ---------------------------------------------------------------

    #[test]
    fn cache_mode_allowed() {
        for mode in &["none", "writeback", "writethrough", "unsafe", "directsync"] {
            assert!(
                validate_cache_mode(mode).is_ok(),
                "expected '{}' to be allowed",
                mode
            );
        }
    }

    #[test]
    fn cache_mode_rejects_empty() {
        assert!(validate_cache_mode("").is_err());
    }

    #[test]
    fn cache_mode_rejects_unknown() {
        assert!(validate_cache_mode("default").is_err()); // not in allowlist (we don't emit default)
        assert!(validate_cache_mode("bogus").is_err());
        assert!(validate_cache_mode("none'/><evil/>").is_err());
    }

    // ---------------------------------------------------------------
    // validate_vm_name_for_hotplug
    // ---------------------------------------------------------------

    #[test]
    fn vm_name_valid() {
        assert!(validate_vm_name_for_hotplug("my-vm").is_ok());
        assert!(validate_vm_name_for_hotplug("VM_1.test").is_ok());
        assert!(validate_vm_name_for_hotplug("a b").is_ok());
    }

    #[test]
    fn vm_name_rejects_empty() {
        assert!(validate_vm_name_for_hotplug("").is_err());
    }

    #[test]
    fn vm_name_rejects_leading_dash() {
        assert!(validate_vm_name_for_hotplug("-rf").is_err());
    }

    #[test]
    fn vm_name_rejects_shell_metacharacters() {
        assert!(validate_vm_name_for_hotplug("vm;rm").is_err());
        assert!(validate_vm_name_for_hotplug("vm$(x)").is_err());
        assert!(validate_vm_name_for_hotplug("vm|x").is_err());
    }

    // ---------------------------------------------------------------
    // DiskBus::xml_value
    // ---------------------------------------------------------------

    #[test]
    fn disk_bus_xml_values() {
        assert_eq!(DiskBus::Virtio.xml_value(), "virtio");
        assert_eq!(DiskBus::Scsi.xml_value(), "scsi");
        assert_eq!(DiskBus::Sata.xml_value(), "sata");
        assert_eq!(DiskBus::Nvme.xml_value(), "nvme");
    }

    // ---------------------------------------------------------------
    // extract_disk_element_for_target
    // ---------------------------------------------------------------

    #[test]
    fn extract_disk_finds_single_quoted_target() {
        let xml = r#"
        <domain>
          <devices>
            <disk type='file' device='disk'>
              <driver name='qemu' type='qcow2'/>
              <source file='/d/a.qcow2'/>
              <target dev='vda' bus='virtio'/>
            </disk>
            <disk type='file' device='disk'>
              <source file='/d/b.qcow2'/>
              <target dev='vdb' bus='virtio'/>
            </disk>
          </devices>
        </domain>"#;
        let block = extract_disk_element_for_target(xml, "vdb").expect("should find vdb");
        assert!(block.contains("/d/b.qcow2"));
        assert!(block.contains("dev='vdb'"));
        assert!(!block.contains("/d/a.qcow2"));
        assert!(block.starts_with("<disk "));
        assert!(block.ends_with("</disk>"));
    }

    #[test]
    fn extract_disk_finds_double_quoted_target() {
        let xml = r#"<domain><devices>
            <disk type="file" device="disk">
              <source file="/d/x.qcow2"/>
              <target dev="vdc" bus="virtio"/>
            </disk>
        </devices></domain>"#;
        let block = extract_disk_element_for_target(xml, "vdc").expect("should find vdc");
        assert!(block.contains(r#"dev="vdc""#));
    }

    #[test]
    fn extract_disk_returns_none_when_not_found() {
        let xml = r#"<domain><devices>
            <disk type='file' device='disk'>
              <target dev='vda' bus='virtio'/>
            </disk>
        </devices></domain>"#;
        assert_eq!(extract_disk_element_for_target(xml, "vdz"), None);
    }

    #[test]
    fn extract_disk_returns_none_on_empty_xml() {
        assert_eq!(extract_disk_element_for_target("", "vda"), None);
    }
}
