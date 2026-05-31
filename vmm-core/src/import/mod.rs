//! VM Import Module
//!
//! Parses and imports VMs from multiple configuration formats:
//! - Libvirt XML (.xml)
//! - VMware Workstation (.vmx)
//! - VirtualBox (.vbox)
//! - Quickemu (.conf)
//!
//! Converts foreign VM definitions into libre-vmm's VmConfig for seamless import.

mod libvirt;
mod quickemu;
pub mod virtualbox;
pub mod vmware;

use crate::config::{
    BootDevice, DisplayProtocol, GpuModel, NetworkMode, NicConfig, OsType, VmConfig,
};
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

// Re-export submodule parse functions for testing
use libvirt::parse_libvirt_xml;
use quickemu::parse_quickemu_conf;
use virtualbox::parse_virtualbox_vbox;
use vmware::parse_vmware_vmx;

// =========================================================================
// Data Structures
// =========================================================================

/// Source format of an imported VM.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportSource {
    LibvirtXml,
    VmwareVmx,
    VirtualBoxVbox,
    QuickemuConf,
}

impl std::fmt::Display for ImportSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportSource::LibvirtXml => write!(f, "Libvirt XML"),
            ImportSource::VmwareVmx => write!(f, "VMware VMX"),
            ImportSource::VirtualBoxVbox => write!(f, "VirtualBox"),
            ImportSource::QuickemuConf => write!(f, "Quickemu"),
        }
    }
}

/// Result of parsing an import file.
#[derive(Debug, Clone)]
pub struct ImportedVm {
    pub source: ImportSource,
    pub name: String,
    pub os_type: OsType,
    pub vcpus: u32,
    pub memory_mib: u64,
    pub disk_paths: Vec<PathBuf>,
    pub disk_format: String,
    pub iso_path: Option<PathBuf>,
    pub network_mode: NetworkMode,
    pub nic_model: String,
    pub display_protocol: DisplayProtocol,
    pub uefi: bool,
    pub tpm: bool,
    pub gpu_model: String,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
}

impl ImportedVm {
    pub(crate) fn new(source: ImportSource) -> Self {
        Self {
            source,
            name: String::new(),
            os_type: OsType::Linux,
            vcpus: 2,
            memory_mib: 2048,
            disk_paths: Vec::new(),
            disk_format: "qcow2".to_string(),
            iso_path: None,
            network_mode: NetworkMode::Nat,
            nic_model: "virtio".to_string(),
            display_protocol: DisplayProtocol::default(),
            uefi: false,
            tpm: false,
            gpu_model: "qxl".to_string(),
            warnings: Vec::new(),
            notes: Vec::new(),
        }
    }
}

/// How to handle disk images during import.
#[derive(Debug, Clone, PartialEq)]
pub enum DiskAction {
    /// Symlink to original disk (no copy, saves space).
    Symlink,
    /// Copy disk to libre-vmm storage.
    Copy,
    /// Move disk to libre-vmm storage.
    Move,
    /// Convert disk format (e.g., vmdk -> qcow2).
    Convert,
}

// =========================================================================
// Format Detection
// =========================================================================

/// Detect import format from file extension.
pub fn detect_format(path: &Path) -> Option<ImportSource> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("xml") => Some(ImportSource::LibvirtXml),
        Some("vmx") => Some(ImportSource::VmwareVmx),
        Some("vbox") => Some(ImportSource::VirtualBoxVbox),
        Some("conf") => Some(ImportSource::QuickemuConf),
        _ => None,
    }
}

// =========================================================================
// Main Parse Entry Point
// =========================================================================

/// Parse an import file into an ImportedVm, auto-detecting the format.
pub fn parse_import(path: &Path) -> Result<ImportedVm, String> {
    let source = detect_format(path).ok_or_else(|| {
        format!(
            "Unsupported file format: {}. Supported: .xml, .vmx, .vbox, .conf",
            path.display()
        )
    })?;

    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

    match source {
        ImportSource::LibvirtXml => parse_libvirt_xml(&content, path),
        ImportSource::VmwareVmx => parse_vmware_vmx(&content, path),
        ImportSource::VirtualBoxVbox => parse_virtualbox_vbox(&content, path),
        ImportSource::QuickemuConf => parse_quickemu_conf(&content, path),
    }
}

// =========================================================================
// Conversion to VmConfig
// =========================================================================

/// Convert an ImportedVm into a VmConfig ready for creation.
pub fn to_vm_config(imported: &ImportedVm) -> VmConfig {
    let disk_path = imported
        .disk_paths
        .first()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let iso_path = imported
        .iso_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());

    // Map NIC model to NicConfig
    let network_interfaces = vec![NicConfig {
        mode: imported.network_mode.clone(),
        model: imported.nic_model.clone(),
        mac: String::new(),
    }];

    // Map GPU model string to GpuModel enum
    let gpu_model = match imported.gpu_model.as_str() {
        "qxl" => GpuModel::Qxl,
        "virtio" | "virtio-gpu" => GpuModel::VirtioGpu,
        "vga" | "std" => GpuModel::Vga,
        "vmware" | "vmvga" => GpuModel::VmwareSvga,
        "none" => GpuModel::None,
        _ => GpuModel::Auto,
    };

    VmConfig {
        id: Uuid::new_v4(),
        name: imported.name.clone(),
        vcpus: imported.vcpus,
        memory_mib: imported.memory_mib,
        disk_size_gib: 0, // unknown from import — will be detected from disk file
        disk_path,
        iso_path,
        os_type: imported.os_type,
        uefi: imported.uefi,
        gpu_accel: false,
        network: imported.network_mode.clone(),
        display_protocol: imported.display_protocol,
        usb_support: true,
        audio: true,
        shared_folder: None,
        description: format!(
            "Imported from {} ({})",
            imported.source,
            imported.warnings.len() + imported.notes.len()
        ),
        boot_order: vec![BootDevice::Hd, BootDevice::Cdrom],
        network_interfaces,
        autostart: false,
        tags: vec!["imported".to_string()],
        folder: None,
        favorite: false,
        tpm_enabled: imported.tpm,
        notes: {
            let mut all = Vec::new();
            if !imported.warnings.is_empty() {
                all.push("## Import Warnings".to_string());
                for w in &imported.warnings {
                    all.push(format!("- {}", w));
                }
            }
            if !imported.notes.is_empty() {
                all.push("## Import Notes".to_string());
                for n in &imported.notes {
                    all.push(format!("- {}", n));
                }
            }
            all.join("\n")
        },
        gpu_model,
        ..VmConfig::default()
    }
}

// =========================================================================
// Import Execution
// =========================================================================

/// Execute the import: handle disk, create config, return VmConfig.
///
/// This copies/symlinks/moves the disk image into libre-vmm's storage directory
/// and returns a ready-to-use VmConfig.
pub fn execute_import(
    imported: &ImportedVm,
    disk_action: DiskAction,
    vm_name: &str,
) -> Result<VmConfig, String> {
    let storage_dir = get_storage_dir()?;
    let vm_disk_dir = storage_dir.join("disks");
    fs::create_dir_all(&vm_disk_dir)
        .map_err(|e| format!("Failed to create disk directory: {}", e))?;

    let mut config = to_vm_config(imported);
    config.name = vm_name.to_string();

    // Handle each disk
    for (i, src_path) in imported.disk_paths.iter().enumerate() {
        if !src_path.exists() {
            continue;
        }

        let target_ext = match disk_action {
            DiskAction::Convert => "qcow2",
            _ => imported.disk_format.as_str(),
        };

        let disk_filename = if i == 0 {
            format!("{}.{}", sanitize_name(vm_name), target_ext)
        } else {
            format!("{}-disk{}.{}", sanitize_name(vm_name), i + 1, target_ext)
        };

        let dest = vm_disk_dir.join(&disk_filename);

        match disk_action {
            DiskAction::Symlink => {
                let abs_source = fs::canonicalize(src_path)
                    .map_err(|e| format!("Failed to resolve path {}: {}", src_path.display(), e))?;
                unix_fs::symlink(&abs_source, &dest).map_err(|e| {
                    format!(
                        "Failed to create symlink {} -> {}: {}",
                        dest.display(),
                        abs_source.display(),
                        e
                    )
                })?;
            },
            DiskAction::Copy => {
                fs::copy(src_path, &dest).map_err(|e| {
                    format!(
                        "Failed to copy {} -> {}: {}",
                        src_path.display(),
                        dest.display(),
                        e
                    )
                })?;
            },
            DiskAction::Move => {
                if fs::rename(src_path, &dest).is_err() {
                    // Cross-device: copy + remove
                    fs::copy(src_path, &dest).map_err(|e| {
                        format!(
                            "Failed to copy {} -> {}: {}",
                            src_path.display(),
                            dest.display(),
                            e
                        )
                    })?;
                    fs::remove_file(src_path).map_err(|e| {
                        format!(
                            "Failed to remove original disk {}: {}",
                            src_path.display(),
                            e
                        )
                    })?;
                }
            },
            DiskAction::Convert => {
                // Shell out to qemu-img convert
                let status = std::process::Command::new("qemu-img")
                    .args([
                        "convert",
                        "-f",
                        &imported.disk_format,
                        "-O",
                        "qcow2",
                        &src_path.to_string_lossy(),
                        &dest.to_string_lossy(),
                    ])
                    .status()
                    .map_err(|e| format!("Failed to run qemu-img: {}", e))?;

                if !status.success() {
                    return Err(format!(
                        "qemu-img convert failed for {}",
                        src_path.display()
                    ));
                }
            },
        }

        // Update disk path in config to point to new location
        if i == 0 {
            config.disk_path = dest.to_string_lossy().to_string();
        }
    }

    // Save config to configs directory
    let config_dir = storage_dir.join("configs");
    fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Failed to create config directory: {}", e))?;

    let config_path = config_dir.join(format!("{}.json", sanitize_name(vm_name)));
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    fs::write(&config_path, json)
        .map_err(|e| format!("Failed to write config to {}: {}", config_path.display(), e))?;

    Ok(config)
}

// =========================================================================
// Discovery
// =========================================================================

/// Maximum recursion depth when scanning unknown directories for VM configs.
///
/// Keeps runaway scans bounded even when the user points us at a deeply nested tree.
pub const MAX_SCAN_DEPTH: usize = 3;

/// Discover importable VMs from well-known system directories.
///
/// Scans libvirt XML, VirtualBox, VMware, Quickemu, and legacy libvirt image
/// locations. Each directory tree is walked up to [`MAX_SCAN_DEPTH`] levels deep.
pub fn discover_importable_vms() -> Vec<ImportedVm> {
    let mut vms = Vec::new();

    // ---- Libvirt XML locations ----
    let mut libvirt_dirs: Vec<PathBuf> = vec![
        PathBuf::from("/etc/libvirt/qemu"),
        dirs::config_dir()
            .map(|d| d.join("libvirt/qemu"))
            .unwrap_or_default(),
    ];
    // Legacy / system locations that occasionally have stray VM XML.
    libvirt_dirs.push(PathBuf::from("/var/lib/libvirt/qemu"));
    for dir in &libvirt_dirs {
        if dir.as_os_str().is_empty() || !dir.exists() {
            continue;
        }
        scan_dir_for_extension_deep(dir, "xml", MAX_SCAN_DEPTH, &mut vms);
    }

    // Legacy libvirt images dir (catch raw qcow2 that have matching .xml siblings)
    let legacy_images = PathBuf::from("/var/lib/libvirt/images");
    if legacy_images.exists() {
        scan_dir_for_extension_deep(&legacy_images, "xml", MAX_SCAN_DEPTH, &mut vms);
    }

    if let Some(home) = dirs::home_dir() {
        // ---- VirtualBox ----
        for d in virtualbox::default_search_roots(&home) {
            if d.exists() {
                scan_dir_for_extension_deep(&d, "vbox", MAX_SCAN_DEPTH, &mut vms);
            }
        }

        // ---- VMware ----
        for d in vmware::default_search_roots(&home) {
            if d.exists() {
                scan_dir_for_extension_deep(&d, "vmx", MAX_SCAN_DEPTH, &mut vms);
            }
        }

        // ---- Quickemu ----
        for d in [
            home.join(".local/share/quickemu"),
            home.join("snap/quickemu/common"),
            home.join("quickemu"),
        ] {
            if d.exists() {
                scan_dir_for_extension_deep(&d, "conf", MAX_SCAN_DEPTH, &mut vms);
            }
        }

        // ---- macOS-style paths sometimes present on Linux ----
        let mac_style = home.join("Library/Application Support/VMware Fusion");
        if mac_style.exists() {
            scan_dir_for_extension_deep(&mac_style, "vmx", MAX_SCAN_DEPTH, &mut vms);
        }
    }

    dedupe_by_disk_or_name(&mut vms);
    vms
}

/// Discover VMs and group by source format for UI display.
///
/// Returns up to `max_results` VMs spread across all sources, preserving the
/// natural source ordering: Libvirt → VMware → VirtualBox → Quickemu.
pub fn discover_and_group(max_results: usize) -> Vec<(ImportSource, Vec<ImportedVm>)> {
    let mut all = discover_importable_vms();
    if max_results > 0 && all.len() > max_results {
        all.truncate(max_results);
    }

    let order = [
        ImportSource::LibvirtXml,
        ImportSource::VmwareVmx,
        ImportSource::VirtualBoxVbox,
        ImportSource::QuickemuConf,
    ];

    let mut groups: Vec<(ImportSource, Vec<ImportedVm>)> = Vec::new();
    for src in &order {
        let bucket: Vec<ImportedVm> = all.iter().filter(|v| &v.source == src).cloned().collect();
        if !bucket.is_empty() {
            groups.push((src.clone(), bucket));
        }
    }
    groups
}

/// Drop discovered entries that share the same first disk path (the same VM
/// referenced both from a libvirt XML and the VirtualBox / VMware library).
fn dedupe_by_disk_or_name(vms: &mut Vec<ImportedVm>) {
    let mut seen_disk: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut seen_name: std::collections::HashSet<String> = std::collections::HashSet::new();
    vms.retain(|vm| {
        // Prefer disk-based dedup when available (most accurate).
        if let Some(disk) = vm.disk_paths.first() {
            let canon = fs::canonicalize(disk).unwrap_or_else(|_| disk.clone());
            if !seen_disk.insert(canon) {
                return false;
            }
        }
        let key = format!("{}::{}", vm.source, vm.name.to_lowercase());
        seen_name.insert(key)
    });
}

// =========================================================================
// Helpers
// =========================================================================

/// Recursively scan a directory for files with a given extension (one level deep).
///
/// Kept as a thin wrapper for backwards compatibility with the old single-level
/// behaviour used in tests.
fn scan_dir_for_extension(dir: &Path, ext: &str, vms: &mut Vec<ImportedVm>) {
    scan_dir_for_extension_deep(dir, ext, 2, vms);
}

/// Recursively scan a directory for files with the given extension, bounded by `max_depth`.
///
/// `max_depth = 0` only inspects `dir` itself for files (no recursion).
/// `max_depth = 1` recurses into immediate subdirectories.
pub(crate) fn scan_dir_for_extension_deep(
    dir: &Path,
    ext: &str,
    max_depth: usize,
    vms: &mut Vec<ImportedVm>,
) {
    walk_for_extension(dir, ext, max_depth, &mut |path| {
        if let Ok(vm) = parse_import(path) {
            vms.push(vm);
        }
    });
}

/// Walk `dir` up to `max_depth` levels and invoke `visit` for every file matching `ext`.
pub(crate) fn walk_for_extension<F: FnMut(&Path)>(
    dir: &Path,
    ext: &str,
    max_depth: usize,
    visit: &mut F,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if path.extension().and_then(|e| e.to_str()) == Some(ext) {
                visit(&path);
            }
        } else if path.is_dir() && max_depth > 0 {
            walk_for_extension(&path, ext, max_depth - 1, visit);
        }
    }
}

/// Get the libre-vmm storage directory.
fn get_storage_dir() -> Result<PathBuf, String> {
    let dir = dirs::data_local_dir()
        .ok_or("Could not determine local data directory")?
        .join("libre-vmm");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create storage directory: {}", e))?;
    Ok(dir)
}

/// Sanitize a VM name for use as a filename.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::libvirt::convert_memory_to_mib;
    use super::quickemu::parse_ram_string;
    use super::virtualbox::detect_os_from_vbox_ostype;
    use super::vmware::detect_os_from_vmware_guest;
    use super::*;

    #[test]
    fn test_detect_format() {
        assert_eq!(
            detect_format(Path::new("test.xml")),
            Some(ImportSource::LibvirtXml)
        );
        assert_eq!(
            detect_format(Path::new("test.vmx")),
            Some(ImportSource::VmwareVmx)
        );
        assert_eq!(
            detect_format(Path::new("test.vbox")),
            Some(ImportSource::VirtualBoxVbox)
        );
        assert_eq!(
            detect_format(Path::new("test.conf")),
            Some(ImportSource::QuickemuConf)
        );
        assert_eq!(detect_format(Path::new("test.txt")), None);
    }

    #[test]
    fn test_parse_vmware_vmx() {
        let vmx = r#"
.encoding = "UTF-8"
displayName = "Windows 11 Pro"
memsize = "8192"
numvcpus = "4"
guestOS = "windows11-64"
firmware = "efi"
scsi0:0.fileName = "disk.vmdk"
ethernet0.connectionType = "nat"
ethernet0.virtualDev = "e1000e"
"#;
        let result = parse_vmware_vmx(vmx, Path::new("/tmp/test.vmx")).unwrap();
        assert_eq!(result.name, "Windows 11 Pro");
        assert_eq!(result.memory_mib, 8192);
        assert_eq!(result.vcpus, 4);
        assert_eq!(result.os_type, OsType::Windows);
        assert!(result.uefi);
        assert_eq!(result.network_mode, NetworkMode::Nat);
    }

    #[test]
    fn test_parse_libvirt_xml() {
        let xml = r#"
<domain type='kvm'>
  <name>ubuntu-desktop</name>
  <memory unit='MiB'>4096</memory>
  <vcpu>2</vcpu>
  <os>
    <type arch='x86_64' machine='pc-q35-8.2'>hvm</type>
    <loader readonly='yes' type='pflash'>/usr/share/OVMF/OVMF_CODE.fd</loader>
  </os>
  <features>
    <acpi/>
  </features>
  <devices>
    <disk type='file' device='disk'>
      <source file='/var/lib/libvirt/images/ubuntu.qcow2'/>
      <target dev='vda' bus='virtio'/>
    </disk>
    <interface type='network'>
      <model type='virtio'/>
    </interface>
    <graphics type='spice'/>
    <video>
      <model type='qxl'/>
    </video>
  </devices>
</domain>"#;
        let result = parse_libvirt_xml(xml, Path::new("/tmp/test.xml")).unwrap();
        assert_eq!(result.name, "ubuntu-desktop");
        assert_eq!(result.memory_mib, 4096);
        assert_eq!(result.vcpus, 2);
        assert!(result.uefi);
        assert_eq!(result.display_protocol, DisplayProtocol::Spice);
        assert_eq!(result.disk_paths.len(), 1);
    }

    #[test]
    fn test_parse_vbox_xml() {
        let xml = r#"
<?xml version="1.0"?>
<VirtualBox>
  <Machine name="Fedora 39" OSType="Fedora_64">
    <Hardware>
      <CPU count="4"/>
      <Memory RAMSize="4096"/>
      <EFI enabled="true"/>
      <Network>
        <Adapter slot="0" enabled="true" type="82540EM">
          <NAT/>
        </Adapter>
      </Network>
    </Hardware>
    <MediaRegistry>
      <HardDisks>
        <HardDisk location="Fedora 39.vdi" format="VDI"/>
      </HardDisks>
    </MediaRegistry>
  </Machine>
</VirtualBox>"#;
        let result = parse_virtualbox_vbox(
            xml,
            Path::new("/home/user/VirtualBox VMs/Fedora 39/Fedora 39.vbox"),
        )
        .unwrap();
        assert_eq!(result.name, "Fedora 39");
        assert_eq!(result.memory_mib, 4096);
        assert_eq!(result.vcpus, 4);
        assert!(result.uefi);
        assert_eq!(result.os_type, OsType::Linux);
        assert_eq!(result.network_mode, NetworkMode::Nat);
    }

    #[test]
    fn test_convert_memory_to_mib() {
        assert_eq!(convert_memory_to_mib(4096, "MiB"), 4096);
        assert_eq!(convert_memory_to_mib(4194304, "KiB"), 4096);
        assert_eq!(convert_memory_to_mib(2, "GiB"), 2048);
        assert_eq!(convert_memory_to_mib(4294967296, "b"), 4096);
    }

    #[test]
    fn test_parse_ram_string() {
        assert_eq!(parse_ram_string("4G"), 4096);
        assert_eq!(parse_ram_string("2048M"), 2048);
        assert_eq!(parse_ram_string("2048"), 2048);
        assert_eq!(parse_ram_string(""), 2048);
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("My VM (test)"), "My_VM__test_");
        assert_eq!(sanitize_name("simple-name"), "simple-name");
        assert_eq!(sanitize_name("with spaces"), "with_spaces");
    }

    #[test]
    fn test_os_detection_vmware() {
        assert_eq!(detect_os_from_vmware_guest("windows11-64"), OsType::Windows);
        assert_eq!(detect_os_from_vmware_guest("ubuntu-64"), OsType::Linux);
        assert_eq!(detect_os_from_vmware_guest("darwin20-64"), OsType::MacOS);
        assert_eq!(detect_os_from_vmware_guest("freebsd-64"), OsType::FreeBSD);
        assert_eq!(detect_os_from_vmware_guest(""), OsType::Linux);
    }

    #[test]
    fn test_os_detection_vbox() {
        assert_eq!(detect_os_from_vbox_ostype("Windows11_64"), OsType::Windows);
        assert_eq!(detect_os_from_vbox_ostype("Fedora_64"), OsType::Linux);
        assert_eq!(detect_os_from_vbox_ostype("MacOS_64"), OsType::MacOS);
        assert_eq!(detect_os_from_vbox_ostype("FreeBSD_64"), OsType::FreeBSD);
    }

    #[test]
    fn test_to_vm_config() {
        let mut imported = ImportedVm::new(ImportSource::VmwareVmx);
        imported.name = "Test VM".to_string();
        imported.vcpus = 4;
        imported.memory_mib = 8192;
        imported.os_type = OsType::Windows;
        imported.uefi = true;
        imported.tpm = true;

        let config = to_vm_config(&imported);
        assert_eq!(config.name, "Test VM");
        assert_eq!(config.vcpus, 4);
        assert_eq!(config.memory_mib, 8192);
        assert_eq!(config.os_type, OsType::Windows);
        assert!(config.uefi);
        assert!(config.tpm_enabled);
        assert!(config.tags.contains(&"imported".to_string()));
    }

    // ---- Wave 13.7 / 13.8 / 13.9 — discovery + library scan ----

    fn make_tmp_subdir(base: &Path, name: &str) -> PathBuf {
        let d = base.join(name);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_minimal_vmx(path: &Path, name: &str) {
        let content = format!(
            "displayName = \"{}\"\nmemsize = \"2048\"\nnumvcpus = \"2\"\nguestOS = \"ubuntu-64\"\n",
            name
        );
        fs::write(path, content).unwrap();
    }

    fn write_minimal_vbox(path: &Path, name: &str) {
        let content = format!(
            "<?xml version=\"1.0\"?>\n<VirtualBox>\n  <Machine name=\"{}\" OSType=\"Ubuntu_64\">\n    <Hardware>\n      <CPU count=\"2\"/>\n      <Memory RAMSize=\"2048\"/>\n    </Hardware>\n  </Machine>\n</VirtualBox>\n",
            name
        );
        fs::write(path, content).unwrap();
    }

    #[test]
    fn scan_vmware_library_recurses_to_depth() {
        let tmp = std::env::temp_dir().join(format!("lv-vmware-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let lvl1 = make_tmp_subdir(&tmp, "MyVM");
        let lvl2 = make_tmp_subdir(&lvl1, "Snapshots");
        write_minimal_vmx(&lvl1.join("MyVM.vmx"), "MyVM");
        write_minimal_vmx(&lvl2.join("inner.vmx"), "Inner");

        let found = super::vmware::scan_vmware_library(&tmp);
        assert!(
            found.len() >= 2,
            "expected at least 2 VMs, got {}",
            found.len()
        );
        let names: Vec<_> = found.iter().map(|v| v.name.clone()).collect();
        assert!(names.contains(&"MyVM".to_string()));
        assert!(names.contains(&"Inner".to_string()));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_vmware_library_empty_when_root_missing() {
        let missing = std::env::temp_dir().join("lv-vmware-nonexistent-xyz");
        let _ = fs::remove_dir_all(&missing);
        let found = super::vmware::scan_vmware_library(&missing);
        assert!(found.is_empty());
    }

    #[test]
    fn scan_vbox_library_recurses() {
        let tmp = std::env::temp_dir().join(format!("lv-vbox-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let vm_dir = make_tmp_subdir(&tmp, "Fedora 39");
        write_minimal_vbox(&vm_dir.join("Fedora 39.vbox"), "Fedora 39");

        let found = super::virtualbox::scan_vbox_library(&tmp);
        assert!(!found.is_empty(), "expected at least one VBox VM");
        assert!(found.iter().any(|v| v.name == "Fedora 39"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn default_search_roots_returns_paths() {
        let home = PathBuf::from("/home/testuser");
        let vmw = super::vmware::default_search_roots(&home);
        assert!(vmw.iter().any(|p| p.ends_with("vmware")));
        assert!(vmw
            .iter()
            .any(|p| p.ends_with("Documents/Virtual Machines")));

        let vbox = super::virtualbox::default_search_roots(&home);
        assert!(vbox.iter().any(|p| p.ends_with("VirtualBox VMs")));
        assert!(vbox
            .iter()
            .any(|p| p.ends_with("snap/virtualbox/common/Machines")));
    }

    #[test]
    fn walk_for_extension_respects_depth_zero() {
        let tmp = std::env::temp_dir().join(format!("lv-walk-d0-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let sub = make_tmp_subdir(&tmp, "deep");
        write_minimal_vmx(&sub.join("a.vmx"), "A");
        write_minimal_vmx(&tmp.join("top.vmx"), "Top");

        let mut hits = 0;
        walk_for_extension(&tmp, "vmx", 0, &mut |_p| hits += 1);
        assert_eq!(hits, 1, "depth 0 should only find files in root, not sub/");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn walk_for_extension_max_depth_three() {
        let tmp = std::env::temp_dir().join(format!("lv-walk-d3-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let l1 = make_tmp_subdir(&tmp, "l1");
        let l2 = make_tmp_subdir(&l1, "l2");
        let l3 = make_tmp_subdir(&l2, "l3");
        let l4 = make_tmp_subdir(&l3, "l4");
        write_minimal_vmx(&l3.join("ok.vmx"), "ok");
        write_minimal_vmx(&l4.join("too-deep.vmx"), "too-deep");

        let mut hits: Vec<String> = Vec::new();
        walk_for_extension(&tmp, "vmx", 3, &mut |p| {
            hits.push(p.file_name().unwrap().to_string_lossy().to_string());
        });
        assert!(hits.contains(&"ok.vmx".to_string()));
        assert!(
            !hits.contains(&"too-deep.vmx".to_string()),
            "depth 3 should not reach l4"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_and_group_returns_sorted_buckets() {
        // We can't easily inject fake home, but we can at least confirm the
        // function shape and that it doesn't panic with a low cap.
        let groups = discover_and_group(10);
        // Order property: each known source appears at most once, in sorted order.
        let mut last_idx: i32 = -1;
        let order = [
            ImportSource::LibvirtXml,
            ImportSource::VmwareVmx,
            ImportSource::VirtualBoxVbox,
            ImportSource::QuickemuConf,
        ];
        for (src, vms) in &groups {
            let idx = order.iter().position(|s| s == src).expect("known source") as i32;
            assert!(idx > last_idx, "groups must be in canonical order");
            last_idx = idx;
            assert!(!vms.is_empty());
        }
    }

    #[test]
    fn detect_vmware_library_returns_some_or_none_without_panic() {
        // Smoke test — never panics regardless of host state.
        let _ = super::vmware::detect_vmware_library();
        let _ = super::virtualbox::detect_vbox_library();
    }

    #[test]
    fn dedupe_by_disk_drops_duplicates() {
        let mut vms = vec![
            {
                let mut v = ImportedVm::new(ImportSource::LibvirtXml);
                v.name = "shared".into();
                v.disk_paths
                    .push(PathBuf::from("/tmp/shared-disk-XYZ.qcow2"));
                v
            },
            {
                let mut v = ImportedVm::new(ImportSource::VirtualBoxVbox);
                v.name = "shared".into();
                v.disk_paths
                    .push(PathBuf::from("/tmp/shared-disk-XYZ.qcow2"));
                v
            },
            {
                let mut v = ImportedVm::new(ImportSource::VmwareVmx);
                v.name = "different".into();
                v.disk_paths.push(PathBuf::from("/tmp/other-XYZ.qcow2"));
                v
            },
        ];
        dedupe_by_disk_or_name(&mut vms);
        assert_eq!(vms.len(), 2);
    }
}
