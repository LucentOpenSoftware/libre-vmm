//! PCI device enumeration and IOMMU group scanning via sysfs.
//!
//! Reads `/sys/bus/pci/devices/` to discover all PCI devices on the host,
//! including vendor/device IDs, bound drivers, IOMMU groups, and NUMA topology.
//! Used by the VFIO passthrough module to identify devices eligible for
//! passthrough to virtual machines.

use std::fs;
use std::path::{Path, PathBuf};

const SYSFS_PCI_DEVICES: &str = "/sys/bus/pci/devices";
const SYSFS_IOMMU_GROUPS: &str = "/sys/kernel/iommu_groups";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A PCI device discovered on the host.
#[derive(Debug, Clone)]
pub struct PciDevice {
    /// PCI address in domain:bus:slot.function format, e.g. "0000:01:00.0".
    pub address: String,
    /// Vendor ID (hex, no 0x prefix), e.g. "10de".
    pub vendor_id: String,
    /// Device ID (hex, no 0x prefix), e.g. "2684".
    pub device_id: String,
    /// Human-readable vendor name (best-effort from built-in table).
    pub vendor_name: String,
    /// Human-readable device name (best-effort from built-in table).
    pub device_name: String,
    /// High-level device class.
    pub class: PciClass,
    /// Raw class code (first 4 hex digits of the 6-digit sysfs value), e.g. "0300".
    pub class_code: String,
    /// Kernel driver currently bound to this device, if any.
    pub driver: Option<String>,
    /// IOMMU group number, if the kernel has assigned one.
    pub iommu_group: Option<u32>,
    /// NUMA node (-1 means no affinity).
    pub numa_node: Option<i32>,
    /// Subsystem string from sysfs (subsystem_vendor:subsystem_device).
    pub subsystem: Option<String>,
}

/// Broad PCI device class derived from the class code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PciClass {
    /// Display controller (class 0300 VGA, 0302 3D).
    Gpu,
    /// Audio device (class 0403 HD Audio).
    Audio,
    /// Ethernet controller (class 0200).
    Network,
    /// NVMe controller (class 0108).
    Nvme,
    /// USB controller (class 0c03).
    UsbController,
    /// Mass storage (SCSI 0100, IDE 0101, RAID 0104, SATA/AHCI 0106).
    Storage,
    /// PCI bridge (class 0604).
    Bridge,
    /// Anything else.
    Other,
}

/// An IOMMU group containing one or more PCI devices.
#[derive(Debug, Clone)]
pub struct IommuGroup {
    pub id: u32,
    pub devices: Vec<PciDevice>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Scan all PCI devices from `/sys/bus/pci/devices/`.
pub fn scan_pci_devices() -> Vec<PciDevice> {
    let pci_dir = Path::new(SYSFS_PCI_DEVICES);
    let entries = match fs::read_dir(pci_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut devices: Vec<PciDevice> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let address = entry.file_name().to_string_lossy().to_string();
            read_pci_device(&address)
        })
        .collect();

    devices.sort_by(|a, b| a.address.cmp(&b.address));
    devices
}

/// Get all IOMMU groups with their member devices.
pub fn scan_iommu_groups() -> Vec<IommuGroup> {
    let groups_dir = Path::new(SYSFS_IOMMU_GROUPS);
    let entries = match fs::read_dir(groups_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut groups: Vec<IommuGroup> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let id: u32 = entry.file_name().to_string_lossy().parse().ok()?;
            let devices_dir = entry.path().join("devices");
            let devs = match fs::read_dir(&devices_dir) {
                Ok(d) => d,
                Err(_) => {
                    return Some(IommuGroup {
                        id,
                        devices: Vec::new(),
                    })
                },
            };
            let devices: Vec<PciDevice> = devs
                .filter_map(|d| {
                    let d = d.ok()?;
                    let addr = d.file_name().to_string_lossy().to_string();
                    read_pci_device(&addr)
                })
                .collect();
            Some(IommuGroup { id, devices })
        })
        .collect();

    groups.sort_by_key(|g| g.id);
    groups
}

/// Find all GPUs (display controllers) on the system.
pub fn find_gpus() -> Vec<PciDevice> {
    scan_pci_devices()
        .into_iter()
        .filter(|d| d.class == PciClass::Gpu)
        .collect()
}

/// Find all devices in the same IOMMU group as the given PCI address.
pub fn get_iommu_group_members(address: &str) -> Vec<PciDevice> {
    let dev_path = PathBuf::from(SYSFS_PCI_DEVICES)
        .join(address)
        .join("iommu_group");
    let group_path = match fs::read_link(&dev_path) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let group_id: u32 = match group_path.file_name().and_then(|n| n.to_str()) {
        Some(name) => match name.parse() {
            Ok(id) => id,
            Err(_) => return Vec::new(),
        },
        None => return Vec::new(),
    };

    let devices_dir = PathBuf::from(SYSFS_IOMMU_GROUPS)
        .join(group_id.to_string())
        .join("devices");

    let entries = match fs::read_dir(&devices_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let addr = entry.file_name().to_string_lossy().to_string();
            read_pci_device(&addr)
        })
        .collect()
}

/// Check if IOMMU is enabled on the system (i.e. the kernel has created IOMMU groups).
pub fn is_iommu_enabled() -> bool {
    let groups_dir = Path::new(SYSFS_IOMMU_GROUPS);
    match fs::read_dir(groups_dir) {
        Ok(mut entries) => entries.next().is_some(),
        Err(_) => false,
    }
}

/// Check if a PCI device is currently bound to the `vfio-pci` driver.
pub fn is_bound_to_vfio(address: &str) -> bool {
    let driver_link = PathBuf::from(SYSFS_PCI_DEVICES)
        .join(address)
        .join("driver");
    match fs::read_link(&driver_link) {
        Ok(target) => target
            .file_name()
            .and_then(|n| n.to_str())
            .map(|name| name == "vfio-pci")
            .unwrap_or(false),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Read a single PCI device's attributes from sysfs.
fn read_pci_device(address: &str) -> Option<PciDevice> {
    let base = PathBuf::from(SYSFS_PCI_DEVICES).join(address);
    if !base.is_dir() {
        return None;
    }

    let vendor_id = read_sysfs_hex(&base.join("vendor"))?;
    let device_id = read_sysfs_hex(&base.join("device"))?;
    let class_raw = read_sysfs_hex(&base.join("class"))?;
    // class_raw is 6 hex digits (e.g. "030000"); take first 4 for the class code.
    let class_code = if class_raw.len() >= 4 {
        class_raw[..4].to_string()
    } else {
        class_raw.clone()
    };
    let class = classify_pci(&class_code);

    let driver = read_driver(&base);
    let iommu_group = read_iommu_group(&base);
    let numa_node = read_sysfs_i32(&base.join("numa_node"));
    let subsystem = read_subsystem(&base);

    let (vendor_name, device_name) = lookup_names(&vendor_id, &device_id);

    Some(PciDevice {
        address: address.to_string(),
        vendor_id,
        device_id,
        vendor_name,
        device_name,
        class,
        class_code,
        driver,
        iommu_group,
        numa_node,
        subsystem,
    })
}

/// Read a sysfs file containing a hex value like "0x10de\n" and return the
/// hex digits without the "0x" prefix.
fn read_sysfs_hex(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim().trim_start_matches("0x");
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_lowercase())
    }
}

fn read_sysfs_i32(path: &Path) -> Option<i32> {
    let raw = fs::read_to_string(path).ok()?;
    raw.trim().parse().ok()
}

/// Read the driver symlink basename.
fn read_driver(base: &Path) -> Option<String> {
    let link = fs::read_link(base.join("driver")).ok()?;
    link.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
}

/// Read the IOMMU group number from the iommu_group symlink.
fn read_iommu_group(base: &Path) -> Option<u32> {
    let link = fs::read_link(base.join("iommu_group")).ok()?;
    link.file_name()
        .and_then(|n| n.to_str())
        .and_then(|s| s.parse().ok())
}

/// Read subsystem vendor:device from sysfs.
fn read_subsystem(base: &Path) -> Option<String> {
    let sv = read_sysfs_hex(&base.join("subsystem_vendor"))?;
    let sd = read_sysfs_hex(&base.join("subsystem_device"))?;
    Some(format!("{}:{}", sv, sd))
}

/// Map the 4-digit PCI class code to our enum.
fn classify_pci(class_code: &str) -> PciClass {
    match class_code {
        "0300" | "0302" => PciClass::Gpu,
        "0403" => PciClass::Audio,
        "0200" => PciClass::Network,
        "0108" => PciClass::Nvme,
        "0c03" => PciClass::UsbController,
        "0100" | "0101" | "0104" | "0106" => PciClass::Storage,
        "0604" => PciClass::Bridge,
        _ => PciClass::Other,
    }
}

/// Look up vendor and device names from a built-in table of common hardware.
///
/// This covers the most frequently passthrough-ed devices (GPUs, audio
/// companions, NICs) so the UI can show something meaningful without
/// requiring an external pci.ids database.
fn lookup_names(vendor_id: &str, device_id: &str) -> (String, String) {
    let vendor_name = match vendor_id {
        "10de" => "NVIDIA Corporation",
        "1002" => "Advanced Micro Devices, Inc. [AMD/ATI]",
        "8086" => "Intel Corporation",
        "1022" => "Advanced Micro Devices, Inc. [AMD]",
        "14e4" => "Broadcom Inc.",
        "15b3" => "Mellanox Technologies",
        "144d" => "Samsung Electronics Co Ltd",
        "1987" => "Phison Electronics Corporation",
        "1b4b" => "Marvell Technology Group Ltd.",
        "1af4" => "Red Hat, Inc. (virtio)",
        "1234" => "QEMU",
        _ => "Unknown vendor",
    };

    // Common GPU device IDs — enough for the passthrough UI to show meaningful names.
    let device_name = match (vendor_id, device_id) {
        // NVIDIA Ada Lovelace
        ("10de", "2684") => "GeForce RTX 4090",
        ("10de", "2704") => "GeForce RTX 4080",
        ("10de", "2782") => "GeForce RTX 4070 Ti",
        ("10de", "2786") => "GeForce RTX 4070",
        ("10de", "2803") => "GeForce RTX 4060 Ti",
        ("10de", "2882") => "GeForce RTX 4060",
        // NVIDIA Blackwell
        ("10de", "2900") => "GeForce RTX 5090",
        ("10de", "2920") => "GeForce RTX 5080",
        ("10de", "2940") => "GeForce RTX 5070 Ti",
        ("10de", "2960") => "GeForce RTX 5070",
        // NVIDIA Ampere
        ("10de", "2204") => "GeForce RTX 3090",
        ("10de", "2206") => "GeForce RTX 3080",
        ("10de", "2484") => "GeForce RTX 3070",
        ("10de", "2504") => "GeForce RTX 3060 Ti",
        ("10de", "2544") => "GeForce RTX 3060",
        // NVIDIA Turing
        ("10de", "1e04") => "GeForce RTX 2080 Ti",
        ("10de", "1e82") => "GeForce RTX 2080",
        ("10de", "1f02") => "GeForce RTX 2070",
        ("10de", "1f82") => "GeForce RTX 2060",
        // NVIDIA HD Audio (IOMMU companion to GPUs)
        ("10de", "22ba") | ("10de", "228b") | ("10de", "1aef") => "NVIDIA HD Audio Controller",
        // AMD RDNA 3
        ("1002", "744c") => "Radeon RX 7900 XTX",
        ("1002", "7480") => "Radeon RX 7900 XT",
        ("1002", "7460") => "Radeon RX 7800 XT",
        ("1002", "7470") => "Radeon RX 7700 XT",
        // AMD RDNA 4
        ("1002", "7500") => "Radeon RX 9070 XT",
        ("1002", "7510") => "Radeon RX 9070",
        // AMD RDNA 2
        ("1002", "73bf") => "Radeon RX 6900 XT",
        ("1002", "73af") => "Radeon RX 6800 XT",
        ("1002", "73df") => "Radeon RX 6700 XT",
        ("1002", "73ff") => "Radeon RX 6600 XT",
        // AMD HD Audio companion
        ("1002", "ab28") | ("1002", "ab38") => "AMD HD Audio Controller",
        // Intel Arc
        ("8086", "56a0") => "Arc A770",
        ("8086", "56a1") => "Arc A750",
        ("8086", "5690") => "Arc A380",
        // Intel integrated
        ("8086", "9a49") => "UHD Graphics (11th Gen)",
        ("8086", "4680") => "UHD Graphics (12th Gen)",
        ("8086", "a780") => "UHD Graphics (13th/14th Gen)",
        // Samsung NVMe
        ("144d", "a808") => "NVMe SSD Controller PM981",
        ("144d", "a809") => "NVMe SSD Controller 980",
        ("144d", "a80a") => "NVMe SSD Controller 980 PRO",
        // Phison NVMe
        ("1987", "5016") => "PS5016-E16 PCIe 4.0 NVMe Controller",
        ("1987", "5018") => "PS5018-E18 PCIe 4.0 NVMe Controller",
        ("1987", "5019") => "PS5019-E19T PCIe 4.0 NVMe Controller",
        // virtio (QEMU guests)
        ("1af4", "1000") => "virtio-net",
        ("1af4", "1001") => "virtio-blk",
        ("1af4", "1003") => "virtio-console",
        ("1af4", "1005") => "virtio-scsi",
        ("1af4", "1009") => "virtio-fs",
        ("1af4", "1050") => "virtio-gpu",
        // QEMU
        ("1234", "1111") => "QEMU VGA",
        _ => "Unknown device",
    };

    (vendor_name.to_string(), device_name.to_string())
}

impl std::fmt::Display for PciDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {} {} (driver: {}, IOMMU group: {})",
            self.address,
            self.vendor_name,
            self.device_name,
            self.driver.as_deref().unwrap_or("none"),
            self.iommu_group
                .map(|g| g.to_string())
                .unwrap_or_else(|| "none".into()),
        )
    }
}

impl std::fmt::Display for PciClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            PciClass::Gpu => "GPU",
            PciClass::Audio => "Audio",
            PciClass::Network => "Network",
            PciClass::Nvme => "NVMe",
            PciClass::UsbController => "USB Controller",
            PciClass::Storage => "Storage",
            PciClass::Bridge => "PCI Bridge",
            PciClass::Other => "Other",
        };
        write!(f, "{}", label)
    }
}

impl std::fmt::Display for IommuGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "IOMMU Group {}:", self.id)?;
        for dev in &self.devices {
            writeln!(f, "  {}", dev)?;
        }
        Ok(())
    }
}
