//! Manages the connection to the libvirt hypervisor.

use crate::config::{self, VmConfig, VmConfigIo};
use crate::domain::{VmInfo, VmState};
use crate::error::{VmmError, VmmResult};
use crate::restricted::{self, RestrictionPolicy};
use crate::xml_builder;
use quick_xml::events::Event;
use quick_xml::Reader;
use tracing::{debug, info, warn};
use virt::connect::Connect;
use virt::domain::Domain;
use virt::sys;

/// Wraps a libvirt connection and provides high-level VM operations.
pub struct HypervisorConnection {
    conn: Connect,
    /// The URI used to establish this connection (for display / reconnect).
    uri: String,
    /// Whether this is a remote connection.
    is_remote: bool,
}

impl HypervisorConnection {
    /// Connect to the system QEMU/KVM hypervisor.
    pub fn connect_system() -> VmmResult<Self> {
        info!("Connecting to qemu:///system");
        let conn = Connect::open(Some("qemu:///system"))?;
        Ok(Self {
            conn,
            uri: "qemu:///system".to_string(),
            is_remote: false,
        })
    }

    /// Connect to the session (user-level, no root needed) hypervisor.
    pub fn connect_session() -> VmmResult<Self> {
        info!("Connecting to qemu:///session");
        let conn = Connect::open(Some("qemu:///session"))?;
        Ok(Self {
            conn,
            uri: "qemu:///session".to_string(),
            is_remote: false,
        })
    }

    /// Try system first, fall back to session.
    pub fn connect_best() -> VmmResult<Self> {
        match Self::connect_system() {
            Ok(c) => Ok(c),
            Err(e) => {
                warn!("System connection failed ({e}), trying session...");
                Self::connect_session()
            },
        }
    }

    /// Connect to a remote hypervisor via URI (e.g. qemu+ssh://user@host/system).
    ///
    /// SECURITY (CWE-918): Validates the URI scheme against an allowlist to prevent
    /// SSRF attacks where arbitrary protocols could be used to probe internal services.
    pub fn connect_remote(uri: &str) -> VmmResult<Self> {
        // SECURITY (CWE-918): Allowlist valid libvirt URI schemes to prevent SSRF.
        // Without this, an attacker-controlled URI could use unexpected transport
        // protocols to probe internal network services.
        const ALLOWED_SCHEMES: &[&str] = &[
            "qemu://",
            "qemu:///",
            "qemu+ssh://",
            "qemu+tls://",
            "qemu+tcp://",
        ];
        let uri_lower = uri.to_lowercase();
        if !ALLOWED_SCHEMES.iter().any(|s| uri_lower.starts_with(s)) {
            return Err(VmmError::InvalidConfig(format!(
                "Unsupported libvirt URI scheme. Allowed: qemu://, qemu+ssh://, qemu+tls://, qemu+tcp://. Got: {}",
                uri
            )));
        }
        // SECURITY: Block null bytes that could truncate the URI in C libraries (CWE-626)
        if uri.contains('\0') {
            return Err(VmmError::InvalidConfig(
                "URI must not contain null bytes".to_string(),
            ));
        }
        info!("Connecting to remote: {}", uri);
        let conn = Connect::open(Some(uri))?;
        Ok(Self {
            conn,
            uri: uri.to_string(),
            is_remote: true,
        })
    }

    /// Get the connection URI.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Whether this connection is to a remote host.
    pub fn is_remote(&self) -> bool {
        self.is_remote
    }

    /// Get hypervisor info string.
    pub fn hypervisor_info(&self) -> VmmResult<String> {
        let hv_type = self.conn.get_type().unwrap_or_default();
        let ver = self.conn.get_hyp_version().unwrap_or(0);
        let major = ver / 1_000_000;
        let minor = (ver % 1_000_000) / 1000;
        let patch = ver % 1000;
        Ok(format!("{} {}.{}.{}", hv_type, major, minor, patch))
    }

    /// Check if KVM hardware acceleration is available.
    pub fn kvm_available(&self) -> bool {
        std::path::Path::new("/dev/kvm").exists()
    }

    /// List all VMs managed by Libre VMM.
    pub fn list_vms(&self) -> VmmResult<Vec<VmInfo>> {
        let flags = sys::VIR_CONNECT_LIST_DOMAINS_ACTIVE | sys::VIR_CONNECT_LIST_DOMAINS_INACTIVE;
        let domains = self.conn.list_all_domains(flags)?;
        let mut vms = Vec::with_capacity(domains.len());

        for domain in domains {
            let name = domain.get_name().unwrap_or_default();
            let info = domain.get_info()?;
            let uuid_str = domain.get_uuid_string().unwrap_or_default();

            let state = match info.state as u32 {
                sys::VIR_DOMAIN_RUNNING => VmState::Running,
                sys::VIR_DOMAIN_PAUSED => VmState::Paused,
                sys::VIR_DOMAIN_SHUTDOWN => VmState::ShuttingDown,
                sys::VIR_DOMAIN_SHUTOFF => VmState::Off,
                sys::VIR_DOMAIN_CRASHED => VmState::Crashed,
                sys::VIR_DOMAIN_PMSUSPENDED => VmState::Suspended,
                _ => VmState::Unknown,
            };

            vms.push(VmInfo {
                name,
                uuid: uuid_str,
                state,
                vcpus: info.nr_virt_cpu as u32,
                memory_mib: info.memory / 1024,
                cpu_time_ns: info.cpu_time,
            });
        }

        Ok(vms)
    }

    /// Create and define a new VM from config.
    ///
    /// SECURITY: Validates VM name, cleans up disk and libvirt definition on partial
    /// failure to prevent resource leaks (CWE-404, CWE-459).
    pub fn create_vm(&self, config: &VmConfig) -> VmmResult<()> {
        // SECURITY (CWE-20): Validate VM name before any operations.
        // The name is used in virsh commands, file paths, and XML — must be safe.
        if let Some(err) = config::validate_vm_name(&config.name) {
            return Err(VmmError::InvalidConfig(format!(
                "Invalid VM name '{}': {} (CWE-20)",
                config.name, err
            )));
        }

        info!("Creating VM: {}", config.name);

        // Ensure disk directory exists
        if let Some(parent) = std::path::Path::new(&config.disk_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Create the disk image
        crate::disk::create_qcow2(&config.disk_path, config.disk_size_gib)?;

        // Fix permissions so libvirt's qemu user can access the disk.
        fix_disk_permissions(&config.disk_path);

        // Generate and define the domain XML
        let xml = xml_builder::build_domain_xml(config);
        debug!("Domain XML:\n{}", xml);

        // SECURITY (CWE-404): If define_xml fails, clean up the disk we just created.
        let domain = match Domain::define_xml(&self.conn, &xml) {
            Ok(d) => d,
            Err(e) => {
                warn!("define_xml failed, cleaning up disk: {}", config.disk_path);
                let _ = std::fs::remove_file(&config.disk_path);
                return Err(e.into());
            },
        };

        // Set autostart if requested
        if config.autostart {
            if let Err(e) = domain.set_autostart(true) {
                warn!("Failed to set autostart for '{}': {}", config.name, e);
            }
        }

        // SECURITY (CWE-459): If config save fails, undefine the domain and remove disk
        // to avoid orphaned libvirt definitions without corresponding config files.
        if let Err(e) = config.save() {
            warn!("config.save() failed, rolling back domain and disk: {}", e);
            let _ = domain.undefine();
            let _ = std::fs::remove_file(&config.disk_path);
            return Err(e);
        }

        info!("VM '{}' created successfully", config.name);
        Ok(())
    }

    /// Create and define a VM from an existing config (disk already exists).
    /// Used for cloning and importing where the disk is prepared externally.
    pub fn create_vm_from_existing(&self, config: &VmConfig) -> VmmResult<()> {
        // SECURITY (CWE-20): Validate VM name before any operations.
        if let Some(err) = config::validate_vm_name(&config.name) {
            return Err(VmmError::InvalidConfig(format!(
                "Invalid VM name '{}': {} (CWE-20)",
                config.name, err
            )));
        }

        info!("Defining VM from existing config: {}", config.name);

        let xml = xml_builder::build_domain_xml(config);
        debug!("Domain XML:\n{}", xml);

        let domain = Domain::define_xml(&self.conn, &xml)?;

        if config.autostart {
            if let Err(e) = domain.set_autostart(true) {
                warn!("Failed to set autostart for '{}': {}", config.name, e);
            }
        }

        // SECURITY (CWE-459): If config save fails, undefine the domain.
        if let Err(e) = config.save() {
            warn!(
                "config.save() failed, rolling back domain definition: {}",
                e
            );
            let _ = domain.undefine();
            return Err(e);
        }

        info!("VM '{}' defined successfully", config.name);
        Ok(())
    }

    /// Start a VM.
    pub fn start_vm(&self, name: &str) -> VmmResult<()> {
        let domain = self.find_domain(name)?;
        let info = domain.get_info()?;

        if info.state as u32 == sys::VIR_DOMAIN_RUNNING {
            return Err(VmmError::VmAlreadyRunning {
                name: name.to_string(),
            });
        }

        // Re-apply disk-permission fixes before every start. This is the
        // retroactive-repair path for VMs created by older builds whose
        // home-directory ACL was never set (the buggy depth-3 walk).
        // Idempotent: setfacl on an already-set ACL is a no-op.
        ensure_disk_accessible(&domain);

        domain.create()?;
        info!("VM '{}' started", name);
        Ok(())
    }

    /// Gracefully shut down a VM (sends ACPI shutdown signal).
    pub fn shutdown_vm(&self, name: &str) -> VmmResult<()> {
        let domain = self.find_domain(name)?;
        domain.shutdown()?;
        info!("Shutdown signal sent to VM '{}'", name);
        Ok(())
    }

    /// Force-stop a VM (like pulling the power cord).
    pub fn force_stop_vm(&self, name: &str) -> VmmResult<()> {
        let domain = self.find_domain(name)?;
        // Wave 11.8: enforce restriction policy (block_force_stop).
        let uuid = domain.get_uuid_string().unwrap_or_default();
        enforce_policy(&uuid, restricted::Operation::ForceStop)?;
        domain.destroy()?;
        info!("VM '{}' force-stopped", name);
        Ok(())
    }

    /// Pause (suspend) a running VM.
    pub fn pause_vm(&self, name: &str) -> VmmResult<()> {
        let domain = self.find_domain(name)?;
        domain.suspend()?;
        info!("VM '{}' paused", name);
        Ok(())
    }

    /// Resume a paused or PM-suspended VM.
    pub fn resume_vm(&self, name: &str) -> VmmResult<()> {
        // SECURITY (CWE-78): Validate name — this function may invoke virsh subprocess.
        validate_vm_name_for_command(name)?;
        let domain = self.find_domain(name)?;
        let info = domain.get_info()?;

        if info.state as u32 == sys::VIR_DOMAIN_PMSUSPENDED {
            // PM-suspended VMs need dompmwakeup, not resume
            // SECURITY: Use "--" to prevent VM name from being interpreted as a flag (CWE-88)
            // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
            let output = std::process::Command::new("virsh")
                .args(["dompmwakeup", "--", name])
                .stdin(std::process::Stdio::null())
                .output()
                .map_err(|e| VmmError::Other(format!("Failed to run virsh: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(VmmError::Other(format!(
                    "Failed to wake PM-suspended VM: {}",
                    stderr
                )));
            }
            info!("VM '{}' woken from PM suspend", name);
        } else {
            domain.resume()?;
            info!("VM '{}' resumed", name);
        }
        Ok(())
    }

    /// Reboot a VM gracefully.
    pub fn reboot_vm(&self, name: &str) -> VmmResult<()> {
        let domain = self.find_domain(name)?;
        domain.reboot(0)?;
        info!("Reboot signal sent to VM '{}'", name);
        Ok(())
    }

    /// Delete a VM and optionally its disk image.
    pub fn delete_vm(&self, name: &str, delete_disk: bool) -> VmmResult<()> {
        let domain = self.find_domain(name)?;

        // SECURITY (CWE-416): Capture UUID and disk path BEFORE undefining the domain.
        // Previously, get_uuid_string() was called AFTER undefine(), which accesses
        // a freed/invalid libvirt object. This could return garbage data or crash.
        let uuid_str = domain.get_uuid_string().unwrap_or_default();

        // Wave 11.8: enforce restriction policy (block_delete) before any
        // state changes so a locked VM can never be force-stopped + undefined.
        enforce_policy(&uuid_str, restricted::Operation::Delete)?;

        // Stop if running
        let info = domain.get_info()?;
        if info.state as u32 == sys::VIR_DOMAIN_RUNNING {
            domain.destroy()?;
        }
        let xml = domain.get_xml_desc(0).unwrap_or_default();
        let disk_path = extract_disk_path(&xml);

        // Undefine with NVRAM if UEFI
        let flags = sys::VIR_DOMAIN_UNDEFINE_NVRAM | sys::VIR_DOMAIN_UNDEFINE_SNAPSHOTS_METADATA;
        if domain.undefine_flags(flags).is_err() {
            // Fall back to simple undefine
            domain.undefine()?;
        }

        // Delete disk image
        if delete_disk {
            if let Some(path) = disk_path {
                if std::path::Path::new(&path).exists() {
                    std::fs::remove_file(&path)?;
                    info!("Deleted disk image: {}", path);
                }
            }
        }

        // Delete our config file (using UUID captured before undefine)
        if let Ok(uuid) = uuid::Uuid::parse_str(&uuid_str) {
            let _ = VmConfig::load(&uuid).map(|c| c.delete_config());
        }

        info!("VM '{}' deleted", name);
        Ok(())
    }

    /// Open the SPICE console for a running VM.
    ///
    /// SECURITY (CWE-88): VM name is passed to `virt-viewer` with `--` separator
    /// and validated via `find_domain()` to prevent argument injection. Port values
    /// are bounds-checked to prevent connecting to arbitrary services (CWE-20).
    pub fn open_console(&self, name: &str) -> VmmResult<()> {
        let domain = self.find_domain(name)?;
        let info = domain.get_info()?;

        if info.state as u32 != sys::VIR_DOMAIN_RUNNING {
            return Err(VmmError::VmNotRunning {
                name: name.to_string(),
            });
        }

        let xml = domain.get_xml_desc(0).unwrap_or_default();

        if let Some(port) = extract_spice_port(&xml) {
            // SECURITY (CWE-20): Validate port is in a sane range to prevent
            // connecting to arbitrary local services.
            if port == 0 || port < 1024 {
                return Err(VmmError::Other(format!(
                    "SPICE port {} is outside valid range (1024-65535) (CWE-20)",
                    port
                )));
            }
            info!("Opening SPICE console on port {}", port);
            // SECURITY: CWE-403 — Close stdin and don't inherit parent FDs to prevent
            // sensitive file descriptors (secret files, lock files) from leaking to
            // the long-lived console viewer process.
            std::process::Command::new("spicy")
                .args(["--host=127.0.0.1", &format!("--port={}", port)])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| VmmError::Other(format!("Failed to open console: {}", e)))?;
            Ok(())
        } else if let Some(port) = extract_vnc_port(&xml) {
            // SECURITY (CWE-20): Validate VNC display port range.
            if port > 1000 {
                return Err(VmmError::Other(format!(
                    "VNC display number {} is outside valid range (0-1000) (CWE-20)",
                    port
                )));
            }
            info!("Opening VNC console on port {}", port);
            let display_port = 5900 + port;
            // SECURITY (CWE-88): Use "--" separator before VM name to prevent
            // argument injection. A VM named "--help" or "-x" could be
            // interpreted as a flag by virt-viewer without the separator.
            // SECURITY: CWE-403 — Close stdin/stdout/stderr to prevent FD leaks
            // to long-lived viewer processes.
            std::process::Command::new("virt-viewer")
                .args(["--connect", "qemu:///system", "--", name])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .or_else(|_| {
                    std::process::Command::new("remote-viewer")
                        .arg(format!("vnc://127.0.0.1:{}", display_port))
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                })
                .map_err(|e| VmmError::Other(format!("Failed to open console: {}", e)))?;
            Ok(())
        } else {
            Err(VmmError::Other(
                "No display (SPICE/VNC) found for this VM".into(),
            ))
        }
    }

    /// Get the SPICE connection URI for a running VM.
    pub fn get_spice_uri(&self, name: &str) -> VmmResult<Option<String>> {
        let domain = self.find_domain(name)?;
        let xml = domain.get_xml_desc(0).unwrap_or_default();

        if let Some(port) = extract_spice_port(&xml) {
            Ok(Some(format!("spice://127.0.0.1:{}", port)))
        } else {
            Ok(None)
        }
    }

    /// Get the SPICE TCP port for a running VM (for embedded console).
    pub fn get_spice_port(&self, name: &str) -> VmmResult<Option<u16>> {
        let domain = self.find_domain(name)?;
        let xml = domain.get_xml_desc(0).unwrap_or_default();
        Ok(extract_spice_port(&xml))
    }

    /// Get the VNC port for a running VM (for embedded console).
    pub fn get_vnc_port(&self, name: &str) -> VmmResult<Option<u16>> {
        let domain = self.find_domain(name)?;
        let info = domain.get_info()?;

        if info.state as u32 != sys::VIR_DOMAIN_RUNNING {
            return Ok(None);
        }

        let xml = domain.get_xml_desc(0).unwrap_or_default();
        Ok(extract_vnc_port(&xml))
    }

    /// Suspend a running VM to disk (managed save).
    /// Saves RAM state to disk and shuts the VM down. When started again,
    /// the VM resumes from the saved state automatically.
    pub fn suspend_to_disk(&self, name: &str) -> VmmResult<()> {
        // SECURITY (CWE-78): Validate name before passing to virsh subprocess.
        // find_domain() rejects names starting with '-' and empty names, but
        // additional validation prevents shell metacharacters from being passed
        // to the virsh command even with "--" separator.
        validate_vm_name_for_command(name)?;
        info!("Suspending VM '{}' to disk (managed save)", name);
        let domain = self.find_domain(name)?;
        let info = domain.get_info()?;

        if info.state as u32 != sys::VIR_DOMAIN_RUNNING {
            return Err(VmmError::VmNotRunning {
                name: name.to_string(),
            });
        }

        // Use virsh for managed save since the virt crate API may vary
        // SECURITY: Use "--" to prevent VM name from being interpreted as a flag (CWE-88)
        // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
        let output = std::process::Command::new("virsh")
            .args(["managedsave", "--", name])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| VmmError::Other(format!("Failed to run virsh: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VmmError::Other(format!("Managed save failed: {}", stderr)));
        }

        info!("VM '{}' suspended to disk", name);
        Ok(())
    }

    /// Change the CD/DVD media of a running VM.
    /// Uses `virsh change-media` to insert a new ISO into the VM's CDROM drive.
    /// CWE-88: Uses `--` to separate options from VM name argument.
    /// CWE-22: Validates ISO path.
    pub fn change_media(&self, vm_name: &str, iso_path: &str) -> VmmResult<()> {
        // SECURITY (CWE-78): Validate VM name before passing to virsh subprocess.
        validate_vm_name_for_command(vm_name)?;

        // SECURITY (CWE-22): Validate ISO path to prevent path traversal and injection.
        if iso_path.is_empty() {
            return Err(VmmError::InvalidConfig(
                "ISO path must not be empty (CWE-22)".to_string(),
            ));
        }
        if iso_path.contains('\0') {
            return Err(VmmError::InvalidConfig(
                "ISO path must not contain null bytes (CWE-626)".to_string(),
            ));
        }
        if iso_path.contains("..") {
            return Err(VmmError::InvalidConfig(
                "ISO path must not contain '..' (CWE-22)".to_string(),
            ));
        }
        if !(iso_path.ends_with(".iso") || iso_path.ends_with(".img")) {
            return Err(VmmError::InvalidConfig(
                "ISO path must end with .iso or .img (CWE-22)".to_string(),
            ));
        }

        // Auto-detect the first CDROM target device from domain XML
        let cdrom_target = self.find_cdrom_target(vm_name)?;

        info!(
            "Changing CD/DVD media for VM '{}' target '{}' to '{}'",
            vm_name, cdrom_target, iso_path
        );

        // SECURITY (CWE-88): Use named options (--domain, --path, --source) to prevent
        // VM name or ISO path from being interpreted as flags. virsh doesn't support "--".
        // SECURITY (CWE-403): Close stdin to prevent FD inheritance.
        let output = std::process::Command::new("virsh")
            .args([
                "change-media",
                "--domain",
                vm_name,
                "--path",
                &cdrom_target,
                "--source",
                iso_path,
                "--update",
            ])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| VmmError::Other(format!("Failed to run virsh: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VmmError::Other(format!(
                "Failed to change media: {}",
                stderr
            )));
        }

        info!("CD/DVD media changed for VM '{}'", vm_name);
        Ok(())
    }

    /// Eject CD/DVD media from a running VM.
    pub fn eject_media(&self, vm_name: &str) -> VmmResult<()> {
        // SECURITY (CWE-78): Validate VM name before passing to virsh subprocess.
        validate_vm_name_for_command(vm_name)?;

        // Auto-detect the first CDROM target device from domain XML
        let cdrom_target = self.find_cdrom_target(vm_name)?;

        info!(
            "Ejecting CD/DVD media from VM '{}' target '{}'",
            vm_name, cdrom_target
        );

        // SECURITY (CWE-88): Use named options (--domain, --path) to prevent
        // VM name from being interpreted as a flag. virsh doesn't support "--".
        // SECURITY (CWE-403): Close stdin to prevent FD inheritance.
        let output = std::process::Command::new("virsh")
            .args([
                "change-media",
                "--domain",
                vm_name,
                "--path",
                &cdrom_target,
                "--eject",
            ])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| VmmError::Other(format!("Failed to run virsh: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VmmError::Other(format!(
                "Failed to eject media: {}",
                stderr
            )));
        }

        info!("CD/DVD media ejected from VM '{}'", vm_name);
        Ok(())
    }

    /// Find the first CDROM target device name from domain XML.
    /// Returns e.g. "sda", "sdb", "sdc" depending on VM configuration.
    fn find_cdrom_target(&self, vm_name: &str) -> VmmResult<String> {
        let domain = self.find_domain(vm_name)?;
        let xml = domain.get_xml_desc(0).unwrap_or_default();
        Ok(parse_cdrom_target(&xml).unwrap_or_else(|| "sda".to_string()))
    }

    /// Get the currently inserted CD/DVD media path, if any.
    /// Parses the domain XML for `<disk device='cdrom'>...<source file='...'/>`.
    pub fn get_cdrom_media(&self, vm_name: &str) -> VmmResult<Option<String>> {
        let domain = self.find_domain(vm_name)?;
        let xml = domain.get_xml_desc(0).unwrap_or_default();
        Ok(parse_cdrom_media(&xml))
    }

    /// Set the VM to boot into UEFI firmware setup on next start.
    /// Enables the boot menu with a timeout so the user can enter setup.
    pub fn boot_to_firmware(&self, vm_name: &str) -> VmmResult<()> {
        // SECURITY (CWE-78): Validate VM name before passing to virsh subprocess.
        validate_vm_name_for_command(vm_name)?;

        info!("Setting boot-to-firmware for VM '{}'", vm_name);

        // Get the current domain XML
        // SECURITY (CWE-88): Use "--" to prevent VM name from being interpreted as a flag.
        // SECURITY (CWE-403): Close stdin to prevent FD inheritance.
        let output = std::process::Command::new("virsh")
            .args(["dumpxml", "--", vm_name])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| VmmError::Other(format!("Failed to run virsh dumpxml: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VmmError::Other(format!(
                "Failed to dump VM XML: {}",
                stderr
            )));
        }

        let xml = String::from_utf8_lossy(&output.stdout).to_string();

        // Check that this is a UEFI VM
        if !xml.contains("firmware") && !xml.contains("loader") {
            return Err(VmmError::InvalidConfig(
                "VM does not appear to use UEFI firmware; boot-to-firmware requires UEFI"
                    .to_string(),
            ));
        }

        // Modify the XML to enable boot menu with timeout
        let modified_xml = if let Some(_) = xml.find("<bootmenu") {
            // Replace existing <bootmenu .../> with our version
            let re_start = xml.find("<bootmenu").unwrap();
            let after = &xml[re_start..];
            let re_end = after
                .find("/>")
                .map(|i| re_start + i + 2)
                .or_else(|| {
                    after
                        .find("</bootmenu>")
                        .map(|i| re_start + i + "</bootmenu>".len())
                })
                .unwrap_or(re_start);
            format!(
                "{}<bootmenu enable='yes' timeout='5000'/>{}",
                &xml[..re_start],
                &xml[re_end..]
            )
        } else if let Some(os_end) = xml.find("</os>") {
            // Insert bootmenu before </os>
            format!(
                "{}    <bootmenu enable='yes' timeout='5000'/>\n  {}",
                &xml[..os_end],
                &xml[os_end..]
            )
        } else {
            return Err(VmmError::Other(
                "Could not find <os> section in VM XML to insert boot menu".to_string(),
            ));
        };

        // SECURITY: CWE-377 — Write XML to /dev/shm (ramdisk) instead of /tmp (disk-backed,
        // world-writable). Set restrictive permissions to prevent other users from reading.
        let temp_dir = std::path::PathBuf::from("/dev/shm");
        let temp_path = temp_dir.join(format!(".libre-vmm-boot-fw-{}.xml", std::process::id()));
        {
            use std::io::Write;
            #[cfg(unix)]
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp_path)
                .map_err(|e| VmmError::Other(format!("Failed to create temp XML file: {}", e)))?;
            f.write_all(modified_xml.as_bytes())
                .map_err(|e| VmmError::Other(format!("Failed to write temp XML file: {}", e)))?;
        }

        // Redefine the domain with the modified XML
        // SECURITY (CWE-88): Use "--" to separate options from the file path argument.
        // SECURITY (CWE-403): Close stdin to prevent FD inheritance.
        let define_output = std::process::Command::new("virsh")
            .args(["define", "--", &temp_path.to_string_lossy()])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| VmmError::Other(format!("Failed to run virsh define: {}", e)));

        // Clean up temp file regardless of outcome
        let _ = std::fs::remove_file(&temp_path);

        let define_output = define_output?;
        if !define_output.status.success() {
            let stderr = String::from_utf8_lossy(&define_output.stderr);
            return Err(VmmError::Other(format!(
                "Failed to redefine VM with boot menu: {}",
                stderr
            )));
        }

        info!(
            "Boot-to-firmware enabled for VM '{}' (boot menu with 5s timeout)",
            vm_name
        );
        Ok(())
    }

    /// Check if a VM has a managed save image (suspended to disk).
    pub fn has_managed_save(&self, name: &str) -> bool {
        // SECURITY (CWE-78): Full name validation before passing to virsh.
        if validate_vm_name_for_command(name).is_err() {
            return false;
        }
        // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
        let output = std::process::Command::new("virsh")
            .args(["dominfo", "--", name])
            .stdin(std::process::Stdio::null())
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if line.starts_with("Managed save:") {
                        return line.contains("yes");
                    }
                }
            }
        }
        false
    }

    /// Discard a managed save image (don't restore, start fresh).
    pub fn discard_managed_save(&self, name: &str) -> VmmResult<()> {
        // SECURITY (CWE-78): Full name validation before passing to virsh subprocess.
        validate_vm_name_for_command(name)?;
        info!("Discarding managed save for VM '{}'", name);
        // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
        let output = std::process::Command::new("virsh")
            .args(["managedsave-remove", "--", name])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| VmmError::Other(format!("Failed to run virsh: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Ignore "no managed save image" errors
            if !stderr.contains("has no managed save") && !stderr.contains("not found") {
                return Err(VmmError::Other(format!(
                    "Failed to discard managed save: {}",
                    stderr
                )));
            }
        }

        info!("Managed save discarded for VM '{}'", name);
        Ok(())
    }

    /// Expose the raw libvirt connection for use by snapshot/network modules.
    pub fn raw_conn(&self) -> &Connect {
        &self.conn
    }

    /// Update an existing VM's configuration.
    /// Undefines the old domain and redefines with new XML from the updated config.
    /// VM must be powered off.
    ///
    /// SECURITY: Validates VM name and provides rollback if redefine fails (CWE-459).
    pub fn update_vm(&self, config: &VmConfig) -> VmmResult<()> {
        // SECURITY (CWE-20): Validate the new VM name.
        if let Some(err) = config::validate_vm_name(&config.name) {
            return Err(VmmError::InvalidConfig(format!(
                "Invalid VM name '{}': {} (CWE-20)",
                config.name, err
            )));
        }

        info!("Updating VM: {}", config.name);

        let domain = self.find_domain(&config.name)?;

        // Wave 11.8: enforce restriction policy (read_only_config) before
        // touching the domain definition.
        let uuid_str = domain.get_uuid_string().unwrap_or_default();
        enforce_policy(&uuid_str, restricted::Operation::ModifyConfig)?;

        let info = domain.get_info()?;

        if info.state as u32 == sys::VIR_DOMAIN_RUNNING {
            return Err(VmmError::VmAlreadyRunning {
                name: config.name.clone(),
            });
        }

        // SECURITY (CWE-459): Capture old XML before undefining so we can roll back
        // if redefine fails. Without this, a failed update leaves the VM in an
        // undefined (deleted) state — data loss.
        let old_xml = domain.get_xml_desc(0).unwrap_or_default();

        // Undefine with NVRAM flag if UEFI
        let flags = sys::VIR_DOMAIN_UNDEFINE_NVRAM;
        if domain.undefine_flags(flags).is_err() {
            let _ = domain.undefine();
        }

        // Rebuild XML from updated config and redefine
        let xml = xml_builder::build_domain_xml(config);
        debug!("Updated domain XML:\n{}", xml);

        // SECURITY (CWE-459): If redefine fails, restore the old definition.
        let domain = match Domain::define_xml(&self.conn, &xml) {
            Ok(d) => d,
            Err(e) => {
                warn!("Redefine failed, attempting rollback to old XML: {}", e);
                if !old_xml.is_empty() {
                    let _ = Domain::define_xml(&self.conn, &old_xml);
                }
                return Err(e.into());
            },
        };

        // Set autostart
        if let Err(e) = domain.set_autostart(config.autostart) {
            warn!("Failed to set autostart for '{}': {}", config.name, e);
        }

        // Save our config file
        config.save()?;

        info!("VM '{}' settings updated", config.name);
        Ok(())
    }

    /// Set autostart flag on a domain (safe to call while running).
    pub fn set_autostart(&self, name: &str, enabled: bool) -> VmmResult<()> {
        let domain = self.find_domain(name)?;
        domain
            .set_autostart(enabled)
            .map_err(|e| VmmError::Other(format!("autostart: {}", e)))?;
        Ok(())
    }

    fn find_domain(&self, name: &str) -> VmmResult<Domain> {
        // SECURITY (CWE-88, CWE-78): Full validation of VM name before passing
        // to libvirt. Names flow to virsh subprocesses and XML — must be safe.
        validate_vm_name_for_command(name)?;
        Domain::lookup_by_name(&self.conn, name).map_err(|_| VmmError::VmNotFound {
            name: name.to_string(),
        })
    }
}

impl Drop for HypervisorConnection {
    fn drop(&mut self) {
        let _ = self.conn.close();
    }
}

// TODO (Wave 11.8 follow-up): wire enforce_policy() into the snapshot,
// clone, USB, shared-folder, and network modules. Each of those crates
// is independent today; touching them in a single batch would balloon
// this change. The policy data model already covers those operations
// (TakeSnapshot, RevertSnapshot, Clone, Export, AttachUsb,
// AddSharedFolder, ChangeNetwork) — only the call sites are missing.

/// Enforce a VM's restriction policy (if any) for the given operation.
///
/// `vm_uuid` is the libvirt domain UUID string. If no policy file exists for
/// the VM, this is a no-op. If a policy exists and forbids `op` (or is
/// expired), returns `VmmError::InvalidConfig` with the policy's reason.
///
/// SECURITY MODEL: This is cooperative intent enforcement (see restricted.rs
/// module docs). A user with filesystem access can edit/delete the policy
/// file; combine with disk encryption + OS-level ACLs for real isolation.
fn enforce_policy(vm_uuid: &str, op: restricted::Operation) -> VmmResult<()> {
    // Tolerate empty/invalid UUID — treat as unrestricted rather than break
    // existing flows (a missing policy is the same as no restriction).
    let uuid = match uuid::Uuid::parse_str(vm_uuid) {
        Ok(u) => u,
        Err(_) => return Ok(()),
    };
    match RestrictionPolicy::load(&uuid)? {
        Some(policy) => restricted::check_or_err(&policy, op),
        None => Ok(()),
    }
}

/// SECURITY (CWE-78, CWE-88): Validate a VM name before passing it to any
/// subprocess (virsh, virt-viewer, etc.). This is defense-in-depth beyond
/// `find_domain()` which only checks for '-' prefix and emptiness.
/// Rejects names containing shell metacharacters, control characters,
/// or null bytes that could be exploited even with "--" argument separators.
fn validate_vm_name_for_command(name: &str) -> VmmResult<()> {
    if name.is_empty() {
        return Err(VmmError::InvalidConfig(
            "VM name must not be empty (CWE-78)".to_string(),
        ));
    }
    if name.len() > 128 {
        return Err(VmmError::InvalidConfig(
            "VM name too long for command use (max 128 chars) (CWE-78)".to_string(),
        ));
    }
    // SECURITY (SVE #11): Strict allowlist — alphanumeric, space, hyphen, underscore, dot only.
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || " -_.".contains(c))
    {
        return Err(VmmError::InvalidConfig(format!(
            "VM name '{}' contains unsafe characters for subprocess use (CWE-78)",
            name
        )));
    }
    if name.starts_with('-') || name.starts_with('.') {
        return Err(VmmError::InvalidConfig(format!(
            "VM name '{}' must not start with '-' or '.' (CWE-88)",
            name
        )));
    }
    // Block null bytes (CWE-626)
    if name.contains('\0') {
        return Err(VmmError::InvalidConfig(
            "VM name must not contain null bytes (CWE-626)".to_string(),
        ));
    }
    Ok(())
}

/// Extract the disk image path from a `<disk type='file' device='disk'>` element.
///
/// Uses quick-xml to properly parse the XML instead of fragile string slicing.
/// SECURITY (SVE #20): Proper XML parsing prevents reading past element boundaries.
fn extract_disk_path(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut inside_file_disk = false;
    let mut depth: u32 = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name();
                if local.as_ref() == b"disk" {
                    let mut is_file = false;
                    let mut is_disk = false;
                    for attr in e.attributes().flatten() {
                        match attr.key.local_name().as_ref() {
                            b"type" if attr.value.as_ref() == b"file" => is_file = true,
                            b"device" if attr.value.as_ref() == b"disk" => is_disk = true,
                            _ => {},
                        }
                    }
                    if is_file && is_disk && !inside_file_disk {
                        inside_file_disk = true;
                        depth = 1;
                    }
                } else if inside_file_disk {
                    depth += 1;
                }
            },
            Ok(Event::Empty(ref e)) => {
                if inside_file_disk && e.local_name().as_ref() == b"source" {
                    for attr in e.attributes().flatten() {
                        if attr.key.local_name().as_ref() == b"file" {
                            let val = String::from_utf8_lossy(&attr.value).to_string();
                            if !val.is_empty() {
                                return Some(val);
                            }
                        }
                    }
                }
            },
            Ok(Event::End(ref e)) => {
                if inside_file_disk {
                    if e.local_name().as_ref() == b"disk" && depth == 1 {
                        // First file-type disk had no source — give up
                        return None;
                    }
                    depth = depth.saturating_sub(1);
                }
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {},
        }
        buf.clear();
    }
    None
}

/// Extract a graphics port from the domain XML for the given display type.
///
/// Uses quick-xml to properly parse the XML instead of fragile string slicing.
/// SECURITY (SVE #20): Proper XML parsing prevents reading past element boundaries.
fn extract_graphics_port(xml: &str, display_type: &[u8]) -> Option<u16> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                if e.local_name().as_ref() == b"graphics" {
                    let mut is_target_type = false;
                    let mut port_value: Option<u16> = None;
                    for attr in e.attributes().flatten() {
                        match attr.key.local_name().as_ref() {
                            b"type" if attr.value.as_ref() == display_type => {
                                is_target_type = true;
                            },
                            b"port" => {
                                let val = String::from_utf8_lossy(&attr.value);
                                port_value = val.parse::<u16>().ok();
                            },
                            _ => {},
                        }
                    }
                    if is_target_type {
                        return port_value;
                    }
                }
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {},
        }
        buf.clear();
    }
    None
}

/// SECURITY (SVE #20): Bound search within the <graphics> element to prevent
/// reading past the tag boundary. Return None if closing delimiter not found.
fn extract_spice_port(xml: &str) -> Option<u16> {
    extract_graphics_port(xml, b"spice")
}

/// SECURITY (SVE #20): Bound search within the <graphics> element to prevent
/// reading past the tag boundary. Return None if closing delimiter not found.
fn extract_vnc_port(xml: &str) -> Option<u16> {
    extract_graphics_port(xml, b"vnc")
}

/// Parse the first CDROM target device name from domain XML.
///
/// Extracted from `HypervisorConnection::find_cdrom_target` so the XML parsing
/// logic can be unit-tested without a libvirt connection.
fn parse_cdrom_target(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut inside_cdrom = false;
    let mut depth: u32 = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name();
                if local.as_ref() == b"disk" {
                    let is_cdrom = e.attributes().flatten().any(|a| {
                        a.key.local_name().as_ref() == b"device" && a.value.as_ref() == b"cdrom"
                    });
                    if is_cdrom && !inside_cdrom {
                        inside_cdrom = true;
                        depth = 1;
                    }
                } else if inside_cdrom {
                    depth += 1;
                }
            },
            Ok(Event::Empty(ref e)) => {
                if inside_cdrom && e.local_name().as_ref() == b"target" {
                    for attr in e.attributes().flatten() {
                        if attr.key.local_name().as_ref() == b"dev" {
                            let val = String::from_utf8_lossy(&attr.value).to_string();
                            if !val.is_empty() {
                                return Some(val);
                            }
                        }
                    }
                }
            },
            Ok(Event::End(ref e)) => {
                if inside_cdrom {
                    if e.local_name().as_ref() == b"disk" && depth == 1 {
                        return None;
                    }
                    depth = depth.saturating_sub(1);
                }
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {},
        }
        buf.clear();
    }
    None
}

/// Parse the CDROM source file path from domain XML.
///
/// Extracted from `HypervisorConnection::get_cdrom_media` so the XML parsing
/// logic can be unit-tested without a libvirt connection.
fn parse_cdrom_media(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut inside_cdrom = false;
    let mut depth: u32 = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name();
                if local.as_ref() == b"disk" {
                    let is_cdrom = e.attributes().flatten().any(|a| {
                        a.key.local_name().as_ref() == b"device" && a.value.as_ref() == b"cdrom"
                    });
                    if is_cdrom && !inside_cdrom {
                        inside_cdrom = true;
                        depth = 1;
                    }
                } else if inside_cdrom {
                    depth += 1;
                }
            },
            Ok(Event::Empty(ref e)) => {
                if inside_cdrom && e.local_name().as_ref() == b"source" {
                    for attr in e.attributes().flatten() {
                        if attr.key.local_name().as_ref() == b"file" {
                            let val = String::from_utf8_lossy(&attr.value).to_string();
                            if !val.is_empty() {
                                return Some(val);
                            }
                        }
                    }
                }
            },
            Ok(Event::End(ref e)) => {
                if inside_cdrom {
                    if e.local_name().as_ref() == b"disk" && depth == 1 {
                        // First cdrom had no source — no media inserted
                        return None;
                    }
                    depth = depth.saturating_sub(1);
                }
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {},
        }
        buf.clear();
    }
    None
}

/// Fix file permissions so libvirt's qemu user can access the disk image.
///
/// This addresses a Linux-libvirt pain point: when libvirt runs QEMU as the
/// dedicated `libvirt-qemu` system user, that user cannot reach a disk file in
/// `$HOME` because the home directory itself is typically mode 0700 or 0750.
///
/// We use POSIX ACLs (not group-traversal bits) so we can grant access
/// surgically to `libvirt-qemu` without making the home directory readable to
/// other users on the system.
///
/// Behaviour:
/// - The disk file itself gets mode 0664 and `u:libvirt-qemu:rw` ACL.
/// - Each parent directory between the disk and the user's `$HOME` (inclusive)
///   gets `u:libvirt-qemu:x` ACL, so libvirt-qemu can traverse to the file.
/// - We never touch `/home`, `/`, or anything above `$HOME`.
/// - If the disk is outside `$HOME` (e.g., `/var/lib/libvirt/images/...`), we
///   only fix the file's own ACL and skip the walk entirely — libvirt has
///   natural access to its own data paths.
/// - Safety cap of 20 iterations on the walk.
///
/// Idempotent: re-applying the same ACL is a no-op, so this is safe to call
/// before each VM start to retroactively repair VMs created by older builds.
fn fix_disk_permissions(disk_path: &str) {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    // SECURITY: Use 0o664 (owner+group rw, other read) instead of 0o666.
    if let Ok(metadata) = std::fs::metadata(disk_path) {
        let mut perms = metadata.permissions();
        perms.set_mode(0o664);
        let _ = std::fs::set_permissions(disk_path, perms);
    }

    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
    let _ = std::process::Command::new("setfacl")
        .args(["-m", "u:libvirt-qemu:rw", disk_path])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();

    // Determine the upper bound for the directory walk: the user's $HOME.
    // If the disk is outside $HOME, libvirt-qemu has access by default; skip.
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let disk_pb = PathBuf::from(disk_path);
    if !disk_pb.starts_with(&home_dir) {
        return;
    }

    let mut path: &Path = disk_pb.as_path();
    for _ in 0..20 {
        let parent = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => break,
        };

        if let Ok(metadata) = std::fs::metadata(parent) {
            let mode = metadata.permissions().mode();
            if mode & 0o011 == 0 {
                // Not group/other-executable — add g+x only.
                let mut perms = metadata.permissions();
                perms.set_mode(mode | 0o010);
                let _ = std::fs::set_permissions(parent, perms);
            }
        }
        // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
        let _ = std::process::Command::new("setfacl")
            .args(["-m", "u:libvirt-qemu:x", &parent.to_string_lossy()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();

        // Stop after processing the home directory itself; never walk above it.
        if parent == home_dir.as_path() {
            break;
        }
        path = parent;
    }
}

/// Ensure the disk image for the given libvirt domain is accessible by
/// `libvirt-qemu` before starting. Idempotent — safe to call repeatedly.
///
/// This catches the common case where a VM was created by an older build
/// (with the buggy 3-level walk depth in `fix_disk_permissions`) and now has
/// a disk under `$HOME` that libvirt cannot reach. By re-applying the fix
/// before every start, existing VMs auto-repair on first launch with this
/// build.
fn ensure_disk_accessible(domain: &Domain) {
    let xml = match domain.get_xml_desc(0) {
        Ok(x) => x,
        Err(_) => return,
    };
    if let Some(disk_path) = extract_disk_path(&xml) {
        fix_disk_permissions(&disk_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // extract_disk_path
    // -----------------------------------------------------------------------

    #[test]
    fn disk_path_valid() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <disk type='file' device='disk'>
              <driver name='qemu' type='qcow2'/>
              <source file='/var/lib/libvirt/images/test.qcow2'/>
              <target dev='vda' bus='virtio'/>
            </disk>
          </devices>
        </domain>"#;
        assert_eq!(
            extract_disk_path(xml),
            Some("/var/lib/libvirt/images/test.qcow2".to_string())
        );
    }

    #[test]
    fn disk_path_no_disk_element() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <interface type='network'/>
          </devices>
        </domain>"#;
        assert_eq!(extract_disk_path(xml), None);
    }

    #[test]
    fn disk_path_missing_source() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <disk type='file' device='disk'>
              <driver name='qemu' type='qcow2'/>
              <target dev='vda' bus='virtio'/>
            </disk>
          </devices>
        </domain>"#;
        assert_eq!(extract_disk_path(xml), None);
    }

    #[test]
    fn disk_path_multiple_disks_gets_first_file_type() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <disk type='block' device='disk'>
              <source dev='/dev/sda1'/>
              <target dev='vda' bus='virtio'/>
            </disk>
            <disk type='file' device='cdrom'>
              <source file='/iso/boot.iso'/>
              <target dev='sda' bus='sata'/>
            </disk>
            <disk type='file' device='disk'>
              <source file='/images/second.qcow2'/>
              <target dev='vdb' bus='virtio'/>
            </disk>
            <disk type='file' device='disk'>
              <source file='/images/third.qcow2'/>
              <target dev='vdc' bus='virtio'/>
            </disk>
          </devices>
        </domain>"#;
        // Should skip the block disk and the cdrom, return the first file+disk
        assert_eq!(
            extract_disk_path(xml),
            Some("/images/second.qcow2".to_string())
        );
    }

    #[test]
    fn disk_path_double_quoted_attributes() {
        // libvirt can emit double-quoted attributes
        let xml = r#"
        <domain type="kvm">
          <devices>
            <disk type="file" device="disk">
              <source file="/home/user/vm.qcow2"/>
              <target dev="vda" bus="virtio"/>
            </disk>
          </devices>
        </domain>"#;
        assert_eq!(
            extract_disk_path(xml),
            Some("/home/user/vm.qcow2".to_string())
        );
    }

    // -----------------------------------------------------------------------
    // extract_spice_port
    // -----------------------------------------------------------------------

    #[test]
    fn spice_port_valid() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <graphics type='spice' port='5900' autoport='yes' listen='127.0.0.1'/>
          </devices>
        </domain>"#;
        assert_eq!(extract_spice_port(xml), Some(5900));
    }

    #[test]
    fn spice_port_missing_graphics() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <disk type='file' device='disk'/>
          </devices>
        </domain>"#;
        assert_eq!(extract_spice_port(xml), None);
    }

    #[test]
    fn spice_port_negative_one() {
        // port=-1 means autoport is active and VM is not running yet
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <graphics type='spice' port='-1' autoport='yes'/>
          </devices>
        </domain>"#;
        // -1 doesn't parse as u16, so should be None
        assert_eq!(extract_spice_port(xml), None);
    }

    #[test]
    fn spice_port_no_spice_graphics() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <graphics type='vnc' port='5901'/>
          </devices>
        </domain>"#;
        assert_eq!(extract_spice_port(xml), None);
    }

    #[test]
    fn spice_port_with_children() {
        // <graphics> can have child <listen> elements (non-empty tag)
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <graphics type='spice' port='5910' autoport='yes'>
              <listen type='address' address='127.0.0.1'/>
            </graphics>
          </devices>
        </domain>"#;
        assert_eq!(extract_spice_port(xml), Some(5910));
    }

    // -----------------------------------------------------------------------
    // extract_vnc_port
    // -----------------------------------------------------------------------

    #[test]
    fn vnc_port_valid() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <graphics type='vnc' port='0' autoport='yes'/>
          </devices>
        </domain>"#;
        assert_eq!(extract_vnc_port(xml), Some(0));
    }

    #[test]
    fn vnc_port_missing() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <graphics type='spice' port='5900'/>
          </devices>
        </domain>"#;
        assert_eq!(extract_vnc_port(xml), None);
    }

    #[test]
    fn vnc_port_negative_one() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <graphics type='vnc' port='-1' autoport='yes'/>
          </devices>
        </domain>"#;
        assert_eq!(extract_vnc_port(xml), None);
    }

    #[test]
    fn vnc_port_both_graphics_types() {
        // Make sure VNC picks VNC, not SPICE
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <graphics type='spice' port='5900'/>
            <graphics type='vnc' port='42'/>
          </devices>
        </domain>"#;
        assert_eq!(extract_vnc_port(xml), Some(42));
        assert_eq!(extract_spice_port(xml), Some(5900));
    }

    // -----------------------------------------------------------------------
    // parse_cdrom_target
    // -----------------------------------------------------------------------

    #[test]
    fn cdrom_target_valid() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <disk type='file' device='cdrom'>
              <driver name='qemu' type='raw'/>
              <source file='/iso/ubuntu.iso'/>
              <target dev='sda' bus='sata'/>
              <readonly/>
            </disk>
          </devices>
        </domain>"#;
        assert_eq!(parse_cdrom_target(xml), Some("sda".to_string()));
    }

    #[test]
    fn cdrom_target_no_cdrom() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <disk type='file' device='disk'>
              <target dev='vda' bus='virtio'/>
            </disk>
          </devices>
        </domain>"#;
        assert_eq!(parse_cdrom_target(xml), None);
    }

    #[test]
    fn cdrom_target_multiple_disks() {
        // Should return the first cdrom's target, ignoring hard disks
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <disk type='file' device='disk'>
              <target dev='vda' bus='virtio'/>
            </disk>
            <disk type='file' device='cdrom'>
              <target dev='sdb' bus='sata'/>
            </disk>
            <disk type='file' device='cdrom'>
              <target dev='sdc' bus='sata'/>
            </disk>
          </devices>
        </domain>"#;
        assert_eq!(parse_cdrom_target(xml), Some("sdb".to_string()));
    }

    // -----------------------------------------------------------------------
    // parse_cdrom_media
    // -----------------------------------------------------------------------

    #[test]
    fn cdrom_media_valid() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <disk type='file' device='cdrom'>
              <driver name='qemu' type='raw'/>
              <source file='/iso/ubuntu-22.04.iso'/>
              <target dev='sda' bus='sata'/>
              <readonly/>
            </disk>
          </devices>
        </domain>"#;
        assert_eq!(
            parse_cdrom_media(xml),
            Some("/iso/ubuntu-22.04.iso".to_string())
        );
    }

    #[test]
    fn cdrom_media_no_source() {
        // Empty CDROM drive (no media inserted)
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <disk type='file' device='cdrom'>
              <driver name='qemu' type='raw'/>
              <target dev='sda' bus='sata'/>
              <readonly/>
            </disk>
          </devices>
        </domain>"#;
        assert_eq!(parse_cdrom_media(xml), None);
    }

    #[test]
    fn cdrom_media_no_cdrom() {
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <disk type='file' device='disk'>
              <source file='/images/root.qcow2'/>
              <target dev='vda' bus='virtio'/>
            </disk>
          </devices>
        </domain>"#;
        assert_eq!(parse_cdrom_media(xml), None);
    }

    #[test]
    fn cdrom_media_skips_disk_source() {
        // The <source> inside a hard disk should NOT be picked up
        let xml = r#"
        <domain type='kvm'>
          <devices>
            <disk type='file' device='disk'>
              <source file='/images/root.qcow2'/>
              <target dev='vda' bus='virtio'/>
            </disk>
            <disk type='file' device='cdrom'>
              <source file='/iso/archlinux.iso'/>
              <target dev='sda' bus='sata'/>
            </disk>
          </devices>
        </domain>"#;
        assert_eq!(
            parse_cdrom_media(xml),
            Some("/iso/archlinux.iso".to_string())
        );
    }

    // -----------------------------------------------------------------------
    // Edge cases: empty / malformed XML
    // -----------------------------------------------------------------------

    #[test]
    fn empty_xml_returns_none() {
        assert_eq!(extract_disk_path(""), None);
        assert_eq!(extract_spice_port(""), None);
        assert_eq!(extract_vnc_port(""), None);
        assert_eq!(parse_cdrom_target(""), None);
        assert_eq!(parse_cdrom_media(""), None);
    }

    #[test]
    fn malformed_xml_returns_none() {
        let xml = "<domain><broken><not closed";
        assert_eq!(extract_disk_path(xml), None);
        assert_eq!(extract_spice_port(xml), None);
        assert_eq!(extract_vnc_port(xml), None);
        assert_eq!(parse_cdrom_target(xml), None);
        assert_eq!(parse_cdrom_media(xml), None);
    }
}
