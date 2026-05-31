//! VirtualBox .vbox XML import parser.

use crate::config::{NetworkMode, OsType};
use std::path::{Path, PathBuf};

use super::libvirt::xml_attr_value;
use super::ImportSource;
use super::{walk_for_extension, ImportedVm, MAX_SCAN_DEPTH};

/// Standard VirtualBox library search roots for the given home directory.
pub fn default_search_roots(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join("VirtualBox VMs"),
        home.join("snap/virtualbox/common/Machines"),
        home.join(".config/VirtualBox/Machines"),
    ]
}

/// Detect the first VirtualBox library that exists.
pub fn detect_vbox_library() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    for d in default_search_roots(&home) {
        if d.exists() {
            return Some(d);
        }
    }
    None
}

/// Recursively scan a directory tree for `.vbox` machine files (depth-bounded).
pub fn scan_vbox_library(root: &Path) -> Vec<ImportedVm> {
    let mut out = Vec::new();
    walk_for_extension(
        root,
        "vbox",
        MAX_SCAN_DEPTH,
        &mut |path| match super::parse_import(path) {
            Ok(vm) => out.push(vm),
            Err(e) => {
                tracing::debug!("skip vbox {}: {}", path.display(), e);
            },
        },
    );
    out
}

/// Parse a VirtualBox .vbox machine XML file using quick-xml.
pub(super) fn parse_virtualbox_vbox(xml: &str, config_path: &Path) -> Result<ImportedVm, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut vm = ImportedVm::new(ImportSource::VirtualBoxVbox);
    vm.disk_format = "vdi".to_string();

    let conf_dir = config_path.parent().unwrap_or(Path::new("."));

    let mut reader = Reader::from_str(xml);

    let mut element_stack: Vec<String> = Vec::new();
    let mut os_type_str = String::new();
    let mut has_efi = false;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                match tag.as_str() {
                    "Machine" => {
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"name" => vm.name = xml_attr_value(&attr),
                                b"OSType" => os_type_str = xml_attr_value(&attr),
                                _ => {},
                            }
                        }
                    },
                    "CPU" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"count" {
                                if let Ok(n) = xml_attr_value(&attr).parse::<u32>() {
                                    vm.vcpus = n;
                                }
                            }
                        }
                    },
                    "Adapter" => {
                        // First adapter wins
                        if vm.network_mode == NetworkMode::Nat {
                            // Check children for NAT/Bridged/etc.
                        }
                    },
                    "NAT" => {
                        let parent = element_stack.last().map(|s| s.as_str()).unwrap_or("");
                        if parent == "Adapter" {
                            vm.network_mode = NetworkMode::Nat;
                        }
                    },
                    "BridgedInterface" => {
                        vm.network_mode = NetworkMode::Bridged;
                    },
                    "HostOnlyInterface" => {
                        vm.network_mode = NetworkMode::HostOnly;
                    },
                    "InternalNetwork" => {
                        vm.network_mode = NetworkMode::HostOnly;
                        vm.notes
                            .push("VirtualBox internal network mapped to Host-Only".to_string());
                    },
                    _ => {},
                }

                element_stack.push(tag);
            },
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                match tag.as_str() {
                    "Machine" => {
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"name" => vm.name = xml_attr_value(&attr),
                                b"OSType" => os_type_str = xml_attr_value(&attr),
                                _ => {},
                            }
                        }
                    },
                    "Memory" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"RAMSize" {
                                if let Ok(mb) = xml_attr_value(&attr).parse::<u64>() {
                                    vm.memory_mib = mb;
                                }
                            }
                        }
                    },
                    "CPU" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"count" {
                                if let Ok(n) = xml_attr_value(&attr).parse::<u32>() {
                                    vm.vcpus = n;
                                }
                            }
                        }
                    },
                    "EFI" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"enabled" {
                                has_efi = xml_attr_value(&attr) == "true";
                            }
                        }
                    },
                    "HardDisk" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"location" {
                                let loc = xml_attr_value(&attr);
                                let disk_path = if Path::new(&loc).is_absolute() {
                                    PathBuf::from(&loc)
                                } else {
                                    conf_dir.join(&loc)
                                };
                                // Detect format from extension
                                if let Some(ext) = disk_path.extension().and_then(|e| e.to_str()) {
                                    vm.disk_format = ext.to_string();
                                }
                                vm.disk_paths.push(disk_path);
                            }
                        }
                    },
                    "Image" => {
                        // DVD/CD image
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"uuid" || attr.key.as_ref() == b"location" {
                                // Could track ISO references here
                            }
                        }
                    },
                    "NAT" => {
                        let parent = element_stack.last().map(|s| s.as_str()).unwrap_or("");
                        if parent == "Adapter" {
                            vm.network_mode = NetworkMode::Nat;
                        }
                    },
                    "BridgedInterface" => {
                        vm.network_mode = NetworkMode::Bridged;
                    },
                    "HostOnlyInterface" => {
                        vm.network_mode = NetworkMode::HostOnly;
                    },
                    _ => {},
                }
            },
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if element_stack.last().map(|s| s.as_str()) == Some(&tag) {
                    element_stack.pop();
                }
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("Error parsing VirtualBox XML: {}", e)),
            _ => {},
        }
        buf.clear();
    }

    // Map VBox OSType to our OsType
    vm.os_type = detect_os_from_vbox_ostype(&os_type_str);

    // UEFI
    vm.uefi = has_efi;

    // Fallback name
    if vm.name.is_empty() {
        vm.name = config_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("imported-vm")
            .to_string();
    }

    // Fallback vCPUs (VBox defaults to 1 if not specified)
    if vm.vcpus == 0 {
        vm.vcpus = 1;
    }

    // VirtualBox-specific warnings
    if vm.disk_format == "vdi" {
        vm.notes.push(
            "VDI disks can be used directly by QEMU, or convert to qcow2 for snapshot support"
                .to_string(),
        );
    }

    // NIC model — VBox typically uses Intel PRO/1000
    vm.nic_model = "e1000".to_string();
    vm.notes
        .push("VirtualBox NIC model (Intel PRO/1000) mapped to e1000".to_string());

    // GPU model — VBox uses VBoxSVGA/VMSVGA, not available in QEMU
    vm.warnings
        .push("VirtualBox SVGA display not supported, using QXL instead".to_string());
    vm.gpu_model = "qxl".to_string();

    // Check disk existence
    for (i, path) in vm.disk_paths.iter().enumerate() {
        if !path.exists() {
            vm.warnings
                .push(format!("Disk {} not found: {}", i + 1, path.display()));
        }
    }

    vm.notes.push(format!(
        "Imported from VirtualBox: {}",
        config_path.display()
    ));

    Ok(vm)
}

/// Map VirtualBox OSType string to OsType.
pub(super) fn detect_os_from_vbox_ostype(os_type: &str) -> OsType {
    let t = os_type.to_lowercase();
    if t.contains("windows") || t.starts_with("win") {
        OsType::Windows
    } else if t.contains("macos")
        || t.contains("macosx")
        || t.contains("darwin")
        || t.starts_with("mac")
    {
        OsType::MacOS
    } else if t.contains("freebsd") {
        OsType::FreeBSD
    } else if t.contains("linux")
        || t.contains("ubuntu")
        || t.contains("debian")
        || t.contains("fedora")
        || t.contains("redhat")
        || t.contains("arch")
        || t.contains("gentoo")
        || t.contains("suse")
    {
        OsType::Linux
    } else if t.is_empty() {
        OsType::Linux
    } else {
        OsType::Other
    }
}
