//! VFIO passthrough bind/unbind operations and system readiness checks.
//!
//! Provides helpers to bind PCI devices to `vfio-pci` for passthrough to a VM,
//! restore them to their original kernel drivers afterward, and diagnose common
//! VFIO setup issues (missing IOMMU, kernel modules, boot parameters).
//!
//! **Important:** Binding and unbinding PCI drivers requires root privileges.
//! Operations use `pkexec` (preferred) or `sudo` as a fallback.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::pci;

const SYSFS_PCI_DEVICES: &str = "/sys/bus/pci/devices";
const SYSFS_IOMMU_GROUPS: &str = "/sys/kernel/iommu_groups";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// System readiness report for VFIO passthrough.
#[derive(Debug)]
pub struct VfioStatus {
    /// Whether the kernel has created IOMMU groups.
    pub iommu_enabled: bool,
    /// Whether the `vfio-pci` module (or built-in) is available.
    pub vfio_loaded: bool,
    /// Whether the kernel command line contains `intel_iommu=on` or `amd_iommu=on`.
    pub kernel_params_ok: bool,
    /// Problems detected.
    pub issues: Vec<String>,
    /// Actionable suggestions for the user.
    pub suggestions: Vec<String>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Bind a PCI device to the `vfio-pci` driver.
///
/// Steps (all require root):
/// 1. Read the current driver (if any) from sysfs.
/// 2. Unbind from the current driver.
/// 3. Register the vendor:device pair with `vfio-pci` via `new_id`.
/// 4. Bind the device to `vfio-pci`.
///
/// Returns `Ok(())` on success or a descriptive error.
pub fn bind_to_vfio(address: &str) -> Result<(), String> {
    if !is_valid_pci_address(address) {
        return Err(format!("Invalid PCI address: {}", address));
    }

    // Already bound?
    if pci::is_bound_to_vfio(address) {
        return Ok(());
    }

    let dev_path = PathBuf::from(SYSFS_PCI_DEVICES).join(address);
    if !dev_path.is_dir() {
        return Err(format!("PCI device not found: {}", address));
    }

    // Read vendor and device IDs for new_id registration.
    let vendor_id = read_sysfs_id(&dev_path.join("vendor"))?;
    let device_id = read_sysfs_id(&dev_path.join("device"))?;

    // Step 1: Unbind from current driver (if any).
    let unbind_path = dev_path.join("driver").join("unbind");
    if unbind_path.exists() {
        write_privileged(&unbind_path.to_string_lossy(), address)?;
    }

    // Step 2: Register vendor:device with vfio-pci so it will accept the device.
    let new_id = format!("{} {}", vendor_id, device_id);
    write_privileged("/sys/bus/pci/drivers/vfio-pci/new_id", &new_id)
        // new_id write fails with EEXIST if already registered — that is fine.
        .or_else(|e| {
            if e.contains("File exists") || e.contains("EEXIST") {
                Ok(())
            } else {
                Err(e)
            }
        })?;

    // Step 3: Bind to vfio-pci.
    write_privileged("/sys/bus/pci/drivers/vfio-pci/bind", address)?;

    Ok(())
}

/// Unbind a PCI device from `vfio-pci` and restore its original driver.
///
/// `original_driver` is the kernel module name to rebind (e.g. "nvidia",
/// "amdgpu", "nvme"). The caller should have saved this before calling
/// [`bind_to_vfio`].
pub fn unbind_from_vfio(address: &str, original_driver: &str) -> Result<(), String> {
    if !is_valid_pci_address(address) {
        return Err(format!("Invalid PCI address: {}", address));
    }
    if !is_valid_driver_name(original_driver) {
        return Err(format!("Invalid driver name: {}", original_driver));
    }

    // Unbind from vfio-pci.
    let unbind_path = format!("{}/{}/driver/unbind", SYSFS_PCI_DEVICES, address);
    if Path::new(&unbind_path).exists() {
        write_privileged(&unbind_path, address)?;
    }

    // Rebind to the original driver.
    let bind_path = format!("/sys/bus/pci/drivers/{}/bind", original_driver);
    write_privileged(&bind_path, address)?;

    Ok(())
}

/// Bind **all** devices in an IOMMU group to `vfio-pci`.
///
/// This is required because VFIO operates at the IOMMU group level: every
/// device in the group must be bound to `vfio-pci` (or have no driver at all)
/// before any of them can be passed through.
///
/// Returns a list of `(pci_address, original_driver)` tuples so the caller
/// can restore them later with [`restore_drivers`].
pub fn bind_iommu_group_to_vfio(group_id: u32) -> Result<Vec<(String, String)>, String> {
    let devices_dir = PathBuf::from(SYSFS_IOMMU_GROUPS)
        .join(group_id.to_string())
        .join("devices");

    if !devices_dir.is_dir() {
        return Err(format!("IOMMU group {} not found", group_id));
    }

    let entries = fs::read_dir(&devices_dir)
        .map_err(|e| format!("Failed to read IOMMU group {}: {}", group_id, e))?;

    let mut bound: Vec<(String, String)> = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|e| format!("Read error: {}", e))?;
        let address = entry.file_name().to_string_lossy().to_string();

        // Record original driver (if any).
        let dev_path = PathBuf::from(SYSFS_PCI_DEVICES).join(&address);
        let original_driver = read_driver_name(&dev_path).unwrap_or_default();

        // Skip devices already on vfio-pci.
        if original_driver == "vfio-pci" {
            bound.push((address, "vfio-pci".to_string()));
            continue;
        }

        bind_to_vfio(&address).map_err(|e| {
            // Roll back devices we already bound in this call.
            for (addr, drv) in &bound {
                if drv != "vfio-pci" && !drv.is_empty() {
                    let _ = unbind_from_vfio(addr, drv);
                }
            }
            format!(
                "Failed to bind {} to vfio-pci: {}. Rolled back.",
                address, e
            )
        })?;

        bound.push((address, original_driver));
    }

    Ok(bound)
}

/// Restore all devices from `vfio-pci` to their original drivers.
///
/// Accepts the list returned by [`bind_iommu_group_to_vfio`]. Devices whose
/// original driver is empty or "vfio-pci" are skipped.
pub fn restore_drivers(devices: &[(String, String)]) -> Result<(), String> {
    let mut errors: Vec<String> = Vec::new();

    for (address, original_driver) in devices {
        if original_driver.is_empty() || original_driver == "vfio-pci" {
            continue;
        }
        if let Err(e) = unbind_from_vfio(address, original_driver) {
            errors.push(format!("{}: {}", address, e));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Failed to restore some drivers:\n{}",
            errors.join("\n")
        ))
    }
}

/// Check the overall system readiness for VFIO passthrough.
pub fn check_vfio_readiness() -> VfioStatus {
    let mut issues = Vec::new();
    let mut suggestions = Vec::new();

    // 1. IOMMU groups
    let iommu_enabled = pci::is_iommu_enabled();
    if !iommu_enabled {
        issues.push("IOMMU is not enabled — no IOMMU groups found.".into());
        suggestions.push(
            "Add 'intel_iommu=on iommu=pt' (Intel) or 'amd_iommu=on iommu=pt' (AMD) \
             to your kernel command line (e.g. in /etc/default/grub GRUB_CMDLINE_LINUX_DEFAULT) \
             and reboot."
                .into(),
        );
    }

    // 2. vfio-pci module
    let vfio_loaded = is_module_available("vfio_pci");
    if !vfio_loaded {
        issues.push("vfio-pci kernel module is not loaded.".into());
        suggestions.push(
            "Load it with 'sudo modprobe vfio-pci' or add 'vfio-pci' to /etc/modules-load.d/."
                .into(),
        );
    }

    // Also check for vfio base module.
    if !is_module_available("vfio") {
        issues.push("vfio base kernel module is not loaded.".into());
        suggestions.push("Load it with 'sudo modprobe vfio'.".into());
    }

    // 3. Kernel command line parameters
    let kernel_params_ok = check_kernel_iommu_params();
    if !kernel_params_ok {
        issues.push(
            "Kernel command line does not contain 'intel_iommu=on' or 'amd_iommu=on'.".into(),
        );
        suggestions.push(
            "Edit /etc/default/grub, add the appropriate IOMMU parameter, run update-grub, \
             and reboot."
                .into(),
        );
    }

    // 4. Check for /dev/vfio/vfio character device
    if !Path::new("/dev/vfio/vfio").exists() {
        issues.push("/dev/vfio/vfio device node does not exist.".into());
        suggestions.push(
            "This usually means the vfio module is not loaded. Try 'sudo modprobe vfio-pci'."
                .into(),
        );
    }

    VfioStatus {
        iommu_enabled,
        vfio_loaded,
        kernel_params_ok,
        issues,
        suggestions,
    }
}

/// Generate a modprobe configuration snippet for `vfio-pci`.
///
/// The output can be written to `/etc/modprobe.d/vfio.conf` to ensure
/// `vfio-pci` claims the specified devices at boot, before the native
/// driver loads.
///
/// `device_ids` is a slice of `(vendor_id, device_id)` pairs (hex, no 0x
/// prefix).
pub fn generate_modprobe_config(device_ids: &[(String, String)]) -> String {
    if device_ids.is_empty() {
        return String::from("# No devices specified for VFIO passthrough.\n");
    }

    let ids: Vec<String> = device_ids
        .iter()
        .map(|(v, d)| format!("{}:{}", v, d))
        .collect();

    format!(
        "# VFIO passthrough — generated by Libre VMM\n\
         # Ensure vfio-pci loads before native GPU/device drivers.\n\
         softdep nvidia pre: vfio-pci\n\
         softdep amdgpu pre: vfio-pci\n\
         softdep nouveau pre: vfio-pci\n\
         softdep i915 pre: vfio-pci\n\
         \n\
         options vfio-pci ids={}\n",
        ids.join(",")
    )
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Write a value to a sysfs file using `pkexec` or `sudo`.
fn write_privileged(path: &str, value: &str) -> Result<(), String> {
    // Validate that path is under /sys/ to prevent arbitrary writes.
    if !path.starts_with("/sys/") && !path.starts_with("/dev/") {
        return Err(format!("Refusing to write to non-sysfs path: {}", path));
    }

    // Try pkexec first (Polkit — works with graphical auth prompts).
    let result = Command::new("pkexec")
        .args(["bash", "-c", &format!("echo '{}' > '{}'", value, path)])
        .output();

    match result {
        Ok(output) if output.status.success() => return Ok(()),
        _ => {},
    }

    // Fall back to sudo.
    let result = Command::new("sudo")
        .args(["bash", "-c", &format!("echo '{}' > '{}'", value, path)])
        .output();

    match result {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!(
                "Privileged write to {} failed: {}",
                path,
                stderr.trim()
            ))
        },
        Err(e) => Err(format!(
            "Could not execute privileged command: {}. \
             Ensure pkexec or sudo is available.",
            e
        )),
    }
}

/// Read driver basename from the sysfs driver symlink.
fn read_driver_name(dev_path: &Path) -> Option<String> {
    let link = fs::read_link(dev_path.join("driver")).ok()?;
    link.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
}

/// Read a sysfs ID file (vendor or device) and return the hex value without
/// the "0x" prefix.
fn read_sysfs_id(path: &Path) -> Result<String, String> {
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    Ok(raw.trim().trim_start_matches("0x").to_lowercase())
}

/// Check if a kernel module is loaded (appears in `/proc/modules`) or is
/// built-in (appears in `/sys/module/`).
fn is_module_available(module: &str) -> bool {
    // Check /sys/module/ first (works for both loaded modules and built-ins).
    if Path::new(&format!("/sys/module/{}", module)).is_dir() {
        return true;
    }

    // Fallback: parse /proc/modules.
    if let Ok(modules) = fs::read_to_string("/proc/modules") {
        for line in modules.lines() {
            if line.starts_with(module) && line[module.len()..].starts_with(' ') {
                return true;
            }
        }
    }

    false
}

/// Check `/proc/cmdline` for IOMMU kernel parameters.
fn check_kernel_iommu_params() -> bool {
    let cmdline = fs::read_to_string("/proc/cmdline").unwrap_or_default();
    cmdline.contains("intel_iommu=on") || cmdline.contains("amd_iommu=on")
}

/// Basic validation of a PCI address format (domain:bus:slot.function).
fn is_valid_pci_address(address: &str) -> bool {
    // Expected format: 0000:00:00.0
    let parts: Vec<&str> = address.split(':').collect();
    if parts.len() != 3 {
        return false;
    }
    // Domain: 4 hex digits
    if parts[0].len() != 4 || !parts[0].chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    // Bus: 2 hex digits
    if parts[1].len() != 2 || !parts[1].chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    // Slot.Function: "XX.X"
    let sf: Vec<&str> = parts[2].split('.').collect();
    if sf.len() != 2 {
        return false;
    }
    if sf[0].len() != 2 || !sf[0].chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    if sf[1].len() != 1 || !sf[1].chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    true
}

/// Validate a driver name contains only safe characters.
fn is_valid_driver_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ---------------------------------------------------------------------------
// Wave 12.2: Single-GPU passthrough wizard helpers
// ---------------------------------------------------------------------------
//
// The single-GPU passthrough flow needs to:
//   1. Detect whether the host actually has only one GPU.
//   2. Detect the active display manager so we can stop/start it.
//   3. Render the libvirt hook scripts that detach/reattach the GPU.
//
// Everything here is a pure helper or a read-only sysfs/path lookup. Nothing
// in this module writes to /etc/*, /sys/*, or any privileged location. Scripts
// are returned as strings so the UI can present them for user review (and the
// user is the one who saves and installs them).
//
// SECURITY: All template substitution paths run through validation helpers
// (`validate_vm_name`, `validate_pci_bus`, allowlisted display-manager names).
// This avoids command injection (CWE-78) when the strings end up inside shell
// scripts.

/// Allowlist of well-known display managers. Anything outside this list is
/// rejected by [`detect_display_manager`] / treated as `None` to prevent
/// arbitrary identifiers from being substituted into a shell script.
pub const KNOWN_DISPLAY_MANAGERS: &[&str] = &[
    "gdm", "gdm3", "sddm", "lightdm", "ly", "lxdm", "kdm", "slim",
];

/// Detect the system's display manager by reading the
/// `/etc/systemd/system/display-manager.service` symlink.
///
/// Returns the bare unit name (e.g. `"gdm"`, `"sddm"`) if it matches the
/// allowlist in [`KNOWN_DISPLAY_MANAGERS`]; otherwise `None`.
pub fn detect_display_manager() -> Option<String> {
    let link = fs::read_link("/etc/systemd/system/display-manager.service").ok()?;
    let file = link.file_name()?.to_str()?.to_string();
    // Strip ".service" suffix if present.
    let base = file.strip_suffix(".service").unwrap_or(&file).to_string();
    if KNOWN_DISPLAY_MANAGERS.contains(&base.as_str()) {
        Some(base)
    } else {
        None
    }
}

/// Count GPUs on the host (PCI class `0300` VGA or `0302` 3D).
pub fn host_gpu_count() -> usize {
    crate::pci::find_gpus().len()
}

/// Read the currently active TTY from `/sys/class/tty/tty0/active`.
/// Returns something like `"tty1"` if available.
pub fn detect_active_tty() -> Option<String> {
    let raw = fs::read_to_string("/sys/class/tty/tty0/active").ok()?;
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Validate a PCI bus identifier (the `DDDD:BB:SS.F` form used by sysfs).
///
/// Accepts both forms:
/// * Full address: `0000:01:00.0`
/// * Without domain: `01:00.0` (some scripts use this short form)
pub fn validate_pci_bus(s: &str) -> bool {
    if s.is_empty() || s.len() > 12 {
        return false;
    }
    let parts: Vec<&str> = s.split(':').collect();
    let (bus, slot_func) = match parts.len() {
        3 => {
            // domain:bus:slot.function
            if parts[0].len() != 4 || !parts[0].chars().all(|c| c.is_ascii_hexdigit()) {
                return false;
            }
            (parts[1], parts[2])
        },
        2 => (parts[0], parts[1]),
        _ => return false,
    };

    if bus.len() != 2 || !bus.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let sf: Vec<&str> = slot_func.split('.').collect();
    if sf.len() != 2 {
        return false;
    }
    if sf[0].len() != 2 || !sf[0].chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    if sf[1].len() != 1 || !sf[1].chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    true
}

/// Validate a VM name for use inside the hook-script template.
///
/// Stricter than `vmm_core::config::validate_vm_name` — we forbid spaces and
/// dots too because the name is embedded directly inside a shell script and
/// used as a directory name. Allowed: ASCII letters, digits, `_`, `-`.
pub fn validate_vm_name_for_hook(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Validate a display-manager name against the allowlist.
pub fn validate_dm_name(name: &str) -> bool {
    KNOWN_DISPLAY_MANAGERS.contains(&name)
}

/// Reduce a sysfs PCI address ("0000:01:00.0") to the short bus form
/// ("01:00.0") that fits the `GPU_BUS` placeholder in the hook scripts.
/// Accepts already-short forms unchanged.
pub fn short_pci_bus(address: &str) -> String {
    // A full sysfs address has three colon-separated segments
    // (domain:bus:slot.function). Anything with fewer segments is already
    // "short" and we return it verbatim.
    let parts: Vec<&str> = address.splitn(2, ':').collect();
    if parts.len() == 2 && parts[0].len() == 4 {
        parts[1].to_string()
    } else {
        address.to_string()
    }
}

/// Render the `before-start.sh` hook script that detaches the GPU from the
/// host so it can be passed through to the VM.
///
/// Inputs are validated; any invalid input yields a placeholder comment so the
/// caller can still display *something* but cannot accidentally inject shell
/// metacharacters.
pub fn render_before_start_script(vm_name: &str, gpu_bus: &str, dm: &str) -> String {
    let safe_name = if validate_vm_name_for_hook(vm_name) {
        vm_name
    } else {
        "INVALID_VM_NAME"
    };
    let safe_bus = if validate_pci_bus(gpu_bus) {
        short_pci_bus(gpu_bus)
    } else {
        "INVALID_PCI_BUS".to_string()
    };
    let safe_dm = if validate_dm_name(dm) { dm } else { "" };

    format!(
        "#!/usr/bin/env bash\n\
         set -e\n\
         # Single-GPU passthrough: detach display from host before VM start\n\
         # VM: {name}\n\
         # Generated by Libre VMM Wave 12.2\n\
         \n\
         DM=\"{dm}\"\n\
         GPU_BUS=\"{bus}\"\n\
         GPU_NAME=\"GPU\"\n\
         \n\
         logger -t libre-vmm \"Stopping display manager $DM for VM {name}\"\n\
         if [ -n \"$DM\" ]; then\n    systemctl stop \"$DM\"\nfi\n\
         \n\
         # Switch to TTY1 so the framebuffer is freed\n\
         chvt 1\n\
         \n\
         # Unbind console from GPU framebuffer driver\n\
         echo 0 > /sys/class/vtconsole/vtcon0/bind 2>/dev/null || true\n\
         echo 0 > /sys/class/vtconsole/vtcon1/bind 2>/dev/null || true\n\
         echo efi-framebuffer.0 > /sys/bus/platform/drivers/efi-framebuffer/unbind 2>/dev/null || true\n\
         \n\
         sleep 2\n\
         \n\
         # Unbind the GPU from its current driver\n\
         for dev in 0000:$GPU_BUS-display 0000:$GPU_BUS-audio; do\n\
         \x20   if [ -e \"/sys/bus/pci/devices/0000:$GPU_BUS/driver/unbind\" ]; then\n\
         \x20       logger -t libre-vmm \"Unbinding $GPU_BUS from current driver\"\n\
         \x20       echo \"0000:$GPU_BUS\" > \"/sys/bus/pci/devices/0000:$GPU_BUS/driver/unbind\"\n\
         \x20   fi\n\
         done\n\
         \n\
         # Load vfio-pci\n\
         modprobe vfio-pci\n",
        name = safe_name,
        bus = safe_bus,
        dm = safe_dm,
    )
}

/// Render the `after-stop.sh` hook script that restores the display to the
/// host after the VM powers off.
pub fn render_after_stop_script(vm_name: &str, gpu_bus: &str, dm: &str) -> String {
    let safe_name = if validate_vm_name_for_hook(vm_name) {
        vm_name
    } else {
        "INVALID_VM_NAME"
    };
    let safe_bus = if validate_pci_bus(gpu_bus) {
        short_pci_bus(gpu_bus)
    } else {
        "INVALID_PCI_BUS".to_string()
    };
    let safe_dm = if validate_dm_name(dm) { dm } else { "" };

    format!(
        "#!/usr/bin/env bash\n\
         set -e\n\
         # Single-GPU passthrough: restore display to host after VM stop\n\
         # VM: {name}\n\
         \n\
         DM=\"{dm}\"\n\
         GPU_BUS=\"{bus}\"\n\
         \n\
         logger -t libre-vmm \"Restoring display for VM {name}\"\n\
         \n\
         # Rebind console\n\
         echo 1 > /sys/class/vtconsole/vtcon0/bind 2>/dev/null || true\n\
         echo 1 > /sys/class/vtconsole/vtcon1/bind 2>/dev/null || true\n\
         echo efi-framebuffer.0 > /sys/bus/platform/drivers/efi-framebuffer/bind 2>/dev/null || true\n\
         \n\
         # Restart display manager\n\
         if [ -n \"$DM\" ]; then\n    systemctl start \"$DM\"\nfi\n",
        name = safe_name,
        bus = safe_bus,
        dm = safe_dm,
    )
}

/// Render the suggested `/etc/sudoers.d/libre-vmm-<user>` file.
///
/// The output is *only* presented to the user for manual install via
/// `sudo visudo`. Libre VMM never writes to `/etc/sudoers.d/` itself.
pub fn render_sudoers_snippet(user: &str, hook_root: &Path) -> String {
    // Strip anything that isn't a safe username character so the file can't
    // smuggle in directives. The validation mirrors POSIX login-name rules.
    let safe_user: String = user
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    let safe_user = if safe_user.is_empty() {
        "USER".to_string()
    } else {
        safe_user
    };

    let dir = hook_root.display();
    format!(
        "# Generated by Libre VMM — install with `sudo visudo -f /etc/sudoers.d/libre-vmm-{user}`\n\
         {user} ALL=(root) NOPASSWD: {dir}/*\n",
        user = safe_user,
        dir = dir,
    )
}

/// Default location for per-VM hook scripts.
///
/// Returns `~/.local/share/libre-vmm/vfio-hooks/<vm_name>/`. Falls back to
/// `/tmp/libre-vmm-hooks/<vm_name>` if HOME is not set (mostly for tests).
pub fn hook_dir_for_vm(vm_name: &str) -> PathBuf {
    hook_dir_for_vm_with_base(vm_name, None)
}

/// Compute the hook directory using an explicit base override. When
/// `base_override` is `None`, falls back to `$HOME/.local/share/libre-vmm/vfio-hooks`.
/// This separation lets tests avoid mutating the process-wide `HOME` env var
/// (which corrupts parallel tests in other modules that also read `HOME`).
pub fn hook_dir_for_vm_with_base(
    vm_name: &str,
    base_override: Option<&std::path::Path>,
) -> PathBuf {
    let base: PathBuf = match base_override {
        Some(p) => p.to_path_buf(),
        None => std::env::var("HOME")
            .map(|h| PathBuf::from(h).join(".local/share/libre-vmm/vfio-hooks"))
            .unwrap_or_else(|_| PathBuf::from("/tmp/libre-vmm-hooks")),
    };
    base.join(vm_name)
}

/// Write the two hook scripts to disk with executable permissions
/// (owner rwx, group/other rx).
///
/// SECURITY: Refuses to write if the VM name fails validation; ensures the
/// path stays within `~/.local/share/libre-vmm/vfio-hooks/`.
pub fn save_hook_scripts(vm_name: &str, before: &str, after: &str) -> Result<PathBuf, String> {
    save_hook_scripts_with_base(vm_name, before, after, None)
}

/// Like [`save_hook_scripts`] but with an explicit base directory override
/// (used by tests to avoid mutating `HOME`).
pub fn save_hook_scripts_with_base(
    vm_name: &str,
    before: &str,
    after: &str,
    base_override: Option<&std::path::Path>,
) -> Result<PathBuf, String> {
    if !validate_vm_name_for_hook(vm_name) {
        return Err("Invalid VM name for hook scripts".into());
    }
    let dir = hook_dir_for_vm_with_base(vm_name, base_override);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create {}: {}", dir.display(), e))?;

    let before_path = dir.join("before-start.sh");
    let after_path = dir.join("after-stop.sh");

    fs::write(&before_path, before)
        .map_err(|e| format!("Failed to write {}: {}", before_path.display(), e))?;
    fs::write(&after_path, after)
        .map_err(|e| format!("Failed to write {}: {}", after_path.display(), e))?;

    // chmod 0755
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        let _ = fs::set_permissions(&before_path, perms.clone());
        let _ = fs::set_permissions(&after_path, perms);
    }

    Ok(dir)
}

impl std::fmt::Display for VfioStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "VFIO Readiness: IOMMU={}, vfio-pci={}, kernel_params={}",
            if self.iommu_enabled { "OK" } else { "MISSING" },
            if self.vfio_loaded { "OK" } else { "MISSING" },
            if self.kernel_params_ok {
                "OK"
            } else {
                "MISSING"
            },
        )?;
        if !self.issues.is_empty() {
            writeln!(f, "Issues:")?;
            for issue in &self.issues {
                writeln!(f, "  - {}", issue)?;
            }
        }
        if !self.suggestions.is_empty() {
            writeln!(f, "Suggestions:")?;
            for s in &self.suggestions {
                writeln!(f, "  - {}", s)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests (Wave 12.2 pure helpers)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_pci_bus_accepts_full_address() {
        assert!(validate_pci_bus("0000:01:00.0"));
        assert!(validate_pci_bus("0000:ff:1f.7"));
        assert!(validate_pci_bus("dead:be:ef.0"));
    }

    #[test]
    fn validate_pci_bus_accepts_short_form() {
        assert!(validate_pci_bus("01:00.0"));
        assert!(validate_pci_bus("ff:1f.7"));
    }

    #[test]
    fn validate_pci_bus_rejects_garbage() {
        assert!(!validate_pci_bus(""));
        assert!(!validate_pci_bus("not-a-bus"));
        assert!(!validate_pci_bus("0000:01:00"));
        assert!(!validate_pci_bus("0000:01:00.gg"));
        assert!(!validate_pci_bus("0000:01:00.0; rm -rf /"));
        assert!(!validate_pci_bus("00000:01:00.0"));
        // Too many segments
        assert!(!validate_pci_bus("0000:00:01:00.0"));
    }

    #[test]
    fn validate_vm_name_for_hook_allows_typical_names() {
        assert!(validate_vm_name_for_hook("win10"));
        assert!(validate_vm_name_for_hook("gaming_vm"));
        assert!(validate_vm_name_for_hook("test-vm-1"));
    }

    #[test]
    fn validate_vm_name_for_hook_rejects_unsafe() {
        assert!(!validate_vm_name_for_hook(""));
        assert!(!validate_vm_name_for_hook("name with space"));
        assert!(!validate_vm_name_for_hook("name;rm -rf /"));
        assert!(!validate_vm_name_for_hook("../etc/passwd"));
        assert!(!validate_vm_name_for_hook("name.dot"));
        assert!(!validate_vm_name_for_hook(&"a".repeat(65)));
    }

    #[test]
    fn validate_dm_name_uses_allowlist() {
        assert!(validate_dm_name("gdm"));
        assert!(validate_dm_name("sddm"));
        assert!(validate_dm_name("lightdm"));
        assert!(validate_dm_name("ly"));
        assert!(!validate_dm_name("evilsh"));
        assert!(!validate_dm_name(""));
        assert!(!validate_dm_name("gdm; rm -rf /"));
    }

    #[test]
    fn short_pci_bus_strips_domain() {
        assert_eq!(short_pci_bus("0000:01:00.0"), "01:00.0");
        assert_eq!(short_pci_bus("01:00.0"), "01:00.0");
    }

    #[test]
    fn before_script_contains_vm_and_bus() {
        let s = render_before_start_script("gamingvm", "0000:01:00.0", "gdm");
        assert!(s.starts_with("#!/usr/bin/env bash\n"));
        assert!(s.contains("VM: gamingvm"));
        assert!(s.contains("GPU_BUS=\"01:00.0\""));
        assert!(s.contains("DM=\"gdm\""));
        assert!(s.contains("systemctl stop"));
        assert!(s.contains("modprobe vfio-pci"));
    }

    #[test]
    fn after_script_contains_vm_and_bus() {
        let s = render_after_stop_script("gamingvm", "0000:01:00.0", "gdm");
        assert!(s.contains("VM: gamingvm"));
        assert!(s.contains("DM=\"gdm\""));
        assert!(s.contains("GPU_BUS=\"01:00.0\""));
        assert!(s.contains("systemctl start"));
    }

    #[test]
    fn render_scripts_sanitize_bad_inputs() {
        // Bad VM name should be replaced with INVALID_VM_NAME — no injection.
        let s = render_before_start_script("bad name; rm -rf /", "0000:01:00.0", "gdm");
        assert!(!s.contains("rm -rf /"));
        assert!(s.contains("INVALID_VM_NAME"));

        // Bad PCI bus is replaced.
        let s = render_before_start_script("ok", "$(curl evil)", "gdm");
        assert!(!s.contains("curl"));
        assert!(s.contains("INVALID_PCI_BUS"));

        // Bad DM (not in allowlist) becomes empty rather than being inserted raw.
        let s = render_before_start_script("ok", "0000:01:00.0", "evil; halt");
        assert!(!s.contains("halt"));
        assert!(s.contains("DM=\"\""));
    }

    #[test]
    fn render_sudoers_uses_safe_username() {
        let snippet = render_sudoers_snippet("alice", Path::new("/home/alice/hooks"));
        assert!(snippet.contains("alice ALL=(root) NOPASSWD: /home/alice/hooks/*"));

        // Bad username characters are stripped; nothing exotic survives.
        let snippet = render_sudoers_snippet("bob; rm -rf /", Path::new("/tmp"));
        assert!(snippet.contains("bobrm-rf "));
        assert!(!snippet.contains(";"));
    }

    #[test]
    fn hook_dir_for_vm_lands_under_local_share() {
        // Use the explicit-base variant — mutating $HOME process-wide breaks
        // parallel tests in other modules that also read $HOME (restricted::tests).
        let base = std::path::Path::new("/tmp/lvmmtest-home/.local/share/libre-vmm/vfio-hooks");
        let p = hook_dir_for_vm_with_base("vm1", Some(base));
        assert!(p.starts_with("/tmp/lvmmtest-home/.local/share/libre-vmm/vfio-hooks"));
        assert!(p.ends_with("vm1"));
    }

    #[test]
    fn save_hook_scripts_rejects_bad_name() {
        let err = save_hook_scripts("../etc/passwd", "", "").unwrap_err();
        assert!(err.contains("Invalid"));
    }

    #[test]
    fn save_hook_scripts_writes_two_files() {
        // Use the explicit-base variant — mutating $HOME process-wide breaks
        // parallel tests in other modules.
        let tmp = std::env::temp_dir().join(format!(
            "lvmm-hooks-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&tmp).expect("mkdir tmp");
        let before = render_before_start_script("testvm", "0000:01:00.0", "gdm");
        let after = render_after_stop_script("testvm", "0000:01:00.0", "gdm");
        let dir = save_hook_scripts_with_base("testvm", &before, &after, Some(&tmp)).expect("save");
        assert!(dir.join("before-start.sh").is_file());
        assert!(dir.join("after-stop.sh").is_file());

        // Cleanup
        let _ = fs::remove_dir_all(&tmp);
    }
}
