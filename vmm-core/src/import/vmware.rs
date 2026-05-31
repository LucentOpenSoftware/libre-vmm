//! VMware VMX import parser.

use crate::config::{NetworkMode, OsType};
use std::path::{Path, PathBuf};

use super::ImportSource;
use super::{walk_for_extension, ImportedVm, MAX_SCAN_DEPTH};

/// Standard VMware library search roots for the given home directory.
///
/// Linux-focused — we intentionally do NOT claim to support macOS Fusion or
/// Windows Workstation install layouts here.
pub fn default_search_roots(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join("vmware"),
        home.join("Virtual Machines"),
        home.join("Documents/Virtual Machines"),
    ]
}

/// Detect the first VMware library directory that exists on this system.
pub fn detect_vmware_library() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    for d in default_search_roots(&home) {
        if d.exists() {
            return Some(d);
        }
    }
    None
}

/// Recursively scan a directory tree for VMware `.vmx` files (depth-bounded).
///
/// Errors parsing individual files are swallowed; the returned vector contains
/// every successfully parsed VM. Use this for batch library imports.
pub fn scan_vmware_library(root: &Path) -> Vec<ImportedVm> {
    let mut out = Vec::new();
    walk_for_extension(
        root,
        "vmx",
        MAX_SCAN_DEPTH,
        &mut |path| match super::parse_import(path) {
            Ok(vm) => out.push(vm),
            Err(e) => {
                tracing::debug!("skip vmx {}: {}", path.display(), e);
            },
        },
    );
    out
}

/// Parse a VMware Workstation .vmx configuration file.
pub(super) fn parse_vmware_vmx(content: &str, config_path: &Path) -> Result<ImportedVm, String> {
    let mut vm = ImportedVm::new(ImportSource::VmwareVmx);
    vm.disk_format = "vmdk".to_string();

    let conf_dir = config_path.parent().unwrap_or(Path::new("."));

    let mut guest_os = String::new();
    let mut disk_filename = String::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_lowercase();
            let value = value.trim().trim_matches('"').trim();

            match key.as_str() {
                "displayname" => {
                    vm.name = value.to_string();
                },
                "memsize" => {
                    if let Ok(mb) = value.parse::<u64>() {
                        vm.memory_mib = mb;
                    }
                },
                "numvcpus" => {
                    if let Ok(n) = value.parse::<u32>() {
                        vm.vcpus = n;
                    }
                },
                "guestos" => {
                    guest_os = value.to_lowercase();
                },
                "firmware" => {
                    vm.uefi = value.to_lowercase() == "efi";
                },
                // Disk paths — check common controller prefixes
                k if k.ends_with(".filename") && disk_filename.is_empty() => {
                    // e.g., scsi0:0.filename, sata0:0.filename, ide0:0.filename, nvme0:0.filename
                    let val = value.to_string();
                    if val.ends_with(".vmdk") {
                        disk_filename = val;
                    }
                },
                // Network
                "ethernet0.connectiontype" => {
                    vm.network_mode = match value.to_lowercase().as_str() {
                        "nat" => NetworkMode::Nat,
                        "bridged" => NetworkMode::Bridged,
                        "hostonly" => NetworkMode::HostOnly,
                        "custom" => {
                            vm.notes
                                .push("VMware custom network mapped to NAT".to_string());
                            NetworkMode::Nat
                        },
                        _ => NetworkMode::Nat,
                    };
                },
                "ethernet0.virtualdev" => {
                    vm.nic_model = match value.to_lowercase().as_str() {
                        "e1000" | "e1000e" => "e1000".to_string(),
                        "vmxnet3" => {
                            vm.warnings.push(
                                "VMware vmxnet3 NIC not supported, using virtio instead"
                                    .to_string(),
                            );
                            "virtio".to_string()
                        },
                        "vlance" => "rtl8139".to_string(),
                        other => other.to_string(),
                    };
                },
                _ => {},
            }
        }
    }

    // Map guest OS string to OsType
    vm.os_type = detect_os_from_vmware_guest(&guest_os);

    // Resolve disk path
    if !disk_filename.is_empty() {
        let disk_path = if Path::new(&disk_filename).is_absolute() {
            PathBuf::from(&disk_filename)
        } else {
            conf_dir.join(&disk_filename)
        };
        vm.disk_paths.push(disk_path);
    }

    // Fallback name
    if vm.name.is_empty() {
        vm.name = config_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("imported-vm")
            .to_string();
    }

    // VMware-specific warnings
    vm.warnings
        .push("VMware SVGA display not supported, using QXL instead".to_string());
    vm.gpu_model = "qxl".to_string();

    // Disk format warning
    if !vm.disk_paths.is_empty() {
        vm.notes.push(
            "VMDK disks can be used directly by QEMU, or convert to qcow2 for better performance"
                .to_string(),
        );
    }

    // Check disk existence
    for (i, path) in vm.disk_paths.iter().enumerate() {
        if !path.exists() {
            vm.warnings
                .push(format!("Disk {} not found: {}", i + 1, path.display()));
        }
    }

    vm.notes.push(format!(
        "Imported from VMware VMX: {}",
        config_path.display()
    ));

    Ok(vm)
}

/// Map VMware guestOS string to OsType.
pub(super) fn detect_os_from_vmware_guest(guest_os: &str) -> OsType {
    let g = guest_os.to_lowercase();
    // Check macOS before Windows because "darwin" contains "win"
    if g.contains("darwin") || g.contains("macos") || g.contains("apple") {
        OsType::MacOS
    } else if g.contains("windows") || g.contains("win") {
        OsType::Windows
    } else if g.contains("freebsd") {
        OsType::FreeBSD
    } else if g.contains("linux")
        || g.contains("ubuntu")
        || g.contains("debian")
        || g.contains("centos")
        || g.contains("rhel")
        || g.contains("fedora")
        || g.contains("suse")
        || g.contains("arch")
    {
        OsType::Linux
    } else if g.is_empty() {
        OsType::Linux
    } else {
        OsType::Other
    }
}
