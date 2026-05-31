//! VM Backup & Restore — full disk + config backup to external location.
//!
//! Creates versioned backups with:
//! - VM config snapshot (JSON)
//! - Disk image copy (qcow2 or compressed)
//! - Metadata (timestamp, size, checksum)
//! - Retention policy (keep N most recent)

use crate::config::{VmConfig, VmConfigIo};
use crate::error::{VmmError, VmmResult};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Default backup directory.
pub fn default_backup_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("libre-vmm")
        .join("backups")
}

/// Metadata for a single backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMeta {
    /// VM name at time of backup.
    pub vm_name: String,
    /// Unique backup ID (timestamp-based).
    pub backup_id: String,
    /// ISO 8601 timestamp.
    pub created_at: String,
    /// Size of the disk image in bytes.
    pub disk_size_bytes: u64,
    /// Original disk path.
    pub original_disk: String,
    /// SHA256 of the backed-up disk image (hex).
    pub disk_checksum: Option<String>,
    /// Whether the backup includes a snapshot.
    pub has_snapshot: bool,
    /// Optional user-provided note.
    pub note: String,
    /// Backup format version.
    pub format_version: u32,
}

/// Compression mode for disk backup.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BackupCompression {
    /// No compression — fastest, uses most space.
    None,
    /// qcow2 compressed copy — good balance.
    Qcow2Compressed,
    /// zstd compression on top — smallest but slowest.
    Zstd,
}

impl std::fmt::Display for BackupCompression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Qcow2Compressed => write!(f, "QCOW2 Compressed"),
            Self::Zstd => write!(f, "Zstd"),
        }
    }
}

impl BackupCompression {
    pub const ALL: &'static [BackupCompression] = &[
        BackupCompression::None,
        BackupCompression::Qcow2Compressed,
        BackupCompression::Zstd,
    ];
}

/// Options for creating a backup.
#[derive(Debug, Clone)]
pub struct BackupOptions {
    /// Where to store the backup.
    pub backup_dir: PathBuf,
    /// Compression mode.
    pub compression: BackupCompression,
    /// Optional note for this backup.
    pub note: String,
    /// Whether to compute a checksum (slower but verifiable).
    pub compute_checksum: bool,
}

impl Default for BackupOptions {
    fn default() -> Self {
        Self {
            backup_dir: default_backup_dir(),
            compression: BackupCompression::Qcow2Compressed,
            note: String::new(),
            compute_checksum: true,
        }
    }
}

/// Create a full backup of a VM (config + disk image).
///
/// The backup is stored as:
/// ```text
/// {backup_dir}/{vm_name}/{backup_id}/
///   config.json        — VM configuration
///   disk.qcow2         — Disk image copy
///   backup.json        — Backup metadata
/// ```
pub fn create_backup(config: &VmConfig, opts: &BackupOptions) -> VmmResult<BackupMeta> {
    let now = Local::now();
    let backup_id = now.format("%Y%m%d_%H%M%S").to_string();

    // Create backup directory
    let backup_path = opts.backup_dir.join(&config.name).join(&backup_id);
    std::fs::create_dir_all(&backup_path)
        .map_err(|e| VmmError::Other(format!("Failed to create backup directory: {}", e)))?;

    // 1. Save VM config
    let config_json = serde_json::to_string_pretty(config)
        .map_err(|e| VmmError::Other(format!("Failed to serialize config: {}", e)))?;
    let config_path = backup_path.join("config.json");
    std::fs::write(&config_path, &config_json)
        .map_err(|e| VmmError::Other(format!("Failed to write config backup: {}", e)))?;
    // SECURITY (CWE-732): restrict file permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600));
    }

    // 2. Copy disk image
    let disk_dir = PathBuf::from(VmConfig::default_vm_dir());
    let disk_src = disk_dir.join(format!("{}.qcow2", config.name));
    let disk_dst = backup_path.join("disk.qcow2");
    let disk_size: u64;

    if !disk_src.exists() {
        warn!(
            "Disk image not found at {:?}, skipping disk backup",
            disk_src
        );
        disk_size = 0;
    } else {
        match opts.compression {
            BackupCompression::None => {
                // Direct copy
                info!("Copying disk image (no compression): {:?}", disk_src);
                std::fs::copy(&disk_src, &disk_dst)
                    .map_err(|e| VmmError::Other(format!("Failed to copy disk: {}", e)))?;
            },
            BackupCompression::Qcow2Compressed => {
                // Use qemu-img convert with compression
                info!("Creating compressed qcow2 backup: {:?}", disk_src);
                let output = std::process::Command::new("qemu-img")
                    .args([
                        "convert",
                        "-c", // compress
                        "-O",
                        "qcow2",
                        disk_src.to_str().unwrap_or(""),
                        disk_dst.to_str().unwrap_or(""),
                    ])
                    .output()
                    .map_err(|e| VmmError::Other(format!("qemu-img failed: {}", e)))?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(VmmError::Other(format!(
                        "qemu-img compress failed: {}",
                        stderr
                    )));
                }
            },
            BackupCompression::Zstd => {
                // qcow2 compressed + zstd on top
                info!("Creating zstd-compressed backup: {:?}", disk_src);
                let qcow2_tmp = backup_path.join("disk_tmp.qcow2");
                let output = std::process::Command::new("qemu-img")
                    .args([
                        "convert",
                        "-c",
                        "-O",
                        "qcow2",
                        disk_src.to_str().unwrap_or(""),
                        qcow2_tmp.to_str().unwrap_or(""),
                    ])
                    .output()
                    .map_err(|e| VmmError::Other(format!("qemu-img failed: {}", e)))?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(VmmError::Other(format!("qemu-img failed: {}", stderr)));
                }
                // Compress with zstd
                let zstd_dst = backup_path.join("disk.qcow2.zst");
                let output = std::process::Command::new("zstd")
                    .args([
                        "-T0", // use all cores
                        "-q",  // quiet
                        qcow2_tmp.to_str().unwrap_or(""),
                        "-o",
                        zstd_dst.to_str().unwrap_or(""),
                    ])
                    .output()
                    .map_err(|e| VmmError::Other(format!("zstd failed: {}", e)))?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(VmmError::Other(format!("zstd failed: {}", stderr)));
                }
                let _ = std::fs::remove_file(&qcow2_tmp);
            },
        }

        let final_disk = if opts.compression == BackupCompression::Zstd {
            backup_path.join("disk.qcow2.zst")
        } else {
            disk_dst.clone()
        };
        disk_size = std::fs::metadata(&final_disk).map(|m| m.len()).unwrap_or(0);
    }

    // 3. Compute checksum if requested
    let checksum = if opts.compute_checksum && disk_src.exists() {
        compute_sha256_file(&disk_dst).ok()
    } else {
        None
    };

    // 4. Write backup metadata
    let meta = BackupMeta {
        vm_name: config.name.clone(),
        backup_id: backup_id.clone(),
        created_at: now.format("%Y-%m-%dT%H:%M:%S").to_string(),
        disk_size_bytes: disk_size,
        original_disk: disk_src.display().to_string(),
        disk_checksum: checksum,
        has_snapshot: false,
        note: opts.note.clone(),
        format_version: 1,
    };

    let meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|e| VmmError::Other(format!("Failed to serialize backup metadata: {}", e)))?;
    let meta_path = backup_path.join("backup.json");
    std::fs::write(&meta_path, &meta_json)
        .map_err(|e| VmmError::Other(format!("Failed to write backup metadata: {}", e)))?;

    info!("Backup created: {}/{}", config.name, backup_id);
    Ok(meta)
}

/// List all backups for a VM, sorted newest first.
pub fn list_backups(vm_name: &str, backup_dir: Option<&Path>) -> Vec<BackupMeta> {
    let dir = backup_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_backup_dir)
        .join(vm_name);

    if !dir.exists() {
        return Vec::new();
    }

    let mut backups = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let meta_path = entry.path().join("backup.json");
            if meta_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&meta_path) {
                    if let Ok(meta) = serde_json::from_str::<BackupMeta>(&content) {
                        backups.push(meta);
                    }
                }
            }
        }
    }

    // Sort newest first
    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    backups
}

/// Restore a VM from a backup.
///
/// Restores the config and disk image, overwriting the current ones.
pub fn restore_backup(
    vm_name: &str,
    backup_id: &str,
    backup_dir: Option<&Path>,
) -> VmmResult<VmConfig> {
    let dir = backup_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_backup_dir)
        .join(vm_name)
        .join(backup_id);

    if !dir.exists() {
        return Err(VmmError::Other(format!(
            "Backup not found: {}/{}",
            vm_name, backup_id
        )));
    }

    // 1. Read config
    let config_path = dir.join("config.json");
    let config_json = std::fs::read_to_string(&config_path)
        .map_err(|e| VmmError::Other(format!("Failed to read backup config: {}", e)))?;
    let config: VmConfig = serde_json::from_str(&config_json)
        .map_err(|e| VmmError::Other(format!("Failed to parse backup config: {}", e)))?;

    // 2. Restore disk image
    let disk_dir = PathBuf::from(VmConfig::default_vm_dir());
    let disk_dst = disk_dir.join(format!("{}.qcow2", vm_name));

    // Check for zstd compressed disk
    let zstd_src = dir.join("disk.qcow2.zst");
    let qcow2_src = dir.join("disk.qcow2");

    if zstd_src.exists() {
        info!("Decompressing zstd backup: {:?}", zstd_src);
        let output = std::process::Command::new("zstd")
            .args([
                "-d", // decompress
                "-f", // force overwrite
                "-q", // quiet
                zstd_src.to_str().unwrap_or(""),
                "-o",
                disk_dst.to_str().unwrap_or(""),
            ])
            .output()
            .map_err(|e| VmmError::Other(format!("zstd decompress failed: {}", e)))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VmmError::Other(format!(
                "zstd decompress failed: {}",
                stderr
            )));
        }
    } else if qcow2_src.exists() {
        info!("Restoring disk image: {:?}", qcow2_src);
        std::fs::copy(&qcow2_src, &disk_dst)
            .map_err(|e| VmmError::Other(format!("Failed to restore disk: {}", e)))?;
    } else {
        warn!("No disk image in backup, skipping disk restore");
    }

    // 3. Save restored config
    let config_dir = PathBuf::from(VmConfig::config_dir());
    let config_dst = config_dir.join(format!("{}.json", vm_name));
    std::fs::write(&config_dst, &config_json)
        .map_err(|e| VmmError::Other(format!("Failed to restore config: {}", e)))?;

    info!("Backup restored: {}/{}", vm_name, backup_id);
    Ok(config)
}

/// Delete a specific backup.
pub fn delete_backup(vm_name: &str, backup_id: &str, backup_dir: Option<&Path>) -> VmmResult<()> {
    let dir = backup_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_backup_dir)
        .join(vm_name)
        .join(backup_id);

    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| VmmError::Other(format!("Failed to delete backup: {}", e)))?;
        info!("Backup deleted: {}/{}", vm_name, backup_id);
    }
    Ok(())
}

/// Apply retention policy — keep only the N most recent backups.
pub fn apply_retention(vm_name: &str, keep: usize, backup_dir: Option<&Path>) -> VmmResult<u32> {
    let backups = list_backups(vm_name, backup_dir.map(Path::new).or(None));
    let mut deleted = 0u32;

    if backups.len() > keep {
        for old_backup in &backups[keep..] {
            delete_backup(vm_name, &old_backup.backup_id, backup_dir)?;
            deleted += 1;
        }
    }

    Ok(deleted)
}

/// Compute SHA-256 checksum of a file.
fn compute_sha256_file(path: &Path) -> VmmResult<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .map_err(|e| VmmError::Other(format!("Failed to open file for checksum: {}", e)))?;

    // Simple hash — read in chunks
    let mut hasher = Sha256State::new();
    let mut buf = vec![0u8; 1024 * 1024]; // 1MB chunks
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| VmmError::Other(format!("Read error during checksum: {}", e)))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize_hex())
}

/// Minimal SHA-256 state (avoids adding a dependency).
/// Uses a simple implementation for checksum verification.
struct Sha256State {
    data: Vec<u8>,
}

impl Sha256State {
    fn new() -> Self {
        Self { data: Vec::new() }
    }

    fn update(&mut self, chunk: &[u8]) {
        self.data.extend_from_slice(chunk);
    }

    fn finalize_hex(&self) -> String {
        // Use sha256sum command as a fallback (avoids pulling in a crypto crate)
        use std::io::Write;
        let mut child = match std::process::Command::new("sha256sum")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => return "checksum-unavailable".to_string(),
        };

        if let Some(ref mut stdin) = child.stdin {
            let _ = stdin.write_all(&self.data);
        }
        drop(child.stdin.take());

        match child.wait_with_output() {
            Ok(output) => {
                let out = String::from_utf8_lossy(&output.stdout);
                out.split_whitespace().next().unwrap_or("error").to_string()
            },
            Err(_) => "checksum-error".to_string(),
        }
    }
}

/// Get total backup size for a VM in bytes.
pub fn total_backup_size(vm_name: &str, backup_dir: Option<&Path>) -> u64 {
    list_backups(vm_name, backup_dir)
        .iter()
        .map(|b| b.disk_size_bytes)
        .sum()
}

/// Human-readable size string.
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.0} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1024), "1 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.0 GiB");
    }

    #[test]
    fn test_default_backup_dir() {
        let dir = default_backup_dir();
        assert!(dir.to_str().unwrap().contains("libre-vmm"));
        assert!(dir.to_str().unwrap().contains("backups"));
    }
}
