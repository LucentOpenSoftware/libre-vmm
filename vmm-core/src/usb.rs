//! USB device management — list host USB devices, hotplug attach/detach.

use crate::error::{VmmError, VmmResult};
use tracing::info;
use virt::connect::Connect;
use virt::domain::Domain;

/// Info about a host USB device.
#[derive(Debug, Clone)]
pub struct UsbDevice {
    /// USB bus number
    pub bus: u32,
    /// USB device number
    pub device: u32,
    /// Vendor ID (hex string like "1234")
    pub vendor_id: String,
    /// Product ID (hex string like "5678")
    pub product_id: String,
    /// Vendor name (human-readable)
    pub vendor_name: String,
    /// Product name (human-readable)
    pub product_name: String,
    /// Whether the device is currently attached to a VM
    pub attached: bool,
}

impl UsbDevice {
    /// Short display label for the device.
    pub fn display_label(&self) -> String {
        if !self.product_name.is_empty() {
            format!(
                "{} {} ({}:{})",
                self.vendor_name, self.product_name, self.vendor_id, self.product_id
            )
        } else {
            format!(
                "USB {}:{} (Bus {} Dev {})",
                self.vendor_id, self.product_id, self.bus, self.device
            )
        }
    }
}

/// SECURITY (CWE-22, CWE-59): Allowed sysfs attribute names for USB device reads.
/// Only these known-safe attribute names may be read. This prevents path traversal
/// attacks where a crafted attribute name like "../../etc/shadow" could read
/// arbitrary files. Defense-in-depth even though callers currently use hardcoded names.
const ALLOWED_SYSFS_ATTRS: &[&str] = &[
    "idVendor",
    "idProduct",
    "manufacturer",
    "product",
    "busnum",
    "devnum",
    "serial",
    "speed",
    "bDeviceClass",
    "bDeviceSubClass",
    "bDeviceProtocol",
];

/// List USB devices on the host by reading /sys/bus/usb/devices.
pub fn list_host_usb_devices() -> Vec<UsbDevice> {
    let mut devices = Vec::new();
    let usb_base = std::path::Path::new("/sys/bus/usb/devices");

    if !usb_base.exists() {
        return devices;
    }

    // SECURITY (CWE-59): Canonicalize the sysfs base path to prevent symlink redirection.
    // Ensures we are actually reading from /sys/bus/usb/devices and not a symlink
    // pointing elsewhere.
    let usb_base = match usb_base.canonicalize() {
        Ok(p) => p,
        Err(_) => return devices,
    };

    // SECURITY: Verify canonicalized path is still under /sys/
    if !usb_base.starts_with("/sys/") {
        return devices;
    }

    if let Ok(entries) = std::fs::read_dir(&usb_base) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            // Only look at actual device entries (e.g. "1-2", not "usb1" hubs)
            if !name.contains('-') || name.contains(':') {
                continue;
            }

            // SECURITY (CWE-59): Canonicalize each device path and verify it's under /sys/
            let canonical_path = match path.canonicalize() {
                Ok(p) => p,
                Err(_) => continue,
            };
            if !canonical_path.starts_with("/sys/") {
                continue;
            }

            let vendor_id = read_sysfs_attr(&canonical_path, "idVendor");
            let product_id = read_sysfs_attr(&canonical_path, "idProduct");
            let vendor_name = read_sysfs_attr(&canonical_path, "manufacturer");
            let product_name = read_sysfs_attr(&canonical_path, "product");
            let busnum = read_sysfs_attr(&canonical_path, "busnum")
                .parse::<u32>()
                .unwrap_or(0);
            let devnum = read_sysfs_attr(&canonical_path, "devnum")
                .parse::<u32>()
                .unwrap_or(0);

            // Skip root hubs (vendor 1d6b = Linux Foundation)
            if vendor_id == "1d6b" {
                continue;
            }

            // Skip devices with no vendor
            if vendor_id.is_empty() {
                continue;
            }

            devices.push(UsbDevice {
                bus: busnum,
                device: devnum,
                vendor_id,
                product_id,
                vendor_name,
                product_name,
                attached: false,
            });
        }
    }

    devices.sort_by(|a, b| a.bus.cmp(&b.bus).then_with(|| a.device.cmp(&b.device)));

    devices
}

/// Validate USB vendor/product ID: must be exactly 4 hex chars (CWE-20).
///
/// SECURITY: This validation is critical because vendor/product IDs are interpolated
/// into XML passed to libvirt's attach_device/detach_device. Allowing non-hex characters
/// could enable XML injection (CWE-91) into the libvirt domain XML.
fn validate_usb_id(id: &str, label: &str) -> VmmResult<()> {
    if id.is_empty() || id.len() > 4 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(VmmError::Other(format!(
            "Invalid USB {} ID '{}': must be 1-4 hex characters",
            label, id
        )));
    }
    Ok(())
}

/// SECURITY (CWE-20): Validate VM name to prevent XML injection (CWE-91) in libvirt calls.
/// VM names should contain only alphanumeric, hyphen, underscore, and period characters.
fn validate_vm_name(name: &str) -> VmmResult<()> {
    if name.is_empty() {
        return Err(VmmError::Other("VM name cannot be empty".to_string()));
    }
    if name.len() > 255 {
        return Err(VmmError::Other(
            "VM name too long (max 255 chars)".to_string(),
        ));
    }
    // libvirt allows a limited character set for domain names
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ')
    {
        return Err(VmmError::Other(format!(
            "Invalid VM name '{}': only alphanumeric, hyphen, underscore, and period allowed (CWE-20)",
            name
        )));
    }
    Ok(())
}

/// Attach a USB device to a running VM by vendor:product ID.
pub fn attach_usb_device(
    conn: &Connect,
    vm_name: &str,
    vendor_id: &str,
    product_id: &str,
) -> VmmResult<()> {
    // SECURITY (CWE-20): Validate all inputs before use
    validate_vm_name(vm_name)?;
    validate_usb_id(vendor_id, "vendor")?;
    validate_usb_id(product_id, "product")?;

    let domain = Domain::lookup_by_name(conn, vm_name).map_err(|_| VmmError::VmNotFound {
        name: vm_name.to_string(),
    })?;

    // SECURITY (CWE-91): vendor_id and product_id are validated as hex-only above,
    // preventing XML injection into this template. vm_name is not interpolated here.
    let xml = format!(
        r#"<hostdev mode='subsystem' type='usb' managed='yes'>
  <source>
    <vendor id='0x{}'/>
    <product id='0x{}'/>
  </source>
</hostdev>"#,
        vendor_id, product_id,
    );

    domain
        .attach_device_flags(&xml, virt::sys::VIR_DOMAIN_AFFECT_LIVE)
        .map_err(|e| VmmError::Other(format!("Failed to attach USB device: {}", e)))?;

    info!(
        "USB device {}:{} attached to VM '{}'",
        vendor_id, product_id, vm_name
    );
    Ok(())
}

/// Detach a USB device from a running VM.
pub fn detach_usb_device(
    conn: &Connect,
    vm_name: &str,
    vendor_id: &str,
    product_id: &str,
) -> VmmResult<()> {
    // SECURITY (CWE-20): Validate all inputs before use
    validate_vm_name(vm_name)?;
    validate_usb_id(vendor_id, "vendor")?;
    validate_usb_id(product_id, "product")?;

    let domain = Domain::lookup_by_name(conn, vm_name).map_err(|_| VmmError::VmNotFound {
        name: vm_name.to_string(),
    })?;

    // SECURITY (CWE-91): vendor_id and product_id are validated as hex-only above
    let xml = format!(
        r#"<hostdev mode='subsystem' type='usb' managed='yes'>
  <source>
    <vendor id='0x{}'/>
    <product id='0x{}'/>
  </source>
</hostdev>"#,
        vendor_id, product_id,
    );

    domain
        .detach_device_flags(&xml, virt::sys::VIR_DOMAIN_AFFECT_LIVE)
        .map_err(|e| VmmError::Other(format!("Failed to detach USB device: {}", e)))?;

    info!(
        "USB device {}:{} detached from VM '{}'",
        vendor_id, product_id, vm_name
    );
    Ok(())
}

/// Read a sysfs attribute file, trimming whitespace.
///
/// SECURITY (CWE-22, CWE-59): Validates the attribute name against an allowlist
/// to prevent path traversal. Only reads from the pre-validated device_path
/// which has already been canonicalized and verified to be under /sys/.
fn read_sysfs_attr(device_path: &std::path::Path, attr: &str) -> String {
    // SECURITY (CWE-22): Validate attribute name against allowlist to prevent
    // path traversal via crafted attribute names like "../../etc/shadow".
    if !ALLOWED_SYSFS_ATTRS.contains(&attr) {
        return String::new();
    }

    // SECURITY (CWE-22): Defense-in-depth — reject attr names containing
    // path separators, null bytes, or traversal sequences even if in allowlist.
    if attr.contains('/') || attr.contains('\\') || attr.contains('\0') || attr.contains("..") {
        return String::new();
    }

    let path = device_path.join(attr);

    // SECURITY (CWE-59): Verify the resolved path is still under /sys/ to prevent
    // symlink escape attacks. sysfs entries are often symlinks, but they should
    // always resolve within /sys/.
    match path.canonicalize() {
        Ok(canonical) => {
            if !canonical.starts_with("/sys/") {
                return String::new();
            }
            std::fs::read_to_string(canonical)
                .unwrap_or_default()
                .trim()
                .to_string()
        },
        Err(_) => String::new(),
    }
}
