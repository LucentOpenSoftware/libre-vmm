//! Libvirt XML import parser.

use crate::config::{DisplayProtocol, NetworkMode, OsType};
use std::path::{Path, PathBuf};

use super::ImportSource;
use super::ImportedVm;

/// Parse a libvirt domain XML file using quick-xml.
pub(super) fn parse_libvirt_xml(xml: &str, config_path: &Path) -> Result<ImportedVm, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut vm = ImportedVm::new(ImportSource::LibvirtXml);

    let mut reader = Reader::from_str(xml);

    // State tracking
    let mut element_stack: Vec<String> = Vec::new();
    let mut capture_text_for: Option<String> = None;
    let mut memory_unit = String::from("KiB");

    // Disk state
    let mut in_disk = false;
    let mut current_disk_source = PathBuf::new();

    // Interface state
    let mut in_interface = false;
    let mut first_interface_done = false;
    let mut current_net_type = String::new();
    let mut current_net_model = String::new();

    // Feature detection
    let mut has_hyperv = false;
    let mut has_applesmc = false;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                let parent = element_stack.last().map(|s| s.as_str()).unwrap_or("");

                match tag.as_str() {
                    "name" if parent == "domain" => {
                        capture_text_for = Some("name".to_string());
                    },
                    "memory" | "currentMemory" => {
                        if vm.memory_mib == 2048 {
                            // not yet set
                            memory_unit = "KiB".to_string();
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"unit" {
                                    memory_unit = xml_attr_value(&attr);
                                }
                            }
                            capture_text_for = Some("memory".to_string());
                        }
                    },
                    "vcpu" => {
                        capture_text_for = Some("vcpu".to_string());
                    },
                    "loader" => {
                        vm.uefi = true;
                    },
                    "disk" => {
                        in_disk = true;
                        current_disk_source = PathBuf::new();
                    },
                    "interface" => {
                        in_interface = true;
                        current_net_type.clear();
                        current_net_model.clear();
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                current_net_type = xml_attr_value(&attr);
                            }
                        }
                    },
                    "model" if parent == "video" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                vm.gpu_model = map_libvirt_vga(&xml_attr_value(&attr));
                            }
                        }
                    },
                    "model" if in_interface => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                current_net_model = xml_attr_value(&attr);
                            }
                        }
                    },
                    "tpm" => {
                        vm.tpm = true;
                    },
                    "hyperv" => {
                        has_hyperv = true;
                    },
                    _ => {},
                }
                element_stack.push(tag);
            },
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                let parent = element_stack.last().map(|s| s.as_str()).unwrap_or("");

                match tag.as_str() {
                    "loader" => {
                        vm.uefi = true;
                    },
                    "source" if in_disk => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"file" {
                                current_disk_source = PathBuf::from(xml_attr_value(&attr));
                            }
                        }
                    },
                    "target" if in_disk => {
                        // We could extract bus type here if needed
                    },
                    "source" if in_interface => {
                        // bridge/network name — not strictly needed for NetworkMode mapping
                    },
                    "model" if parent == "video" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                vm.gpu_model = map_libvirt_vga(&xml_attr_value(&attr));
                            }
                        }
                    },
                    "model" if in_interface => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                current_net_model = xml_attr_value(&attr);
                            }
                        }
                    },
                    "graphics" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                vm.display_protocol =
                                    map_graphics_to_display(&xml_attr_value(&attr));
                            }
                        }
                    },
                    "tpm" => {
                        vm.tpm = true;
                    },
                    "qemu:arg" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"value" {
                                let val = xml_attr_value(&attr);
                                if val.contains("applesmc") {
                                    has_applesmc = true;
                                }
                            }
                        }
                    },
                    _ => {},
                }
            },
            Ok(Event::Text(ref t)) => {
                if let Some(ref target) = capture_text_for {
                    let text = String::from_utf8_lossy(t.as_ref()).trim().to_string();
                    match target.as_str() {
                        "name" => vm.name = text,
                        "memory" => {
                            if let Ok(val) = text.parse::<u64>() {
                                vm.memory_mib = convert_memory_to_mib(val, &memory_unit);
                            }
                        },
                        "vcpu" => {
                            if let Ok(val) = text.parse::<u32>() {
                                vm.vcpus = val;
                            }
                        },
                        _ => {},
                    }
                    capture_text_for = None;
                }
            },
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                if tag == "disk" && in_disk {
                    if !current_disk_source.as_os_str().is_empty() {
                        // Detect disk format from extension
                        if let Some(ext) = current_disk_source.extension().and_then(|e| e.to_str())
                        {
                            vm.disk_format = ext.to_string();
                        }
                        vm.disk_paths.push(current_disk_source.clone());
                    }
                    in_disk = false;
                }
                if tag == "interface" && in_interface {
                    if !first_interface_done {
                        vm.network_mode = map_libvirt_net_type(&current_net_type);
                        if !current_net_model.is_empty() {
                            vm.nic_model = current_net_model.clone();
                        }
                        first_interface_done = true;
                    }
                    in_interface = false;
                }

                if element_stack.last().map(|s| s.as_str()) == Some(&tag) {
                    element_stack.pop();
                }
                capture_text_for = None;
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("Error parsing libvirt XML: {}", e)),
            _ => {},
        }
        buf.clear();
    }

    // OS type detection from features
    if has_hyperv {
        vm.os_type = OsType::Windows;
        vm.notes
            .push("Detected Windows guest (Hyper-V enlightenments present)".to_string());
    } else if has_applesmc {
        vm.os_type = OsType::MacOS;
        vm.notes
            .push("Detected macOS guest (AppleSMC device present)".to_string());
    }

    // Fallback name from filename
    if vm.name.is_empty() {
        vm.name = config_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("imported-vm")
            .to_string();
    }

    // Check disk existence
    for (i, path) in vm.disk_paths.iter().enumerate() {
        if !path.exists() {
            vm.warnings
                .push(format!("Disk {} not found: {}", i + 1, path.display()));
        }
    }

    vm.notes.push(format!(
        "Imported from Libvirt XML: {}",
        config_path.display()
    ));

    Ok(vm)
}

/// Helper: extract attribute value from quick-xml attribute.
pub(super) fn xml_attr_value(attr: &quick_xml::events::attributes::Attribute) -> String {
    String::from_utf8_lossy(&attr.value).to_string()
}

/// Convert memory value from a given unit to MiB.
pub(super) fn convert_memory_to_mib(val: u64, unit: &str) -> u64 {
    match unit {
        "b" | "bytes" => val / (1024 * 1024),
        "KB" => (val * 1000) / (1024 * 1024),
        "KiB" | "k" => val / 1024,
        "MB" => (val * 1000 * 1000) / (1024 * 1024),
        "MiB" | "M" => val,
        "GB" => (val * 1000 * 1000 * 1000) / (1024 * 1024),
        "GiB" | "G" => val * 1024,
        _ => val / 1024, // default KiB
    }
}

/// Map libvirt VGA model name to a string used by our config.
fn map_libvirt_vga(model: &str) -> String {
    match model {
        "vga" | "" => "vga".to_string(),
        "cirrus" => "vga".to_string(),
        "vmvga" => "vmware".to_string(),
        "qxl" => "qxl".to_string(),
        "virtio" => "virtio".to_string(),
        "bochs" => "vga".to_string(),
        "none" => "none".to_string(),
        other => other.to_string(),
    }
}

/// Map libvirt graphics type to DisplayProtocol.
fn map_graphics_to_display(graphics: &str) -> DisplayProtocol {
    match graphics {
        "spice" => DisplayProtocol::Spice,
        "vnc" | "" => DisplayProtocol::Vnc,
        _ => DisplayProtocol::Vnc,
    }
}

/// Map libvirt network interface type to NetworkMode.
fn map_libvirt_net_type(net_type: &str) -> NetworkMode {
    match net_type {
        "bridge" => NetworkMode::Bridged,
        "user" => NetworkMode::Nat,
        "network" => NetworkMode::Nat, // libvirt virtual network -> NAT
        "direct" => NetworkMode::Bridged,
        _ => NetworkMode::Nat,
    }
}
