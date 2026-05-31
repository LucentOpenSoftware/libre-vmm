//! VM Template Library — user-created templates for quick VM provisioning.
//!
//! Templates are stored as JSON files in `~/.local/share/libre-vmm/templates/`.
//! A template captures all VM settings (CPU, RAM, disk, network, etc.) without
//! the disk image or UUID, making it easy to create multiple VMs with the same config.

use crate::config::{
    BootDevice, DisplayProtocol, NetworkMode, NicConfig, OsType, VmConfig, VmConfigIo,
};
use crate::error::{VmmError, VmmResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A user-created VM template.
/// Contains everything needed to create a new VM except the disk image and UUID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmTemplate {
    /// Unique template identifier
    pub id: Uuid,
    /// Template name (e.g. "My Dev Server")
    pub name: String,
    /// Template description
    pub description: String,
    /// When this template was created
    pub created_at: String,

    // ===== Hardware settings =====
    pub vcpus: u32,
    pub memory_mib: u64,
    pub disk_size_gib: u64,
    pub os_type: OsType,
    pub uefi: bool,
    pub gpu_accel: bool,
    pub network: NetworkMode,
    pub display_protocol: DisplayProtocol,
    pub usb_support: bool,
    pub audio: bool,

    // ===== Advanced =====
    #[serde(default)]
    pub boot_order: Vec<BootDevice>,
    #[serde(default)]
    pub network_interfaces: Vec<NicConfig>,
    #[serde(default = "default_display_count")]
    pub display_count: u8,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_display_count() -> u8 {
    1
}

impl VmTemplate {
    /// Create a template from an existing VmConfig.
    pub fn from_config(config: &VmConfig, template_name: &str, description: &str) -> Self {
        let now = chrono::Local::now();
        Self {
            id: Uuid::new_v4(),
            name: template_name.to_string(),
            description: description.to_string(),
            created_at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            vcpus: config.vcpus,
            memory_mib: config.memory_mib,
            disk_size_gib: config.disk_size_gib,
            os_type: config.os_type.clone(),
            uefi: config.uefi,
            gpu_accel: config.gpu_accel,
            network: config.network.clone(),
            display_protocol: config.display_protocol,
            usb_support: config.usb_support,
            audio: config.audio,
            boot_order: config.boot_order.clone(),
            network_interfaces: config.network_interfaces.clone(),
            display_count: config.display_count,
            tags: config.tags.clone(),
        }
    }

    /// Apply this template to create a new VmConfig.
    pub fn to_config(&self, vm_name: &str, iso_path: Option<String>) -> VmConfig {
        let id = Uuid::new_v4();
        let vm_dir = VmConfig::default_vm_dir();
        let disk_path = format!("{}/{}.qcow2", vm_dir, id);

        VmConfig {
            id,
            name: vm_name.to_string(),
            vcpus: self.vcpus,
            memory_mib: self.memory_mib,
            disk_size_gib: self.disk_size_gib,
            disk_path,
            iso_path,
            os_type: self.os_type.clone(),
            uefi: self.uefi,
            gpu_accel: self.gpu_accel,
            network: self.network.clone(),
            display_protocol: self.display_protocol,
            usb_support: self.usb_support,
            audio: self.audio,
            shared_folder: None,
            description: String::new(),
            boot_order: self.boot_order.clone(),
            network_interfaces: self.network_interfaces.clone(),
            autostart: false,
            tags: self.tags.clone(),
            folder: None,
            favorite: false,
            display_count: self.display_count,
            disk_encrypted: false,
            encryption_secret_uuid: None,
            tpm_enabled: self.uefi, // TPM on when UEFI enabled
            tpm_version: crate::tpm::TpmVersion::V2_0,
            port_forwards: Vec::new(),
            notes: String::new(),
            resource_limits: crate::resource_limits::ResourceLimits::default(),
            performance_profile: "default".to_string(),
            rollback_enabled: false,
            rollback_max_points: 5,
            network_condition: None,
            cpu_topology: None,
            hugepages: false,
            disk_cache: "writeback".to_string(),
            disk_io_mode: "threads".to_string(),
            io_threads: 0,
            vfio_devices: Vec::new(),
            looking_glass: crate::looking_glass::LookingGlassConfig::default(),
            custom_qemu_args: Vec::new(),
            virtio_mem: false,
            iouring: false,
            cpu_features: Vec::new(),
            box_type: crate::qemu_archs::BoxType::Standard,
            qemu_arch: crate::qemu_archs::QemuArch::X86_64,
            machine_type: "q35".to_string(),
            cpu_model: String::new(),
            custom_firmware_code: None,
            custom_firmware_vars: None,
            boot_timeout: 3000,
            preferred_resolution: None,
            use_kvm: true,
            auto_snapshot: crate::auto_snapshot::AutoSnapshotConfig::default(),
            secure_boot: false,
            report_battery: false,
            gpu_model: crate::config::GpuModel::default(),
            video_ram_mb: 64,
            usb_controller: crate::config::UsbControllerVersion::default(),
            disk_mode: crate::config::DiskMode::default(),
            side_channel_mitigations: true,
            serial_ports: Vec::new(),
            parallel_ports: Vec::new(),
            firewall_rules: Vec::new(),
            vfio_hook_dir: None,
            auto_port_forward: false,
            auto_port_forward_skip_privileged: true,
        }
    }

    /// Directory where templates are stored.
    pub fn template_dir() -> String {
        let home = dirs::home_dir().unwrap_or_default();
        format!("{}/.local/share/libre-vmm/templates", home.display())
    }

    /// Save this template to disk.
    /// SECURITY: CWE-732 — Sets restrictive file permissions (0o600) to protect template data.
    pub fn save(&self) -> VmmResult<()> {
        let dir = Self::template_dir();
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        let path = format!("{}/{}.json", dir, self.id);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    /// Load a template by UUID.
    pub fn load(id: &Uuid) -> VmmResult<Self> {
        let path = format!("{}/{}.json", Self::template_dir(), id);
        let json = std::fs::read_to_string(path)?;
        let template: Self = serde_json::from_str(&json)?;
        Ok(template)
    }

    /// Maximum template file size to read (CWE-400).
    /// Templates are small JSON files; anything over 1 MiB is suspicious.
    const MAX_TEMPLATE_FILE_SIZE: u64 = 1024 * 1024;
    /// Maximum number of template files to read (CWE-400).
    const MAX_TEMPLATE_COUNT: usize = 1000;

    /// List all saved templates.
    ///
    /// SECURITY (CWE-59): Uses symlink_metadata to skip symlinked files that could
    /// point outside the template directory. An attacker with write access to the
    /// template directory could create a symlink to a large file (e.g., /dev/urandom)
    /// causing resource exhaustion, or to a sensitive file causing information leak.
    ///
    /// SECURITY (CWE-400): Enforces per-file size limit and total file count limit.
    pub fn list_all() -> VmmResult<Vec<Self>> {
        let dir = Self::template_dir();
        if !std::path::Path::new(&dir).exists() {
            return Ok(Vec::new());
        }
        let mut templates = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;

            // SECURITY (CWE-400): Cap total templates loaded
            if templates.len() >= Self::MAX_TEMPLATE_COUNT {
                tracing::warn!(
                    "Template count limit reached ({}), skipping remaining",
                    Self::MAX_TEMPLATE_COUNT
                );
                break;
            }

            if entry.path().extension().is_some_and(|e| e == "json") {
                // SECURITY (CWE-59): Use symlink_metadata to detect symlinks
                let lmeta = match std::fs::symlink_metadata(entry.path()) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                // Skip symlinks — they could point anywhere
                if lmeta.file_type().is_symlink() {
                    tracing::warn!(
                        "Skipping symlinked template file: {}",
                        entry.path().display()
                    );
                    continue;
                }
                // Skip non-regular files
                if !lmeta.is_file() {
                    continue;
                }
                // SECURITY (CWE-400): Skip oversized files
                if lmeta.len() > Self::MAX_TEMPLATE_FILE_SIZE {
                    tracing::warn!(
                        "Skipping oversized template file '{}' ({} bytes, max {})",
                        entry.path().display(),
                        lmeta.len(),
                        Self::MAX_TEMPLATE_FILE_SIZE
                    );
                    continue;
                }

                let json = match std::fs::read_to_string(entry.path()) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                if let Ok(template) = serde_json::from_str::<VmTemplate>(&json) {
                    templates.push(template);
                }
            }
        }
        templates.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(templates)
    }

    /// Delete this template.
    pub fn delete(&self) -> VmmResult<()> {
        let path = format!("{}/{}.json", Self::template_dir(), self.id);
        if std::path::Path::new(&path).exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Export a template to a file path.
    ///
    /// SECURITY (CWE-22): Validates the export path to prevent path traversal attacks.
    /// Without validation, an attacker-controlled path like "/etc/cron.d/backdoor"
    /// could overwrite arbitrary system files.
    pub fn export_to(&self, path: &str) -> VmmResult<()> {
        let p = std::path::Path::new(path);

        // SECURITY (CWE-22): Reject path traversal components
        for component in p.components() {
            if matches!(component, std::path::Component::ParentDir) {
                return Err(VmmError::InvalidConfig(
                    "Export path must not contain '..' (CWE-22)".to_string(),
                ));
            }
        }

        // SECURITY (CWE-20): Only allow .json extension
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            return Err(VmmError::InvalidConfig(
                "Export path must have a .json extension".to_string(),
            ));
        }

        // SECURITY (CWE-59): Refuse to overwrite symlinks
        if p.exists() {
            let lmeta = std::fs::symlink_metadata(p)
                .map_err(|e| VmmError::Other(format!("Cannot lstat '{}': {}", path, e)))?;
            if lmeta.file_type().is_symlink() {
                return Err(VmmError::InvalidConfig(
                    "Export path is a symbolic link (blocked for security, CWE-59)".to_string(),
                ));
            }
        }

        // SECURITY (CWE-22): Block sensitive system directories
        if let Some(canonical_parent) = p.parent() {
            let parent_str = canonical_parent.to_string_lossy();
            let blocked = [
                "/etc", "/root", "/proc", "/sys", "/dev", "/boot", "/run", "/bin", "/sbin", "/usr",
            ];
            for prefix in blocked {
                if parent_str.starts_with(prefix) {
                    return Err(VmmError::InvalidConfig(format!(
                        "Export path must not be inside '{}' (CWE-22)",
                        prefix
                    )));
                }
            }
        }

        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;

        // SECURITY (CWE-732): Set restrictive permissions on exported file
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }

    // Maximum sane values for numeric fields (SVE #17).
    const MAX_VCPUS: u32 = 1024;
    const MAX_MEMORY_MIB: u64 = 1_048_576; // 1 TiB
    const MAX_DISK_SIZE_GIB: u64 = 65_536; // 64 TiB

    /// Import a template from a file path.
    ///
    /// SECURITY (SVE #16, CWE-22): Canonicalizes the path and requires a `.json`
    /// extension to prevent path-traversal attacks.
    ///
    /// SECURITY (SVE #17, CWE-1284): Validates that deserialized numeric fields
    /// are within sane bounds to prevent resource-exhaustion attacks.
    pub fn import_from(path: &str) -> VmmResult<Self> {
        // SVE #16 — validate the file path before reading.
        let canonical = std::fs::canonicalize(path)
            .map_err(|e| VmmError::Other(format!("Cannot resolve import path: {}", e)))?;

        if canonical.extension().and_then(|e| e.to_str()) != Some("json") {
            return Err(VmmError::InvalidConfig(
                "Template import path must have a .json extension".to_string(),
            ));
        }

        // SECURITY (SVE-K, CWE-400): Check file size before reading to prevent
        // memory exhaustion from a multi-GB file. Templates are small JSON; anything
        // over MAX_TEMPLATE_FILE_SIZE is suspicious.
        let file_meta = std::fs::metadata(&canonical)
            .map_err(|e| VmmError::Other(format!("Cannot stat import file: {}", e)))?;
        if file_meta.len() > Self::MAX_TEMPLATE_FILE_SIZE {
            return Err(VmmError::InvalidConfig(format!(
                "Import file too large ({} bytes, max {} bytes)",
                file_meta.len(),
                Self::MAX_TEMPLATE_FILE_SIZE
            )));
        }

        let json = std::fs::read_to_string(&canonical)?;
        let mut template: Self = serde_json::from_str(&json)
            .map_err(|e| VmmError::Other(format!("Invalid template file: {}", e)))?;

        // SVE #17 — reject templates with out-of-bounds numeric fields.
        if template.vcpus > Self::MAX_VCPUS {
            return Err(VmmError::InvalidConfig(format!(
                "vcpus {} exceeds maximum allowed ({})",
                template.vcpus,
                Self::MAX_VCPUS
            )));
        }
        if template.memory_mib > Self::MAX_MEMORY_MIB {
            return Err(VmmError::InvalidConfig(format!(
                "memory_mib {} exceeds maximum allowed ({} — 1 TiB)",
                template.memory_mib,
                Self::MAX_MEMORY_MIB
            )));
        }
        if template.disk_size_gib > Self::MAX_DISK_SIZE_GIB {
            return Err(VmmError::InvalidConfig(format!(
                "disk_size_gib {} exceeds maximum allowed ({} — 64 TiB)",
                template.disk_size_gib,
                Self::MAX_DISK_SIZE_GIB
            )));
        }

        // Assign a new UUID to avoid collisions
        template.id = Uuid::new_v4();
        template.save()?;
        Ok(template)
    }

    /// Summary string for display.
    pub fn summary(&self) -> String {
        let os = match self.os_type {
            OsType::Linux => "Linux",
            OsType::Windows => "Windows",
            OsType::MacOS => "macOS",
            OsType::FreeBSD => "FreeBSD",
            OsType::Other => "Other",
        };
        format!(
            "{} | {} vCPU | {} MiB RAM | {} GiB disk",
            os, self.vcpus, self.memory_mib, self.disk_size_gib
        )
    }
}
