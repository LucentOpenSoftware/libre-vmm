//! ISO library — scans and manages ISO images for VM installation.

use crate::config::{VmConfig, VmConfigIo};
use std::path::PathBuf;

/// Maximum number of ISO entries to collect across all scanned directories.
/// Prevents resource exhaustion (CWE-400) from directories with huge file counts.
const MAX_ISO_ENTRIES: usize = 10_000;

/// Information about a discovered ISO image.
#[derive(Debug, Clone)]
pub struct IsoEntry {
    /// Display name (filename without path)
    pub name: String,
    /// Full absolute path to the ISO file
    pub path: String,
    /// File size in bytes
    pub size_bytes: u64,
}

impl IsoEntry {
    /// Human-readable file size string.
    pub fn size_display(&self) -> String {
        let gb = self.size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        if gb >= 1.0 {
            format!("{:.1} GiB", gb)
        } else {
            let mb = self.size_bytes as f64 / (1024.0 * 1024.0);
            format!("{:.0} MiB", mb)
        }
    }
}

/// Scan the ISO library directory for ISO/IMG files.
/// Also accepts additional search paths.
pub fn scan_isos() -> Vec<IsoEntry> {
    let mut entries = Vec::new();
    let iso_dir = VmConfig::iso_dir();

    // Ensure the directory exists
    let _ = std::fs::create_dir_all(&iso_dir);

    // Scan the library directory
    scan_directory(&iso_dir, &mut entries);

    // Also scan common download locations
    if let Some(home) = dirs::home_dir() {
        let downloads = home.join("Downloads");
        if downloads.exists() {
            scan_directory(&downloads.display().to_string(), &mut entries);
        }
        let descargas = home.join("Descargas");
        if descargas.exists() {
            scan_directory(&descargas.display().to_string(), &mut entries);
        }
    }

    // Sort by name
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // Deduplicate by path
    entries.dedup_by(|a, b| a.path == b.path);

    entries
}

/// Scan a single directory (non-recursive) for ISO/IMG files.
fn scan_directory(dir: &str, entries: &mut Vec<IsoEntry>) {
    // SECURITY: Stop collecting if we already hit the cap (CWE-400).
    if entries.len() >= MAX_ISO_ENTRIES {
        return;
    }

    let path = PathBuf::from(dir);

    // SECURITY: Use symlink_metadata (lstat) on the directory itself so we don't
    // follow a symlinked directory into an attacker-controlled tree (CWE-59).
    let dir_meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(_) => return,
    };
    if dir_meta.file_type().is_symlink() || !dir_meta.is_dir() {
        return;
    }

    if let Ok(read_dir) = std::fs::read_dir(&path) {
        for entry in read_dir.flatten() {
            if entries.len() >= MAX_ISO_ENTRIES {
                break;
            }

            // SECURITY (CWE-59, CWE-367): Use symlink_metadata (lstat) for ALL
            // checks on this entry.  The old code called entry.file_type() to
            // reject symlinks but then called entry.metadata() which *follows*
            // symlinks — creating a TOCTOU window where an attacker swaps a
            // regular file for a symlink between the two calls.  Using
            // symlink_metadata consistently means we never follow symlinks.
            let lmeta = match std::fs::symlink_metadata(entry.path()) {
                Ok(m) => m,
                Err(_) => continue,
            };

            // Reject anything that isn't a plain regular file (symlinks,
            // directories, device nodes, sockets, etc.).
            if !lmeta.is_file() {
                continue;
            }

            let file_path = entry.path();

            let ext = file_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if ext == "iso" || ext == "img" {
                // Use the already-obtained lmeta for size — no second stat call,
                // which eliminates a second TOCTOU window.
                entries.push(IsoEntry {
                    name: file_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    path: file_path.display().to_string(),
                    size_bytes: lmeta.len(),
                });
            }
        }
    }
}

/// Get the ISO library directory path.
pub fn iso_library_dir() -> String {
    VmConfig::iso_dir()
}
