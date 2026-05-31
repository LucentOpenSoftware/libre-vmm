//! VM configuration — re-exports pure types from `vmm-types::config` and adds
//! the I/O-touching methods (save/load/list/delete, default_vm_dir, templates,
//! TOML/YAML import/export) on top via the `VmConfigIo` extension trait.
//!
//! ## Wave 16.A1 (Windows port foundation)
//!
//! Most of this file used to live here as plain data definitions. Those
//! definitions now live in `vmm-types` so the GUI/CLI/API can eventually
//! depend on them without dragging in libvirt or any Unix-only code. The
//! re-exports below keep every existing `use vmm_core::config::*` import
//! working unchanged.
//!
//! Because the orphan rule forbids bare `impl VmConfig` outside the crate
//! that defines `VmConfig`, every method that used to live on `VmConfig` and
//! that touches I/O is now an inherent associated function on `VmConfig` via
//! the `VmConfigIo` extension trait, plus inline free functions where
//! appropriate. Existing call sites — `VmConfig::default_vm_dir()`,
//! `VmConfig::config_dir()`, `cfg.save()`, etc. — continue to work because
//! `VmConfigIo` is re-exported at the module root and brought into scope by
//! `pub use vmm_types::config::*;` plus the prelude.

#[cfg(test)]
use serde::Deserialize;
use uuid::Uuid;

// Re-export every pure type that moved to vmm-types so existing
// `use vmm_core::config::Foo` paths keep working.
pub use vmm_types::config::*;

// ───── I/O-touching helpers (stay here — they touch the filesystem) ─────

/// Atomic file write helper for declarative spec exports (Wave 12.7).
/// Writes content to `path.with_extension("tmp")`, then renames into place.
/// Ensures readers never see a partial file. On Unix, sets owner-only
/// permissions (0o600) on the final file so config data stays private.
fn atomic_write(path: &std::path::Path, content: &str) -> Result<(), crate::error::VmmError> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    // Rename is atomic on Unix when both paths are on the same filesystem.
    if let Err(e) = std::fs::rename(&tmp, path) {
        // Best-effort cleanup of the temp file on failure.
        let _ = std::fs::remove_file(&tmp);
        return Err(crate::error::VmmError::Io(e));
    }
    Ok(())
}

/// Extension trait adding all the I/O-touching helpers onto the pure
/// `VmConfig` data type that lives in `vmm-types`. Auto-imported when the
/// `vmm_core::config` module is brought into scope, so call sites like
/// `cfg.save()` and `VmConfig::default_vm_dir()` keep working unchanged.
pub trait VmConfigIo: Sized {
    fn from_template(
        name: &str,
        template: &crate::template::OsTemplate,
        iso_path: Option<String>,
    ) -> Self;

    fn from_arch(name: &str, arch: &crate::qemu_archs::QemuArch, machine: &str, cpu: &str) -> Self;

    fn for_power_user(name: &str) -> Self;

    fn default_vm_dir() -> String;
    fn config_dir() -> String;
    fn iso_dir() -> String;

    fn save(&self) -> Result<(), crate::error::VmmError>;
    fn load(id: &Uuid) -> Result<Self, crate::error::VmmError>;
    fn list_all() -> Result<Vec<Self>, crate::error::VmmError>;
    fn delete_config(&self) -> Result<(), crate::error::VmmError>;

    fn to_toml(&self) -> Result<String, crate::error::VmmError>;
    fn to_yaml(&self) -> Result<String, crate::error::VmmError>;
    fn from_toml(text: &str) -> Result<Self, crate::error::VmmError>;
    fn from_yaml(text: &str) -> Result<Self, crate::error::VmmError>;
    fn save_toml(&self, path: &std::path::Path) -> Result<(), crate::error::VmmError>;
    fn save_yaml(&self, path: &std::path::Path) -> Result<(), crate::error::VmmError>;
}

impl VmConfigIo for VmConfig {
    fn from_template(
        name: &str,
        template: &crate::template::OsTemplate,
        iso_path: Option<String>,
    ) -> Self {
        let id = Uuid::new_v4();
        let vm_dir = <Self as VmConfigIo>::default_vm_dir();
        let disk_path = format!("{}/{}.qcow2", vm_dir, id);

        // Sanitize the name to prevent injection in XML/virsh commands
        let safe_name = sanitize_vm_name(name);

        let mut cfg = Self::default();
        cfg.id = id;
        cfg.name = safe_name;
        cfg.vcpus = template.recommended_cpus;
        cfg.memory_mib = template.recommended_memory_mib;
        cfg.disk_size_gib = template.recommended_disk_gib;
        cfg.disk_path = disk_path;
        cfg.iso_path = iso_path;
        cfg.os_type = template.os_type.clone();
        cfg.uefi = template.uefi;
        cfg.tpm_enabled = template.uefi; // TPM on when UEFI enabled
        cfg
    }

    fn from_arch(name: &str, arch: &crate::qemu_archs::QemuArch, machine: &str, cpu: &str) -> Self {
        let id = Uuid::new_v4();
        let vm_dir = <Self as VmConfigIo>::default_vm_dir();
        let disk_path = format!("{}/{}.qcow2", vm_dir, id);
        let defaults = arch.recommended_defaults();
        let use_kvm = arch.can_use_kvm_on_x86();

        let safe_name = sanitize_vm_name(name);

        let mut cfg = Self::default();
        cfg.id = id;
        cfg.name = safe_name;
        cfg.vcpus = defaults.cpus;
        cfg.memory_mib = defaults.memory_mib;
        cfg.disk_size_gib = defaults.disk_gib;
        cfg.disk_path = disk_path;
        cfg.uefi = defaults.uefi;
        cfg.usb_support = arch.has_usb_support();
        cfg.audio = arch.has_audio_support();
        cfg.tpm_enabled = defaults.uefi; // TPM on when UEFI enabled
        cfg.box_type = crate::qemu_archs::BoxType::HardwareLab;
        cfg.qemu_arch = arch.clone();
        cfg.machine_type = machine.to_string();
        cfg.cpu_model = cpu.to_string();
        cfg.use_kvm = use_kvm;
        cfg
    }

    fn for_power_user(name: &str) -> Self {
        let mut config = Self::default();
        config.name = sanitize_vm_name(name);
        config.box_type = crate::qemu_archs::BoxType::PowerUser;
        config.vcpus = 4;
        config.memory_mib = 8192;
        config.disk_size_gib = 50;
        config.disk_cache = "none".to_string();
        config.disk_io_mode = "native".to_string();
        config.hugepages = false; // user enables explicitly
        config.cpu_topology = Some(CpuTopology {
            sockets: 1,
            cores: 4,
            threads: 1,
        });
        config.gpu_accel = true;
        config.disk_mode = DiskMode::default();
        config.side_channel_mitigations = true;
        config
    }

    fn default_vm_dir() -> String {
        let standard = "/var/lib/libvirt/images/libre-vmm";
        if std::path::Path::new("/var/lib/libvirt/images").exists() {
            if std::fs::create_dir_all(standard).is_ok() {
                return standard.to_string();
            }
        }
        let home = dirs::home_dir().unwrap_or_default();
        format!("{}/.local/share/libre-vmm/disks", home.display())
    }

    fn config_dir() -> String {
        let home = dirs::home_dir().unwrap_or_default();
        format!("{}/.local/share/libre-vmm/configs", home.display())
    }

    fn iso_dir() -> String {
        let home = dirs::home_dir().unwrap_or_default();
        format!("{}/.local/share/libre-vmm/isos", home.display())
    }

    fn save(&self) -> Result<(), crate::error::VmmError> {
        let dir = <Self as VmConfigIo>::config_dir();
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

    fn load(id: &Uuid) -> Result<Self, crate::error::VmmError> {
        let path = format!("{}/{}.json", <Self as VmConfigIo>::config_dir(), id);
        let json = std::fs::read_to_string(path)?;
        let mut config: Self = serde_json::from_str(&json)?;
        config.validate_config_bounds();
        Ok(config)
    }

    fn list_all() -> Result<Vec<Self>, crate::error::VmmError> {
        let dir = <Self as VmConfigIo>::config_dir();
        if !std::path::Path::new(&dir).exists() {
            return Ok(Vec::new());
        }
        let mut configs = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if entry.path().extension().is_some_and(|e| e == "json") {
                let json = std::fs::read_to_string(entry.path())?;
                if let Ok(mut config) = serde_json::from_str::<VmConfig>(&json) {
                    config.validate_config_bounds();
                    configs.push(config);
                }
            }
        }
        configs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(configs)
    }

    fn delete_config(&self) -> Result<(), crate::error::VmmError> {
        let path = format!("{}/{}.json", <Self as VmConfigIo>::config_dir(), self.id);
        if std::path::Path::new(&path).exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    fn to_toml(&self) -> Result<String, crate::error::VmmError> {
        toml::to_string_pretty(self)
            .map_err(|e| crate::error::VmmError::InvalidConfig(format!("toml: {}", e)))
    }

    fn to_yaml(&self) -> Result<String, crate::error::VmmError> {
        serde_yaml::to_string(self)
            .map_err(|e| crate::error::VmmError::InvalidConfig(format!("yaml: {}", e)))
    }

    fn from_toml(text: &str) -> Result<Self, crate::error::VmmError> {
        let mut config: Self = toml::from_str(text)
            .map_err(|e| crate::error::VmmError::InvalidConfig(format!("toml: {}", e)))?;
        config.validate_config_bounds();
        Ok(config)
    }

    fn from_yaml(text: &str) -> Result<Self, crate::error::VmmError> {
        let mut config: Self = serde_yaml::from_str(text)
            .map_err(|e| crate::error::VmmError::InvalidConfig(format!("yaml: {}", e)))?;
        config.validate_config_bounds();
        Ok(config)
    }

    fn save_toml(&self, path: &std::path::Path) -> Result<(), crate::error::VmmError> {
        let text = <Self as VmConfigIo>::to_toml(self)?;
        atomic_write(path, &text)
    }

    fn save_yaml(&self, path: &std::path::Path) -> Result<(), crate::error::VmmError> {
        let text = <Self as VmConfigIo>::to_yaml(self)?;
        atomic_write(path, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ───── validate_vm_name (smoke tests; full coverage lives in vmm-types) ─

    #[test]
    fn valid_name_simple() {
        assert!(validate_vm_name("My VM").is_none());
    }

    #[test]
    fn invalid_name_empty() {
        assert_eq!(validate_vm_name(""), Some("VM name cannot be empty"));
    }

    #[test]
    fn invalid_name_shell_metachar_semicolon() {
        assert!(validate_vm_name("vm; rm -rf /").is_some());
    }

    #[test]
    fn invalid_name_starts_with_dot() {
        assert_eq!(
            validate_vm_name(".hidden"),
            Some("VM name cannot start with a dot or hyphen")
        );
    }

    #[test]
    fn invalid_name_starts_with_hyphen() {
        assert_eq!(
            validate_vm_name("-flag"),
            Some("VM name cannot start with a dot or hyphen")
        );
    }

    #[test]
    fn invalid_name_too_long() {
        let name = "a".repeat(129);
        assert_eq!(
            validate_vm_name(&name),
            Some("VM name must be 128 characters or less")
        );
    }

    #[test]
    fn valid_name_128_chars() {
        let name = "a".repeat(128);
        assert!(validate_vm_name(&name).is_none());
    }

    #[test]
    fn invalid_name_shell_metachar_backtick() {
        assert!(validate_vm_name("vm`cmd`").is_some());
    }

    #[test]
    fn invalid_name_dollar_expansion() {
        assert!(validate_vm_name("vm$(whoami)").is_some());
    }

    #[test]
    fn invalid_name_ampersand() {
        assert!(validate_vm_name("vm & bg").is_some());
    }

    #[test]
    fn invalid_name_pipe() {
        assert!(validate_vm_name("vm | cat").is_some());
    }

    #[test]
    fn invalid_name_null_byte() {
        assert!(validate_vm_name("vm\0name").is_some());
    }

    #[test]
    fn invalid_name_leading_whitespace() {
        assert_eq!(
            validate_vm_name(" leading"),
            Some("VM name cannot start or end with whitespace")
        );
    }

    #[test]
    fn invalid_name_trailing_whitespace() {
        assert_eq!(
            validate_vm_name("trailing "),
            Some("VM name cannot start or end with whitespace")
        );
    }

    #[test]
    fn invalid_name_xml_angle_brackets() {
        assert!(validate_vm_name("vm<script>").is_some());
    }

    #[test]
    fn invalid_name_single_quote() {
        assert!(validate_vm_name("vm'name").is_some());
    }

    #[test]
    fn invalid_name_double_quote() {
        assert!(validate_vm_name("vm\"name").is_some());
    }

    #[test]
    fn valid_name_alphanumeric_with_symbols() {
        assert!(validate_vm_name("Ubuntu_22.04-Desktop").is_none());
    }

    #[test]
    fn valid_name_single_char() {
        assert!(validate_vm_name("a").is_none());
    }

    // ───── sanitize_vm_name ────────────────────────────────────────

    #[test]
    fn sanitize_strips_shell_chars() {
        assert_eq!(sanitize_vm_name("vm; rm -rf /"), "vm rm -rf");
    }

    #[test]
    fn sanitize_empty_becomes_unnamed() {
        assert_eq!(sanitize_vm_name(""), "Unnamed-VM");
    }

    #[test]
    fn sanitize_all_bad_chars_becomes_unnamed() {
        assert_eq!(sanitize_vm_name("${}[]<>|"), "Unnamed-VM");
    }

    #[test]
    fn sanitize_preserves_safe_chars() {
        assert_eq!(
            sanitize_vm_name("Ubuntu 22.04-LTS_v2"),
            "Ubuntu 22.04-LTS_v2"
        );
    }

    #[test]
    fn sanitize_preserves_parentheses() {
        assert_eq!(sanitize_vm_name("VM (copy)"), "VM (copy)");
    }

    #[test]
    fn sanitize_trims_whitespace() {
        assert_eq!(sanitize_vm_name("  hello  "), "hello");
    }

    #[test]
    fn sanitize_strips_null_bytes() {
        assert_eq!(sanitize_vm_name("vm\0name"), "vmname");
    }

    // ───── validate_config_bounds ───────────────────────────────────

    fn make_config() -> VmConfig {
        VmConfig::default()
    }

    #[test]
    fn bounds_vcpus_below_min_clamped() {
        let mut c = make_config();
        c.vcpus = 0;
        c.validate_config_bounds();
        assert_eq!(c.vcpus, 1);
    }

    #[test]
    fn bounds_vcpus_above_max_clamped() {
        let mut c = make_config();
        c.vcpus = 1000;
        c.validate_config_bounds();
        assert_eq!(c.vcpus, 512);
    }

    #[test]
    fn bounds_vcpus_valid_untouched() {
        let mut c = make_config();
        c.vcpus = 8;
        c.validate_config_bounds();
        assert_eq!(c.vcpus, 8);
    }

    #[test]
    fn bounds_memory_below_min_clamped() {
        let mut c = make_config();
        c.memory_mib = 64;
        c.validate_config_bounds();
        assert_eq!(c.memory_mib, 128);
    }

    #[test]
    fn bounds_memory_above_max_clamped() {
        let mut c = make_config();
        c.memory_mib = 2_000_000;
        c.validate_config_bounds();
        assert_eq!(c.memory_mib, 1_048_576);
    }

    #[test]
    fn bounds_memory_valid_untouched() {
        let mut c = make_config();
        c.memory_mib = 4096;
        c.validate_config_bounds();
        assert_eq!(c.memory_mib, 4096);
    }

    #[test]
    fn bounds_disk_below_min_clamped() {
        let mut c = make_config();
        c.disk_size_gib = 0;
        c.validate_config_bounds();
        assert_eq!(c.disk_size_gib, 1);
    }

    #[test]
    fn bounds_disk_above_max_clamped() {
        let mut c = make_config();
        c.disk_size_gib = 100_000;
        c.validate_config_bounds();
        assert_eq!(c.disk_size_gib, 65_536);
    }

    #[test]
    fn bounds_display_count_zero_becomes_one() {
        let mut c = make_config();
        c.display_count = 0;
        c.validate_config_bounds();
        assert_eq!(c.display_count, 1);
    }

    #[test]
    fn bounds_display_count_above_eight_clamped() {
        let mut c = make_config();
        c.display_count = 20;
        c.validate_config_bounds();
        assert_eq!(c.display_count, 8);
    }

    #[test]
    fn bounds_rollback_max_zero_becomes_one() {
        let mut c = make_config();
        c.rollback_max_points = 0;
        c.validate_config_bounds();
        assert_eq!(c.rollback_max_points, 1);
    }

    #[test]
    fn bounds_rollback_max_above_100_clamped() {
        let mut c = make_config();
        c.rollback_max_points = 999;
        c.validate_config_bounds();
        assert_eq!(c.rollback_max_points, 100);
    }

    #[test]
    fn bounds_io_threads_above_16_clamped() {
        let mut c = make_config();
        c.io_threads = 64;
        c.validate_config_bounds();
        assert_eq!(c.io_threads, 16);
    }

    #[test]
    fn bounds_io_threads_valid_untouched() {
        let mut c = make_config();
        c.io_threads = 4;
        c.validate_config_bounds();
        assert_eq!(c.io_threads, 4);
    }

    #[test]
    fn bounds_invalid_disk_cache_reset() {
        let mut c = make_config();
        c.disk_cache = "garbage".to_string();
        c.validate_config_bounds();
        assert_eq!(c.disk_cache, "writeback");
    }

    #[test]
    fn bounds_valid_disk_cache_none() {
        let mut c = make_config();
        c.disk_cache = "none".to_string();
        c.validate_config_bounds();
        assert_eq!(c.disk_cache, "none");
    }

    #[test]
    fn bounds_valid_disk_cache_directsync() {
        let mut c = make_config();
        c.disk_cache = "directsync".to_string();
        c.validate_config_bounds();
        assert_eq!(c.disk_cache, "directsync");
    }

    #[test]
    fn bounds_invalid_disk_io_mode_reset() {
        let mut c = make_config();
        c.disk_io_mode = "io_uring".to_string();
        c.validate_config_bounds();
        assert_eq!(c.disk_io_mode, "threads");
    }

    #[test]
    fn bounds_valid_disk_io_mode_native() {
        let mut c = make_config();
        c.disk_io_mode = "native".to_string();
        c.validate_config_bounds();
        assert_eq!(c.disk_io_mode, "native");
    }

    #[test]
    fn bounds_video_ram_below_min_clamped() {
        let mut c = make_config();
        c.video_ram_mb = 4;
        c.validate_config_bounds();
        assert_eq!(c.video_ram_mb, 16);
    }

    #[test]
    fn bounds_video_ram_above_max_clamped() {
        let mut c = make_config();
        c.video_ram_mb = 512;
        c.validate_config_bounds();
        assert_eq!(c.video_ram_mb, 256);
    }

    #[test]
    fn bounds_video_ram_valid_untouched() {
        let mut c = make_config();
        c.video_ram_mb = 128;
        c.validate_config_bounds();
        assert_eq!(c.video_ram_mb, 128);
    }

    #[test]
    fn bounds_port_forwards_truncated_at_256() {
        let mut c = make_config();
        for i in 0..300u16 {
            c.port_forwards.push(PortForwardRule {
                protocol: PortProtocol::Tcp,
                host_port: i,
                guest_port: i,
                description: String::new(),
            });
        }
        c.validate_config_bounds();
        assert_eq!(c.port_forwards.len(), 256);
    }

    #[test]
    fn bounds_cpu_topology_zero_product_cleared() {
        let mut c = make_config();
        c.cpu_topology = Some(CpuTopology {
            sockets: 0,
            cores: 4,
            threads: 2,
        });
        c.validate_config_bounds();
        assert!(c.cpu_topology.is_none());
    }

    #[test]
    fn bounds_cpu_topology_exceeds_max_cleared() {
        let mut c = make_config();
        c.cpu_topology = Some(CpuTopology {
            sockets: 8,
            cores: 128,
            threads: 2,
        });
        c.validate_config_bounds();
        assert!(c.cpu_topology.is_none());
    }

    #[test]
    fn bounds_cpu_topology_valid_kept() {
        let mut c = make_config();
        c.cpu_topology = Some(CpuTopology {
            sockets: 1,
            cores: 4,
            threads: 2,
        });
        c.validate_config_bounds();
        assert!(c.cpu_topology.is_some());
        assert_eq!(c.cpu_topology.unwrap().total_vcpus(), 8);
    }

    // ───── CpuTopology ─────────────────────────────────────────────

    #[test]
    fn cpu_topology_total_vcpus_basic() {
        let t = CpuTopology {
            sockets: 2,
            cores: 4,
            threads: 2,
        };
        assert_eq!(t.total_vcpus(), 16);
    }

    #[test]
    fn cpu_topology_total_vcpus_single() {
        let t = CpuTopology {
            sockets: 1,
            cores: 1,
            threads: 1,
        };
        assert_eq!(t.total_vcpus(), 1);
    }

    #[test]
    fn cpu_topology_total_vcpus_overflow_saturates() {
        let t = CpuTopology {
            sockets: u32::MAX,
            cores: u32::MAX,
            threads: 2,
        };
        assert_eq!(t.total_vcpus(), u32::MAX);
    }

    #[test]
    fn cpu_topology_total_vcpus_zero_component() {
        let t = CpuTopology {
            sockets: 0,
            cores: 4,
            threads: 2,
        };
        assert_eq!(t.total_vcpus(), 0);
    }

    #[test]
    fn cpu_topology_to_xml_basic() {
        let t = CpuTopology {
            sockets: 2,
            cores: 4,
            threads: 2,
        };
        let xml = t.to_xml();
        assert_eq!(xml, "      <topology sockets='2' cores='4' threads='2'/>\n");
    }

    #[test]
    fn cpu_topology_to_xml_clamps_zero_to_one() {
        let t = CpuTopology {
            sockets: 0,
            cores: 0,
            threads: 0,
        };
        let xml = t.to_xml();
        assert_eq!(xml, "      <topology sockets='1' cores='1' threads='1'/>\n");
    }

    #[test]
    fn cpu_topology_to_xml_clamps_large_to_256() {
        let t = CpuTopology {
            sockets: 1000,
            cores: 1,
            threads: 500,
        };
        let xml = t.to_xml();
        assert_eq!(
            xml,
            "      <topology sockets='256' cores='1' threads='256'/>\n"
        );
    }

    #[test]
    fn cpu_topology_display() {
        let t = CpuTopology {
            sockets: 1,
            cores: 4,
            threads: 2,
        };
        assert_eq!(format!("{}", t), "1S × 4C × 2T (8 vCPUs)");
    }

    // ───── effective_nics ──────────────────────────────────────────

    #[test]
    fn effective_nics_empty_interfaces_legacy_fallback() {
        let c = VmConfig {
            network: NetworkMode::Nat,
            network_interfaces: Vec::new(),
            os_type: OsType::Linux,
            ..VmConfig::default()
        };
        let nics = c.effective_nics();
        assert_eq!(nics.len(), 1);
        assert_eq!(nics[0].mode, NetworkMode::Nat);
        assert_eq!(nics[0].model, "virtio");
    }

    #[test]
    fn effective_nics_windows_uses_e1000e() {
        let c = VmConfig {
            network: NetworkMode::Nat,
            network_interfaces: Vec::new(),
            os_type: OsType::Windows,
            ..VmConfig::default()
        };
        let nics = c.effective_nics();
        assert_eq!(nics[0].model, "e1000e");
    }

    #[test]
    fn effective_nics_macos_uses_vmxnet3() {
        let c = VmConfig {
            network: NetworkMode::Nat,
            network_interfaces: Vec::new(),
            os_type: OsType::MacOS,
            ..VmConfig::default()
        };
        let nics = c.effective_nics();
        assert_eq!(nics[0].model, "vmxnet3");
    }

    #[test]
    fn effective_nics_network_none_returns_empty() {
        let c = VmConfig {
            network: NetworkMode::None,
            network_interfaces: Vec::new(),
            ..VmConfig::default()
        };
        let nics = c.effective_nics();
        assert!(nics.is_empty());
    }

    #[test]
    fn effective_nics_populated_used_directly() {
        let custom_nics = vec![
            NicConfig {
                mode: NetworkMode::Bridged,
                model: "e1000e".to_string(),
                mac: "52:54:00:AA:BB:CC".to_string(),
            },
            NicConfig::default(),
        ];
        let c = VmConfig {
            network_interfaces: custom_nics.clone(),
            ..VmConfig::default()
        };
        let nics = c.effective_nics();
        assert_eq!(nics.len(), 2);
        assert_eq!(nics[0].mode, NetworkMode::Bridged);
        assert_eq!(nics[0].mac, "52:54:00:AA:BB:CC");
        assert_eq!(nics[1].mode, NetworkMode::Nat);
    }

    // ───── DisplayProtocol round-trip & legacy compat ──────────────

    #[test]
    fn display_protocol_default_is_vnc() {
        assert_eq!(DisplayProtocol::default(), DisplayProtocol::Vnc);
    }

    #[test]
    fn display_protocol_has_spice() {
        assert!(!DisplayProtocol::Vnc.has_spice());
        assert!(DisplayProtocol::Spice.has_spice());
        assert!(DisplayProtocol::SpiceWithVnc.has_spice());
    }

    #[test]
    fn display_protocol_has_vnc() {
        assert!(DisplayProtocol::Vnc.has_vnc());
        assert!(!DisplayProtocol::Spice.has_vnc());
        assert!(DisplayProtocol::SpiceWithVnc.has_vnc());
    }

    #[test]
    fn display_protocol_serialize_roundtrip() {
        for &proto in DisplayProtocol::ALL {
            let json = serde_json::to_string(&proto).unwrap();
            let back: DisplayProtocol = serde_json::from_str(&json).unwrap();
            assert_eq!(proto, back);
        }
    }

    #[test]
    fn display_protocol_legacy_bool_true() {
        let json = r#"{"display_protocol": true}"#;
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(deserialize_with = "deserialize_display_protocol")]
            display_protocol: DisplayProtocol,
        }
        let w: Wrapper = serde_json::from_str(json).unwrap();
        assert_eq!(w.display_protocol, DisplayProtocol::SpiceWithVnc);
    }

    #[test]
    fn display_protocol_legacy_bool_false() {
        let json = r#"{"display_protocol": false}"#;
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(deserialize_with = "deserialize_display_protocol")]
            display_protocol: DisplayProtocol,
        }
        let w: Wrapper = serde_json::from_str(json).unwrap();
        assert_eq!(w.display_protocol, DisplayProtocol::Vnc);
    }

    #[test]
    fn display_protocol_string_variants() {
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(deserialize_with = "deserialize_display_protocol")]
            display_protocol: DisplayProtocol,
        }

        for (input, expected) in [
            (r#""Vnc""#, DisplayProtocol::Vnc),
            (r#""vnc""#, DisplayProtocol::Vnc),
            (r#""VNC""#, DisplayProtocol::Vnc),
            (r#""Spice""#, DisplayProtocol::Spice),
            (r#""spice""#, DisplayProtocol::Spice),
            (r#""SPICE""#, DisplayProtocol::Spice),
            (r#""SpiceWithVnc""#, DisplayProtocol::SpiceWithVnc),
            (r#""spice_with_vnc""#, DisplayProtocol::SpiceWithVnc),
            (r#""SPICE + VNC""#, DisplayProtocol::SpiceWithVnc),
        ] {
            let json = format!(r#"{{"display_protocol": {}}}"#, input);
            let w: Wrapper = serde_json::from_str(&json).unwrap();
            assert_eq!(w.display_protocol, expected, "failed for input {}", input);
        }
    }

    #[test]
    fn display_protocol_invalid_string_errors() {
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(deserialize_with = "deserialize_display_protocol")]
            #[allow(dead_code)]
            display_protocol: DisplayProtocol,
        }
        let json = r#"{"display_protocol": "banana"}"#;
        assert!(serde_json::from_str::<Wrapper>(json).is_err());
    }

    // ───── Default trait ───────────────────────────────────────────

    #[test]
    fn default_has_sane_vcpus() {
        let d = VmConfig::default();
        assert_eq!(d.vcpus, 2);
    }

    #[test]
    fn default_has_sane_memory() {
        let d = VmConfig::default();
        assert_eq!(d.memory_mib, 2048);
    }

    #[test]
    fn default_has_sane_disk() {
        let d = VmConfig::default();
        assert_eq!(d.disk_size_gib, 20);
    }

    #[test]
    fn default_uefi_enabled() {
        assert!(VmConfig::default().uefi);
    }

    #[test]
    fn default_os_type_linux() {
        assert_eq!(VmConfig::default().os_type, OsType::Linux);
    }

    #[test]
    fn default_network_nat() {
        assert_eq!(VmConfig::default().network, NetworkMode::Nat);
    }

    #[test]
    fn default_display_protocol_vnc() {
        assert_eq!(VmConfig::default().display_protocol, DisplayProtocol::Vnc);
    }

    #[test]
    fn default_boot_order_cdrom_then_hd() {
        let d = VmConfig::default();
        assert_eq!(d.boot_order, vec![BootDevice::Cdrom, BootDevice::Hd]);
    }

    #[test]
    fn default_disk_cache_writeback() {
        assert_eq!(VmConfig::default().disk_cache, "writeback");
    }

    #[test]
    fn default_disk_io_mode_threads() {
        assert_eq!(VmConfig::default().disk_io_mode, "threads");
    }

    #[test]
    fn default_machine_type_q35() {
        assert_eq!(VmConfig::default().machine_type, "q35");
    }

    #[test]
    fn default_usb_support_enabled() {
        assert!(VmConfig::default().usb_support);
    }

    #[test]
    fn default_audio_enabled() {
        assert!(VmConfig::default().audio);
    }

    #[test]
    fn default_gpu_model_auto() {
        assert_eq!(VmConfig::default().gpu_model, GpuModel::Auto);
    }

    #[test]
    fn default_video_ram_64() {
        assert_eq!(VmConfig::default().video_ram_mb, 64);
    }

    #[test]
    fn default_usb_controller_usb3() {
        assert_eq!(
            VmConfig::default().usb_controller,
            UsbControllerVersion::Usb3
        );
    }

    #[test]
    fn default_tpm_enabled() {
        assert!(VmConfig::default().tpm_enabled);
    }

    #[test]
    fn default_display_count_one() {
        assert_eq!(VmConfig::default().display_count, 1);
    }

    #[test]
    fn default_rollback_max_points_five() {
        assert_eq!(VmConfig::default().rollback_max_points, 5);
    }

    #[test]
    fn default_passes_own_bounds_check() {
        let mut d = VmConfig::default();
        let before_vcpus = d.vcpus;
        let before_memory = d.memory_mib;
        let before_disk = d.disk_size_gib;
        d.validate_config_bounds();
        assert_eq!(d.vcpus, before_vcpus);
        assert_eq!(d.memory_mib, before_memory);
        assert_eq!(d.disk_size_gib, before_disk);
    }

    // ───── from_template ───────────────────────────────────────────

    #[test]
    fn from_template_uses_template_values() {
        let template = crate::template::OsTemplate {
            id: "test-linux",
            label: "Test Linux",
            os_type: OsType::Linux,
            category: crate::template::OsCategory::LinuxDesktop,
            recommended_cpus: 4,
            recommended_memory_mib: 8192,
            recommended_disk_gib: 50,
            uefi: true,
            description: "A test template",
        };
        let config = VmConfig::from_template("Test VM", &template, Some("/tmp/test.iso".into()));
        assert_eq!(config.name, "Test VM");
        assert_eq!(config.vcpus, 4);
        assert_eq!(config.memory_mib, 8192);
        assert_eq!(config.disk_size_gib, 50);
        assert_eq!(config.os_type, OsType::Linux);
        assert!(config.uefi);
        assert_eq!(config.iso_path, Some("/tmp/test.iso".into()));
        assert!(config.tpm_enabled);
    }

    #[test]
    fn from_template_sanitizes_name() {
        let template = crate::template::OsTemplate {
            id: "t",
            label: "T",
            os_type: OsType::Linux,
            category: crate::template::OsCategory::LinuxDesktop,
            recommended_cpus: 1,
            recommended_memory_mib: 1024,
            recommended_disk_gib: 10,
            uefi: false,
            description: "",
        };
        let config = VmConfig::from_template("bad;name$(evil)", &template, None);
        assert!(!config.name.contains(';'));
        assert!(!config.name.contains('$'));
    }

    // ───── for_power_user ──────────────────────────────────────────

    #[test]
    fn power_user_has_boosted_resources() {
        let c = VmConfig::for_power_user("Power Box");
        assert_eq!(c.vcpus, 4);
        assert_eq!(c.memory_mib, 8192);
        assert_eq!(c.disk_size_gib, 50);
    }

    #[test]
    fn power_user_disk_cache_none() {
        let c = VmConfig::for_power_user("PU");
        assert_eq!(c.disk_cache, "none");
    }

    #[test]
    fn power_user_disk_io_native() {
        let c = VmConfig::for_power_user("PU");
        assert_eq!(c.disk_io_mode, "native");
    }

    #[test]
    fn power_user_has_cpu_topology() {
        let c = VmConfig::for_power_user("PU");
        assert!(c.cpu_topology.is_some());
        let topo = c.cpu_topology.unwrap();
        assert_eq!(topo.sockets, 1);
        assert_eq!(topo.cores, 4);
        assert_eq!(topo.threads, 1);
        assert_eq!(topo.total_vcpus(), 4);
    }

    #[test]
    fn power_user_gpu_accel_enabled() {
        let c = VmConfig::for_power_user("PU");
        assert!(c.gpu_accel);
    }

    #[test]
    fn power_user_box_type() {
        let c = VmConfig::for_power_user("PU");
        assert_eq!(c.box_type, crate::qemu_archs::BoxType::PowerUser);
    }

    #[test]
    fn power_user_sanitizes_name() {
        let c = VmConfig::for_power_user("bad|name<x>");
        assert!(!c.name.contains('|'));
        assert!(!c.name.contains('<'));
        assert!(!c.name.contains('>'));
    }

    // ───── from_arch ───────────────────────────────────────────────

    #[test]
    fn from_arch_sets_architecture() {
        let arch = crate::qemu_archs::QemuArch::Aarch64;
        let c = VmConfig::from_arch("ARM VM", &arch, "virt", "cortex-a72");
        assert_eq!(c.qemu_arch, crate::qemu_archs::QemuArch::Aarch64);
        assert_eq!(c.machine_type, "virt");
        assert_eq!(c.cpu_model, "cortex-a72");
        assert_eq!(c.box_type, crate::qemu_archs::BoxType::HardwareLab);
    }

    #[test]
    fn from_arch_uses_arch_defaults() {
        let arch = crate::qemu_archs::QemuArch::X86_64;
        let defaults = arch.recommended_defaults();
        let c = VmConfig::from_arch("x86 VM", &arch, "q35", "host");
        assert_eq!(c.vcpus, defaults.cpus);
        assert_eq!(c.memory_mib, defaults.memory_mib);
        assert_eq!(c.disk_size_gib, defaults.disk_gib);
    }

    // ───── Enum Display impls ──────────────────────────────────────

    #[test]
    fn network_mode_display() {
        assert_eq!(format!("{}", NetworkMode::Nat), "NAT");
        assert_eq!(format!("{}", NetworkMode::Bridged), "Bridged");
        assert_eq!(format!("{}", NetworkMode::HostOnly), "Host Only");
        assert_eq!(format!("{}", NetworkMode::None), "None");
    }

    #[test]
    fn boot_device_display() {
        assert_eq!(format!("{}", BootDevice::Cdrom), "CD/DVD");
        assert_eq!(format!("{}", BootDevice::Hd), "Hard Disk");
        assert_eq!(format!("{}", BootDevice::Network), "Network (PXE)");
        assert_eq!(format!("{}", BootDevice::Floppy), "Floppy");
    }

    #[test]
    fn boot_device_xml_names() {
        assert_eq!(BootDevice::Cdrom.xml_name(), "cdrom");
        assert_eq!(BootDevice::Hd.xml_name(), "hd");
        assert_eq!(BootDevice::Network.xml_name(), "network");
        assert_eq!(BootDevice::Floppy.xml_name(), "fd");
    }

    #[test]
    fn port_forward_display() {
        let rule = PortForwardRule {
            protocol: PortProtocol::Tcp,
            host_port: 2222,
            guest_port: 22,
            description: "SSH".to_string(),
        };
        let s = format!("{}", rule);
        assert!(s.contains("TCP"));
        assert!(s.contains("2222"));
        assert!(s.contains("22"));
        assert!(s.contains("SSH"));
    }

    #[test]
    fn port_forward_display_no_desc() {
        let rule = PortForwardRule {
            protocol: PortProtocol::Udp,
            host_port: 5000,
            guest_port: 5000,
            description: String::new(),
        };
        let s = format!("{}", rule);
        assert!(s.contains("UDP"));
        assert!(!s.contains("("));
    }

    // ───── GpuModel ────────────────────────────────────────────────

    #[test]
    fn gpu_model_auto_linux_is_virtio() {
        assert_eq!(GpuModel::Auto.libvirt_model(&OsType::Linux), "virtio");
    }

    #[test]
    fn gpu_model_auto_windows_is_qxl() {
        assert_eq!(GpuModel::Auto.libvirt_model(&OsType::Windows), "qxl");
    }

    #[test]
    fn gpu_model_auto_macos_is_vmvga() {
        assert_eq!(GpuModel::Auto.libvirt_model(&OsType::MacOS), "vmvga");
    }

    #[test]
    fn gpu_model_supports_3d() {
        assert!(GpuModel::Auto.supports_3d());
        assert!(GpuModel::VirtioGpu.supports_3d());
        assert!(GpuModel::VirtioGpuGl.supports_3d());
        assert!(!GpuModel::Qxl.supports_3d());
        assert!(!GpuModel::Vga.supports_3d());
        assert!(!GpuModel::VmwareSvga.supports_3d());
        assert!(!GpuModel::None.supports_3d());
    }

    // ───── Wave 11.2 — LAN segments ────────────────────────────────

    #[test]
    fn network_mode_lan_segment_display() {
        let m = NetworkMode::LanSegment("lab-frontend".to_string());
        assert_eq!(format!("{}", m), "LAN: lab-frontend");
    }

    #[test]
    fn network_mode_lan_segment_serialize_roundtrip() {
        let m = NetworkMode::LanSegment("lab-backend".to_string());
        let json = serde_json::to_string(&m).unwrap();
        let back: NetworkMode = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn sanitize_lan_segment_name_basic() {
        assert_eq!(sanitize_lan_segment_name("lab-frontend"), "lab-frontend");
    }

    #[test]
    fn sanitize_lan_segment_name_uppercase_lowered() {
        assert_eq!(sanitize_lan_segment_name("Lab-Frontend"), "lab-frontend");
    }

    #[test]
    fn sanitize_lan_segment_name_strips_bad_chars() {
        assert_eq!(sanitize_lan_segment_name("lab/front<end>"), "lab-front-end");
    }

    #[test]
    fn sanitize_lan_segment_name_empty_becomes_default() {
        assert_eq!(sanitize_lan_segment_name(""), "default");
        assert_eq!(sanitize_lan_segment_name("///"), "default");
    }

    #[test]
    fn sanitize_lan_segment_name_collapses_hyphens() {
        assert_eq!(sanitize_lan_segment_name("a___b   c"), "a-b-c");
    }

    #[test]
    fn effective_nics_lan_segment_fallback() {
        let c = VmConfig {
            network: NetworkMode::LanSegment("lab1".to_string()),
            network_interfaces: Vec::new(),
            os_type: OsType::Linux,
            ..VmConfig::default()
        };
        let nics = c.effective_nics();
        assert_eq!(nics.len(), 1);
        assert_eq!(nics[0].mode, NetworkMode::LanSegment("lab1".to_string()));
        assert_eq!(nics[0].model, "virtio");
    }

    // ───── Wave 11.3 — DiskMode ─────────────────────────────────────

    #[test]
    fn disk_mode_default_snapshotted() {
        assert_eq!(DiskMode::default(), DiskMode::Snapshotted);
    }

    #[test]
    fn disk_mode_display() {
        assert_eq!(format!("{}", DiskMode::Snapshotted), "Snapshotted");
        assert_eq!(
            format!("{}", DiskMode::IndependentPersistent),
            "Independent - Persistent"
        );
        assert_eq!(
            format!("{}", DiskMode::IndependentNonpersistent),
            "Independent - Nonpersistent"
        );
    }

    #[test]
    fn disk_mode_serialize_roundtrip() {
        for &m in DiskMode::ALL {
            let json = serde_json::to_string(&m).unwrap();
            let back: DiskMode = serde_json::from_str(&json).unwrap();
            assert_eq!(m, back);
        }
    }

    #[test]
    fn default_disk_mode_snapshotted() {
        assert_eq!(VmConfig::default().disk_mode, DiskMode::Snapshotted);
    }

    // ───── Wave 11.4 — Side-channel mitigations ─────────────────────

    #[test]
    fn default_side_channel_mitigations_on() {
        assert!(VmConfig::default().side_channel_mitigations);
    }

    #[test]
    fn power_user_side_channel_mitigations_on() {
        assert!(VmConfig::for_power_user("PU").side_channel_mitigations);
    }

    // ───── Wave 11.5 — Display heads ────────────────────────────────

    #[test]
    fn bounds_display_count_9_clamped_to_8() {
        let mut c = make_config();
        c.display_count = 9;
        c.validate_config_bounds();
        assert_eq!(c.display_count, 8);
    }

    #[test]
    fn bounds_display_count_8_kept() {
        let mut c = make_config();
        c.display_count = 8;
        c.validate_config_bounds();
        assert_eq!(c.display_count, 8);
    }

    // ───── Wave 11.6 — Serial / Parallel ports ──────────────────────

    #[test]
    fn serial_backend_default_pty() {
        assert_eq!(SerialBackend::default(), SerialBackend::Pty);
    }

    #[test]
    fn serial_backend_display() {
        assert_eq!(format!("{}", SerialBackend::Pty), "PTY");
        assert_eq!(format!("{}", SerialBackend::File), "File");
        assert_eq!(format!("{}", SerialBackend::UnixSocket), "Unix Socket");
        assert_eq!(format!("{}", SerialBackend::Tcp), "TCP");
        assert_eq!(format!("{}", SerialBackend::Null), "Null");
    }

    #[test]
    fn serial_backend_libvirt_type() {
        assert_eq!(SerialBackend::Pty.libvirt_type(), "pty");
        assert_eq!(SerialBackend::File.libvirt_type(), "file");
        assert_eq!(SerialBackend::UnixSocket.libvirt_type(), "unix");
        assert_eq!(SerialBackend::Tcp.libvirt_type(), "tcp");
        assert_eq!(SerialBackend::Null.libvirt_type(), "null");
    }

    #[test]
    fn serial_port_config_serialize_roundtrip() {
        let p = SerialPortConfig {
            backend: SerialBackend::File,
            target: "/var/log/vm-serial.log".to_string(),
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: SerialPortConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn parallel_port_config_serialize_roundtrip() {
        let p = ParallelPortConfig {
            backend: SerialBackend::Null,
            target: String::new(),
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: ParallelPortConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn bounds_serial_ports_truncated_at_4() {
        let mut c = make_config();
        for _ in 0..10 {
            c.serial_ports.push(SerialPortConfig::default());
        }
        c.validate_config_bounds();
        assert_eq!(c.serial_ports.len(), 4);
    }

    #[test]
    fn bounds_parallel_ports_truncated_at_3() {
        let mut c = make_config();
        for _ in 0..10 {
            c.parallel_ports.push(ParallelPortConfig::default());
        }
        c.validate_config_bounds();
        assert_eq!(c.parallel_ports.len(), 3);
    }

    // ───── Wave 12.5 — Per-VM firewall rules ─────────────────────────

    fn make_rule(action: FirewallAction, proto: FirewallProtocol, port: &str) -> FirewallRule {
        FirewallRule {
            action,
            direction: FirewallDirection::In,
            protocol: proto,
            remote_addr: String::new(),
            local_port: port.to_string(),
            remote_port: String::new(),
            priority: 100,
            description: String::new(),
        }
    }

    #[test]
    fn firewall_action_defaults_to_accept() {
        assert_eq!(FirewallAction::default(), FirewallAction::Accept);
    }

    #[test]
    fn firewall_direction_defaults_to_both() {
        assert_eq!(FirewallDirection::default(), FirewallDirection::Both);
    }

    #[test]
    fn firewall_protocol_defaults_to_any() {
        assert_eq!(FirewallProtocol::default(), FirewallProtocol::Any);
    }

    #[test]
    fn firewall_action_display() {
        assert_eq!(format!("{}", FirewallAction::Accept), "Accept");
        assert_eq!(format!("{}", FirewallAction::Drop), "Drop");
        assert_eq!(format!("{}", FirewallAction::Reject), "Reject");
    }

    #[test]
    fn firewall_direction_display() {
        assert_eq!(format!("{}", FirewallDirection::In), "In");
        assert_eq!(format!("{}", FirewallDirection::Out), "Out");
        assert_eq!(format!("{}", FirewallDirection::Both), "Both");
    }

    #[test]
    fn firewall_protocol_display() {
        assert_eq!(format!("{}", FirewallProtocol::Tcp), "TCP");
        assert_eq!(format!("{}", FirewallProtocol::Udp), "UDP");
        assert_eq!(format!("{}", FirewallProtocol::Icmp), "ICMP");
        assert_eq!(format!("{}", FirewallProtocol::Any), "Any");
    }

    #[test]
    fn firewall_protocol_libvirt_element() {
        assert_eq!(FirewallProtocol::Tcp.libvirt_element(), "tcp");
        assert_eq!(FirewallProtocol::Udp.libvirt_element(), "udp");
        assert_eq!(FirewallProtocol::Icmp.libvirt_element(), "icmp");
        assert_eq!(FirewallProtocol::Any.libvirt_element(), "all");
    }

    #[test]
    fn firewall_direction_libvirt_value() {
        assert_eq!(FirewallDirection::In.libvirt_direction(), "in");
        assert_eq!(FirewallDirection::Out.libvirt_direction(), "out");
        assert_eq!(FirewallDirection::Both.libvirt_direction(), "inout");
    }

    #[test]
    fn firewall_rule_serialize_roundtrip() {
        let rule = FirewallRule {
            action: FirewallAction::Drop,
            direction: FirewallDirection::Out,
            protocol: FirewallProtocol::Udp,
            remote_addr: "192.168.1.0/24".to_string(),
            local_port: "53".to_string(),
            remote_port: "1024-65535".to_string(),
            priority: 250,
            description: "Block outbound DNS".to_string(),
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: FirewallRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, back);
    }

    #[test]
    fn firewall_rule_default_priority_500() {
        let r = FirewallRule::default();
        assert_eq!(r.priority, 500);
        assert_eq!(r.action, FirewallAction::Accept);
        assert_eq!(r.direction, FirewallDirection::Both);
        assert_eq!(r.protocol, FirewallProtocol::Any);
    }

    #[test]
    fn bounds_firewall_rules_truncated_at_64() {
        let mut c = make_config();
        for _ in 0..100 {
            c.firewall_rules.push(FirewallRule::default());
        }
        c.validate_config_bounds();
        assert_eq!(c.firewall_rules.len(), 64);
    }

    #[test]
    fn bounds_firewall_priority_clamped_low() {
        let mut c = make_config();
        let mut r = FirewallRule::default();
        r.priority = -100;
        c.firewall_rules.push(r);
        c.validate_config_bounds();
        assert_eq!(c.firewall_rules[0].priority, 0);
    }

    #[test]
    fn bounds_firewall_priority_clamped_high() {
        let mut c = make_config();
        let mut r = FirewallRule::default();
        r.priority = 9999;
        c.firewall_rules.push(r);
        c.validate_config_bounds();
        assert_eq!(c.firewall_rules[0].priority, 1000);
    }

    #[test]
    fn bounds_firewall_invalid_remote_addr_cleared() {
        let mut c = make_config();
        let mut r = FirewallRule::default();
        r.remote_addr = "bad;DROP TABLE--".to_string();
        c.firewall_rules.push(r);
        c.validate_config_bounds();
        assert_eq!(c.firewall_rules[0].remote_addr, "");
    }

    #[test]
    fn bounds_firewall_valid_remote_addr_kept() {
        let mut c = make_config();
        let mut r = FirewallRule::default();
        r.remote_addr = "10.0.0.0/8".to_string();
        c.firewall_rules.push(r);
        c.validate_config_bounds();
        assert_eq!(c.firewall_rules[0].remote_addr, "10.0.0.0/8");
    }

    #[test]
    fn bounds_firewall_ipv6_addr_kept() {
        let mut c = make_config();
        let mut r = FirewallRule::default();
        r.remote_addr = "fe80::1".to_string();
        c.firewall_rules.push(r);
        c.validate_config_bounds();
        assert_eq!(c.firewall_rules[0].remote_addr, "fe80::1");
    }

    #[test]
    fn bounds_firewall_invalid_port_cleared() {
        let mut c = make_config();
        let mut r = FirewallRule::default();
        r.local_port = "22; rm -rf".to_string();
        r.remote_port = "abc".to_string();
        c.firewall_rules.push(r);
        c.validate_config_bounds();
        assert_eq!(c.firewall_rules[0].local_port, "");
        assert_eq!(c.firewall_rules[0].remote_port, "");
    }

    #[test]
    fn bounds_firewall_valid_port_kept() {
        let mut c = make_config();
        let r = make_rule(FirewallAction::Accept, FirewallProtocol::Tcp, "8080");
        c.firewall_rules.push(r);
        let mut r2 = make_rule(FirewallAction::Accept, FirewallProtocol::Tcp, "1000-2000");
        r2.remote_port = "443".to_string();
        c.firewall_rules.push(r2);
        c.validate_config_bounds();
        assert_eq!(c.firewall_rules[0].local_port, "8080");
        assert_eq!(c.firewall_rules[1].local_port, "1000-2000");
        assert_eq!(c.firewall_rules[1].remote_port, "443");
    }

    #[test]
    fn bounds_firewall_description_truncated_at_256() {
        let mut c = make_config();
        let mut r = FirewallRule::default();
        r.description = "X".repeat(500);
        c.firewall_rules.push(r);
        c.validate_config_bounds();
        assert_eq!(c.firewall_rules[0].description.len(), 256);
    }

    #[test]
    fn default_vm_config_has_empty_firewall_rules() {
        assert!(VmConfig::default().firewall_rules.is_empty());
    }

    #[test]
    fn power_user_has_empty_firewall_rules() {
        assert!(VmConfig::for_power_user("PU").firewall_rules.is_empty());
    }

    // ───── Wave 12.7 — TOML/YAML declarative specs ───────────────────

    fn sample_config() -> VmConfig {
        let mut c = VmConfig::default();
        c.name = "Spec Test".to_string();
        c.vcpus = 4;
        c.memory_mib = 4096;
        c.disk_size_gib = 30;
        c.notes = "Test VM\nwith newlines".to_string();
        c.tags = vec!["test".to_string(), "spec".to_string()];
        c.network_interfaces = vec![NicConfig {
            mode: NetworkMode::Bridged,
            model: "virtio".to_string(),
            mac: "52:54:00:11:22:33".to_string(),
        }];
        c.firewall_rules = vec![FirewallRule {
            action: FirewallAction::Accept,
            direction: FirewallDirection::In,
            protocol: FirewallProtocol::Tcp,
            remote_addr: String::new(),
            local_port: "22".to_string(),
            remote_port: String::new(),
            priority: 100,
            description: "Allow SSH".to_string(),
        }];
        c
    }

    #[test]
    fn toml_roundtrip_preserves_fields() {
        let c = sample_config();
        let text = c.to_toml().expect("toml encode");
        let back = VmConfig::from_toml(&text).expect("toml decode");
        assert_eq!(back.name, c.name);
        assert_eq!(back.vcpus, c.vcpus);
        assert_eq!(back.memory_mib, c.memory_mib);
        assert_eq!(back.disk_size_gib, c.disk_size_gib);
        assert_eq!(back.notes, c.notes);
        assert_eq!(back.tags, c.tags);
        assert_eq!(back.network_interfaces, c.network_interfaces);
        assert_eq!(back.firewall_rules, c.firewall_rules);
    }

    #[test]
    fn yaml_roundtrip_preserves_fields() {
        let c = sample_config();
        let text = c.to_yaml().expect("yaml encode");
        let back = VmConfig::from_yaml(&text).expect("yaml decode");
        assert_eq!(back.name, c.name);
        assert_eq!(back.vcpus, c.vcpus);
        assert_eq!(back.memory_mib, c.memory_mib);
        assert_eq!(back.tags, c.tags);
        assert_eq!(back.network_interfaces, c.network_interfaces);
        assert_eq!(back.firewall_rules, c.firewall_rules);
    }

    #[test]
    fn from_toml_invalid_returns_invalid_config_error() {
        let result = VmConfig::from_toml("not valid toml [[[");
        match result {
            Err(crate::error::VmmError::InvalidConfig(msg)) => {
                assert!(msg.starts_with("toml:"), "expected toml prefix in {}", msg);
            },
            other => panic!("expected InvalidConfig, got {:?}", other),
        }
    }

    #[test]
    fn from_yaml_invalid_returns_invalid_config_error() {
        let result = VmConfig::from_yaml("\t\t\t: : : invalid");
        match result {
            Err(crate::error::VmmError::InvalidConfig(msg)) => {
                assert!(msg.starts_with("yaml:"), "expected yaml prefix in {}", msg);
            },
            other => panic!("expected InvalidConfig, got {:?}", other),
        }
    }

    #[test]
    fn from_toml_calls_validate_config_bounds() {
        let mut c = VmConfig::default();
        c.vcpus = 8;
        c.memory_mib = 4096;
        let mut text = c.to_toml().expect("toml encode");
        text = text.replace("vcpus = 8", "vcpus = 99999");
        text = text.replace("memory_mib = 4096", "memory_mib = 9999999999");
        let back = VmConfig::from_toml(&text).expect("toml decode");
        assert_eq!(back.vcpus, 512, "vcpus should clamp to max 512");
        assert_eq!(
            back.memory_mib, 1_048_576,
            "memory_mib should clamp to 1 TiB"
        );
    }

    #[test]
    fn from_yaml_calls_validate_config_bounds() {
        let mut c = VmConfig::default();
        c.disk_size_gib = 50;
        let mut text = c.to_yaml().expect("yaml encode");
        text = text.replace("disk_size_gib: 50", "disk_size_gib: 999999");
        let back = VmConfig::from_yaml(&text).expect("yaml decode");
        assert_eq!(back.disk_size_gib, 65_536);
    }

    #[test]
    fn toml_output_is_human_readable() {
        let c = sample_config();
        let text = c.to_toml().expect("toml encode");
        assert!(text.contains("name = "));
        assert!(text.contains("vcpus = "));
        assert!(text.contains("network_interfaces") || text.contains("[[network_interfaces]]"));
    }

    #[test]
    fn save_toml_atomic_no_tmp_left() {
        let dir = std::env::temp_dir().join(format!("vmm-toml-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("vm.toml");
        let c = sample_config();
        c.save_toml(&path).expect("save_toml");
        assert!(path.exists(), "final file should exist");
        let tmp = path.with_extension("tmp");
        assert!(!tmp.exists(), "temp file must be cleaned up");
        let back = VmConfig::from_toml(&std::fs::read_to_string(&path).unwrap()).expect("reload");
        assert_eq!(back.name, c.name);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_yaml_atomic_no_tmp_left() {
        let dir = std::env::temp_dir().join(format!("vmm-yaml-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("vm.yaml");
        let c = sample_config();
        c.save_yaml(&path).expect("save_yaml");
        assert!(path.exists());
        let tmp = path.with_extension("tmp");
        assert!(!tmp.exists());
        let back = VmConfig::from_yaml(&std::fs::read_to_string(&path).unwrap()).expect("reload");
        assert_eq!(back.name, c.name);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ───── Wave 11 — full round-trip ────────────────────────────────

    #[test]
    fn vmconfig_wave11_fields_roundtrip() {
        let mut c = VmConfig::default();
        c.network = NetworkMode::LanSegment("lab-x".to_string());
        c.disk_mode = DiskMode::IndependentNonpersistent;
        c.side_channel_mitigations = false;
        c.serial_ports.push(SerialPortConfig {
            backend: SerialBackend::Tcp,
            target: "127.0.0.1:4555".to_string(),
        });
        c.parallel_ports.push(ParallelPortConfig {
            backend: SerialBackend::File,
            target: "/var/log/vm-parallel.log".to_string(),
        });
        let json = serde_json::to_string(&c).unwrap();
        let back: VmConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(c.network, back.network);
        assert_eq!(c.disk_mode, back.disk_mode);
        assert_eq!(c.side_channel_mitigations, back.side_channel_mitigations);
        assert_eq!(c.serial_ports, back.serial_ports);
        assert_eq!(c.parallel_ports, back.parallel_ports);
    }
}
