//! Quickemu .conf import parser.

use crate::config::{DisplayProtocol, OsType};
use std::path::{Path, PathBuf};

use super::ImportSource;
use super::ImportedVm;

/// Parse a quickemu .conf file (key=value format, similar to shell variables).
pub(super) fn parse_quickemu_conf(content: &str, config_path: &Path) -> Result<ImportedVm, String> {
    let mut vm = ImportedVm::new(ImportSource::QuickemuConf);
    let conf_dir = config_path.parent().unwrap_or(Path::new("."));

    let mut guest_os = String::new();
    let mut disk_img = String::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');

            match key {
                "guest_os" => guest_os = value.to_string(),
                "ram" => {
                    vm.memory_mib = parse_ram_string(value);
                },
                "cpu_cores" => {
                    if let Ok(v) = value.parse::<u32>() {
                        vm.vcpus = v;
                    }
                },
                "disk_img" => disk_img = value.to_string(),
                "boot" => {
                    vm.uefi = value == "efi" || value == "uefi";
                },
                "tpm" => {
                    vm.tpm = value == "on" || value == "true" || value == "yes";
                },
                "display" => {
                    vm.display_protocol = match value {
                        "spice" => DisplayProtocol::Spice,
                        _ => DisplayProtocol::Vnc,
                    };
                },
                _ => {},
            }
        }
    }

    // Resolve disk path
    if !disk_img.is_empty() {
        let p = PathBuf::from(&disk_img);
        let disk_path = if p.is_absolute() { p } else { conf_dir.join(p) };
        vm.disk_paths.push(disk_path);
    }

    // Map guest_os to OsType
    vm.os_type = match guest_os.as_str() {
        "windows" => OsType::Windows,
        "macos" => OsType::MacOS,
        "freebsd" => OsType::FreeBSD,
        "linux" | "" => OsType::Linux,
        _ => OsType::Other,
    };

    // Derive name from filename
    vm.name = config_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported-vm")
        .to_string();

    vm.notes
        .push(format!("Imported from Quickemu: {}", config_path.display()));

    Ok(vm)
}

/// Parse a RAM string like "4G", "2048M", or "2048" into MiB.
pub(super) fn parse_ram_string(ram: &str) -> u64 {
    let ram = ram.trim();
    if ram.is_empty() {
        return 2048;
    }
    if let Some(gb) = ram.strip_suffix('G') {
        gb.trim().parse::<u64>().unwrap_or(2048) * 1024
    } else if let Some(mb) = ram.strip_suffix('M') {
        mb.trim().parse::<u64>().unwrap_or(2048)
    } else {
        ram.parse::<u64>().unwrap_or(2048)
    }
}
