//! Generates libvirt domain XML from a VmConfig.
//! This is the bridge between our simple config and what libvirt needs.
//!
//! Supports multi-architecture VMs via the `qemu_arch` and `machine_type`
//! fields on VmConfig. For Box 2 (Hardware Lab) cross-architecture emulation,
//! the XML builder adapts device models, bus types, and firmware paths
//! based on the target architecture and machine type.

use crate::config::{
    DiskMode, FirewallRule, NetworkMode, NicConfig, OsType, ParallelPortConfig, SerialBackend,
    SerialPortConfig, VmConfig,
};
use crate::qemu_archs::{QemuArch, QemuArchIo};
use crate::resource_limits::ResourceLimitsXml;

/// Validate a disk/image path for safety: must be absolute, no symlinks, no traversal,
/// no sensitive directories. Returns the validated path or None if unsafe.
/// SECURITY: CWE-22 (Path Traversal), CWE-59 (Symlink Following)
fn validate_disk_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return None;
    }
    // Block ".." components (CWE-22)
    for component in p.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return None;
        }
    }
    // Check for symlinks if path exists (CWE-59)
    if p.exists() {
        if let Ok(lmeta) = std::fs::symlink_metadata(p) {
            if lmeta.file_type().is_symlink() {
                return None;
            }
            if !lmeta.is_file() {
                return None;
            }
        }
    }
    // Block access to sensitive directories
    let blocked_prefixes = [
        "/etc",
        "/root",
        "/proc",
        "/sys",
        "/dev",
        "/boot",
        "/run",
        "/var/run",
        "/var/log",
        "/var/lib",
        "/var/spool",
        "/usr/sbin",
        "/usr/bin",
        "/sbin",
        "/bin",
        "/tmp",
        "/var/tmp",
        "/home/root",
        "/snap",
        "/lost+found",
    ];
    for prefix in blocked_prefixes {
        if path.starts_with(prefix) {
            return None;
        }
    }
    Some(path.to_string())
}

/// Validate a firmware path: must be absolute, must exist, must not contain path traversal.
/// SECURITY: Prevents loading arbitrary files as UEFI firmware (CWE-22, CWE-73).
fn validate_firmware_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let p = std::path::Path::new(path);
    // Must be absolute
    if !p.is_absolute() {
        tracing::warn!("Blocked non-absolute firmware path: {}", path);
        return None;
    }
    // Block ".." components (CWE-22)
    for component in p.components() {
        if matches!(component, std::path::Component::ParentDir) {
            tracing::warn!("Blocked firmware path with traversal: {}", path);
            return None;
        }
    }
    // Must exist on disk
    if !p.exists() {
        tracing::warn!("Blocked firmware path (does not exist): {}", path);
        return None;
    }
    // Check for symlinks (CWE-59)
    if let Ok(lmeta) = std::fs::symlink_metadata(p) {
        if lmeta.file_type().is_symlink() {
            tracing::warn!("Blocked symlink firmware path: {}", path);
            return None;
        }
    }
    Some(path.to_string())
}

/// Resolve firmware (OVMF code + vars) paths: custom config overrides arch defaults.
/// Both custom paths must be valid for custom firmware to be used; otherwise falls back.
/// When `config.secure_boot` is true AND `config.uefi` is true, uses the secboot
/// firmware variant (OVMF_CODE_4M.secboot.fd) instead of the regular one.
fn resolve_firmware_paths(config: &VmConfig) -> Option<(String, String)> {
    // Try custom paths first
    if let (Some(ref custom_code), Some(ref custom_vars)) =
        (&config.custom_firmware_code, &config.custom_firmware_vars)
    {
        let valid_code = validate_firmware_path(custom_code);
        let valid_vars = validate_firmware_path(custom_vars);
        if let (Some(code), Some(vars)) = (valid_code, valid_vars) {
            return Some((code, vars));
        }
        tracing::warn!("Custom firmware paths invalid, falling back to arch defaults");
    }
    // Fall back to architecture defaults — use secboot variant when Secure Boot enabled
    if config.secure_boot && config.uefi {
        config
            .qemu_arch
            .uefi_secboot_firmware_path()
            .map(|(c, v)| (c.to_string(), v.to_string()))
    } else {
        config
            .qemu_arch
            .uefi_firmware_path()
            .map(|(c, v)| (c.to_string(), v.to_string()))
    }
}

/// Sanitize a value for use in QEMU fw_cfg strings.
/// Replaces characters that would break fw_cfg syntax (commas, equals, control chars)
/// with underscores. SECURITY: CWE-78 (command injection prevention).
fn sanitize_fw_cfg_value(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c == ',' || c == '=' || c == '\'' || c == '"' || c == '\\' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// Validate a CPU feature name: must contain only alphanumeric, hyphens, underscores, dots.
/// SECURITY: CWE-20 (Improper Input Validation) — prevents confusing libvirt with crafted names.
fn is_valid_cpu_feature_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Generate the deterministic VNC password for a VM from its UUID.
/// SECURITY: VNC password derived from VM UUID using FNV-1a hash (CWE-330).
/// VNC protocol limits passwords to 8 characters. The password only protects
/// against local-machine unauthorized access since VNC is bound to 127.0.0.1.
pub fn generate_vnc_password(uuid: &uuid::Uuid) -> String {
    let uuid_bytes = uuid.as_bytes();
    let mut hash: u64 = 0xcbf29ce484222325; // FNV-1a offset basis
    for &b in uuid_bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3); // FNV-1a prime
    }
    let charset = b"0123456789abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ";
    (0..8)
        .map(|i| {
            let idx = ((hash >> (i * 8)) & 0xFF) as usize % charset.len();
            charset[idx] as char
        })
        .collect()
}

/// Build a complete libvirt domain XML string from a VmConfig.
/// Architecture-aware: uses config.qemu_arch and config.machine_type
/// to adapt the XML for cross-architecture emulation.
pub fn build_domain_xml(config: &VmConfig) -> String {
    let uuid = config.id;
    let arch = config.qemu_arch.qemu_suffix();
    let machine_type_raw = if config.machine_type.is_empty() {
        config.qemu_arch.default_machine().to_string()
    } else {
        // Sanitize: machine types should only contain alphanumeric, hyphens, underscores, dots, commas
        let sanitized: String = config
            .machine_type
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.' || *c == ',')
            .collect();
        sanitized
    };
    let machine_type = &machine_type_raw;

    // SECURITY (CWE-20): Clamp numeric values to sane ranges.
    // Prevents resource exhaustion and invalid XML from tampered configs.
    let vcpus = config.vcpus.min(1024).max(1); // libvirt max is typically 710 for QEMU
    let memory_mib = config.memory_mib.min(16_777_216).max(64); // max 16 TiB, min 64 MiB
    let io_threads = config.io_threads.min(64); // QEMU limit is typically 64

    // Determine emulation mode: KVM for same-arch, QEMU/TCG for cross-arch
    let domain_type = if config.use_kvm && config.qemu_arch.can_use_kvm_on_x86() {
        "kvm"
    } else {
        "qemu"
    };

    let mut xml = String::with_capacity(4096);

    // Always declare the qemu namespace — needed for fw_cfg passthrough (LibreUEFI)
    // and custom QEMU args.
    let ns_attr = " xmlns:qemu='http://libvirt.org/schemas/domain/qemu/1.0'";

    // Domain element — with optional qemu namespace
    xml.push_str(&format!("<domain type='{}'{}>\n", domain_type, ns_attr));
    xml.push_str(&format!(
        r#"  <name>{name}</name>
  <uuid>{uuid}</uuid>
  <metadata>
    <librevmm:config xmlns:librevmm="https://libre-vmm.org/xmlns">
      <librevmm:managed>true</librevmm:managed>
      <librevmm:box_type>{box_type}</librevmm:box_type>
      <librevmm:arch>{qemu_arch}</librevmm:arch>
    </librevmm:config>
  </metadata>
  <memory unit='MiB'>{memory}</memory>
  <currentMemory unit='MiB'>{memory}</currentMemory>
  <vcpu placement='static'>{vcpus}</vcpu>
"#,
        name = xml_escape(&config.name),
        uuid = xml_escape(&uuid.to_string()),
        box_type = xml_escape(config.box_type.display_name()),
        qemu_arch = xml_escape(config.qemu_arch.qemu_suffix()),
        memory = memory_mib,
        vcpus = vcpus,
    ));

    // SMBIOS injection — LibreUEFI branding in every VM
    // Generate a per-VM serial number from the VM name (first 8 chars uppercased)
    let serial_suffix: String = config
        .name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(8)
        .collect::<String>()
        .to_uppercase();
    let serial = format!(
        "LBRVMM-{}",
        if serial_suffix.is_empty() {
            "UNKNOWN".to_string()
        } else {
            serial_suffix
        }
    );
    if config.os_type == OsType::MacOS {
        // macOS needs Mac-like SMBIOS to load correct drivers
        xml.push_str(&format!(
            r#"  <sysinfo type='smbios'>
    <system>
      <entry name='manufacturer'>Apple Inc.</entry>
      <entry name='product'>iMacPro1,1</entry>
      <entry name='version'>1.0</entry>
      <entry name='serial'>{serial}</entry>
      <entry name='uuid'>{uuid}</entry>
      <entry name='family'>iMac Pro</entry>
    </system>
  </sysinfo>
"#,
            serial = xml_escape(&serial),
            uuid = xml_escape(&uuid.to_string()),
        ));
    } else {
        xml.push_str(&format!(
            r#"  <sysinfo type='smbios'>
    <bios>
      <entry name='vendor'>LibreUEFI</entry>
      <entry name='version'>LibreUEFI 1.0</entry>
    </bios>
    <system>
      <entry name='manufacturer'>Libre VMM Project</entry>
      <entry name='product'>LibreUEFI Virtual Machine</entry>
      <entry name='version'>1.0</entry>
      <entry name='serial'>{serial}</entry>
      <entry name='uuid'>{uuid}</entry>
      <entry name='family'>Virtual Machine</entry>
    </system>
  </sysinfo>
"#,
            serial = xml_escape(&serial),
            uuid = xml_escape(&uuid.to_string()),
        ));
    }

    // OS section
    xml.push_str(&format!(
        r#"  <os>
    <type arch='{arch}' machine='{machine_type}'>hvm</type>
    <smbios mode='sysinfo'/>
"#,
        arch = arch,
        machine_type = machine_type,
    ));

    if config.uefi {
        // Use custom firmware paths if set and valid, otherwise fall back to arch defaults
        let firmware_paths = resolve_firmware_paths(config);
        if let Some((code, vars)) = firmware_paths {
            xml.push_str(&format!(
                "    <loader readonly='yes' type='pflash'>{}</loader>\n",
                code
            ));
            xml.push_str(&format!("    <nvram template='{}'/>\n", vars));
        }
    }

    // Boot order — driven by config
    for device in &config.boot_order {
        xml.push_str(&format!("    <boot dev='{}'/>\n", device.xml_name()));
    }
    // Boot menu with configurable timeout
    let boot_timeout = config.boot_timeout.min(60000); // cap at 60 seconds
    xml.push_str(&format!(
        "    <bootmenu enable='yes' timeout='{}'/>\n",
        boot_timeout
    ));
    xml.push_str("  </os>\n");

    // Features — ACPI/APIC for x86, GIC for ARM virt
    xml.push_str("  <features>\n    <acpi/>\n    <apic/>\n");
    if config.os_type == OsType::Windows && config.qemu_arch.can_use_kvm_on_x86() {
        xml.push_str("    <hyperv mode='passthrough'>\n");
        xml.push_str("      <relaxed state='on'/>\n");
        xml.push_str("      <vapic state='on'/>\n");
        xml.push_str("      <spinlocks state='on' retries='8191'/>\n");
        xml.push_str("      <vpindex state='on'/>\n");
        xml.push_str("      <runtime state='on'/>\n");
        xml.push_str("      <frequencies state='on'/>\n");
        xml.push_str("    </hyperv>\n");
    }
    xml.push_str("  </features>\n");

    // Resource limits: cputune + memtune (before CPU element)
    if config.resource_limits.has_any() {
        xml.push_str(&config.resource_limits.to_xml());
    }

    // CPU — macOS needs custom Penryn model, not host-passthrough
    if config.os_type == OsType::MacOS && config.qemu_arch.can_use_kvm_on_x86() {
        xml.push_str("  <cpu mode='custom' match='exact'>\n");
        xml.push_str("    <model>Penryn</model>\n");
        xml.push_str("    <feature policy='require' name='invtsc'/>\n");
        xml.push_str("    <feature policy='require' name='ssse3'/>\n");
        xml.push_str("    <feature policy='require' name='sse4.2'/>\n");
        xml.push_str("    <feature policy='require' name='popcnt'/>\n");
        xml.push_str("    <feature policy='require' name='avx'/>\n");
        xml.push_str("    <feature policy='require' name='aes'/>\n");
        xml.push_str("    <feature policy='require' name='xsave'/>\n");
        xml.push_str("    <feature policy='require' name='xsaveopt'/>\n");
        // User-added features
        for feature in &config.cpu_features {
            if is_valid_cpu_feature_name(feature) {
                xml.push_str(&format!(
                    "    <feature policy='require' name='{}'/>\n",
                    xml_escape(feature)
                ));
            }
        }
        if let Some(ref topo) = config.cpu_topology {
            xml.push_str(&topo.to_xml());
        }
        xml.push_str("  </cpu>\n");
    } else
    // CPU — host-passthrough for KVM, explicit model for cross-arch emulation
    if config.use_kvm && config.qemu_arch.can_use_kvm_on_x86() {
        xml.push_str("  <cpu mode='host-passthrough' check='none' migratable='on'>\n");
        // CPU topology
        if let Some(ref topo) = config.cpu_topology {
            xml.push_str(&topo.to_xml());
        }
        // CPU feature flags (KVM host-passthrough can still add/remove features)
        // SECURITY (CWE-20): Validate feature names contain only safe characters.
        for feature in &config.cpu_features {
            if is_valid_cpu_feature_name(feature) {
                xml.push_str(&format!(
                    "    <feature policy='require' name='{}'/>\n",
                    xml_escape(feature)
                ));
            } else {
                tracing::warn!("Blocked invalid CPU feature name: '{}'", feature);
            }
        }
        // Wave 11.4 — Side-channel mitigations toggle (CWE-1037).
        // When disabled, remove Spectre/SSBD/MDS mitigation MSRs for ~10-30% perf.
        // SECURITY: Only safe when guest is trusted (no untrusted workloads inside).
        if !config.side_channel_mitigations {
            xml.push_str("    <feature policy='disable' name='spec-ctrl'/>\n");
            xml.push_str("    <feature policy='disable' name='ssbd'/>\n");
            xml.push_str("    <feature policy='disable' name='md-clear'/>\n");
        }
        xml.push_str("  </cpu>\n");
    } else {
        // Cross-architecture emulation: specify CPU model
        let cpu_model = if config.cpu_model.is_empty() {
            config.qemu_arch.default_cpu().to_string()
        } else {
            config.cpu_model.clone()
        };
        xml.push_str(&format!(
            "  <cpu mode='custom' match='exact'>\n    <model>{}</model>\n",
            xml_escape(&cpu_model)
        ));
        if let Some(ref topo) = config.cpu_topology {
            xml.push_str(&topo.to_xml());
        }
        // CPU feature flags — enable specific ISA extensions
        // SECURITY (CWE-20): Validate feature names contain only safe characters.
        for feature in &config.cpu_features {
            if is_valid_cpu_feature_name(feature) {
                xml.push_str(&format!(
                    "    <feature policy='require' name='{}'/>\n",
                    xml_escape(feature)
                ));
            } else {
                tracing::warn!("Blocked invalid CPU feature name: '{}'", feature);
            }
        }
        xml.push_str("  </cpu>\n");
    }

    // Memory backing — hugepages (Power User feature)
    if config.hugepages {
        xml.push_str("  <memoryBacking>\n    <hugepages/>\n    <nosharepages/>\n    <locked/>\n  </memoryBacking>\n");
    }

    // I/O threads (Power User feature) — must be before <devices>
    if io_threads > 0 {
        xml.push_str(&format!("  <iothreads>{}</iothreads>\n", io_threads));
    }

    // Clock — x86-specific timers, simpler for other architectures
    if config.qemu_arch.can_use_kvm_on_x86() || matches!(config.qemu_arch, QemuArch::I386) {
        if config.os_type == OsType::Windows {
            xml.push_str("  <clock offset='localtime'>\n");
            xml.push_str("    <timer name='rtc' tickpolicy='catchup'/>\n");
            xml.push_str("    <timer name='pit' tickpolicy='delay'/>\n");
            xml.push_str("    <timer name='hpet' present='no'/>\n");
            xml.push_str("    <timer name='hypervclock' present='yes'/>\n");
            xml.push_str("  </clock>\n");
        } else {
            xml.push_str("  <clock offset='utc'>\n");
            xml.push_str("    <timer name='rtc' tickpolicy='catchup'/>\n");
            xml.push_str("    <timer name='pit' tickpolicy='delay'/>\n");
            xml.push_str("    <timer name='hpet' present='no'/>\n");
            xml.push_str("  </clock>\n");
        }
    } else {
        xml.push_str("  <clock offset='utc'/>\n");
    }

    // Power management
    xml.push_str("  <on_poweroff>destroy</on_poweroff>\n");
    xml.push_str("  <on_reboot>restart</on_reboot>\n");
    xml.push_str("  <on_crash>destroy</on_crash>\n");

    // Devices
    xml.push_str("  <devices>\n");
    xml.push_str(&format!(
        "    <emulator>{}</emulator>\n",
        config.qemu_arch.qemu_binary()
    ));

    // Disk bus — depends on architecture, machine type, and OS
    let disk_bus = if (config.os_type == OsType::Windows || config.os_type == OsType::MacOS)
        && config.qemu_arch.can_use_kvm_on_x86()
    {
        "sata"
    } else {
        config.qemu_arch.default_disk_bus(machine_type)
    };

    // Disk target device name depends on bus type
    let disk_dev = match disk_bus {
        "virtio" => "vda",
        "sata" => "sda",
        "scsi" => "sda",
        "ide" => "hda",
        _ => "vda",
    };
    // CDROM device names — avoid collision with main disk when both use SATA
    let (cdrom_dev, cdrom2_dev) = if disk_bus == "sata" {
        ("sdb", "sdc")
    } else {
        ("sda", "sdb")
    };
    // Disk driver — include cache, io, and iothread attributes for Power Users
    // Allowlist valid values to prevent XML injection via config tampering
    let valid_cache = [
        "default",
        "none",
        "writethrough",
        "writeback",
        "directsync",
        "unsafe",
    ];
    let cache_attr = if config.disk_cache.is_empty()
        || config.disk_cache == "writeback"
        || !valid_cache.contains(&config.disk_cache.as_str())
    {
        String::new()
    } else {
        format!(" cache='{}'", config.disk_cache)
    };
    let valid_io = ["default", "native", "threads", "io_uring"];
    let io_attr = if config.disk_io_mode.is_empty()
        || config.disk_io_mode == "threads"
        || !valid_io.contains(&config.disk_io_mode.as_str())
    {
        String::new()
    } else {
        format!(" io='{}'", config.disk_io_mode)
    };
    let iothread_attr = if io_threads > 0 {
        " iothread='1'".to_string()
    } else {
        String::new()
    };

    // SECURITY (CWE-22, CWE-59): Validate disk path against symlinks, traversal,
    // and sensitive directories — same validation as ISO paths.
    let safe_disk_path = validate_disk_path(&config.disk_path);
    if safe_disk_path.is_none() {
        tracing::error!(
            "BLOCKED unsafe disk path for VM '{}': {}",
            config.name,
            config.disk_path
        );
        // Return a minimal error XML that libvirt will reject gracefully,
        // rather than silently generating XML with an unsafe path.
        return format!(
            "<domain type='qemu'><name>{}</name><memory unit='MiB'>64</memory>\
             <vcpu>1</vcpu><os><type>hvm</type></os></domain>\n",
            xml_escape(&config.name)
        );
    }
    let safe_disk_path = safe_disk_path.unwrap();

    xml.push_str(&format!(
        r#"    <disk type='file' device='disk'>
      <driver name='qemu' type='qcow2' discard='unmap'{cache}{io}{iothread}/>
      <source file='{disk_path}'/>
      <target dev='{disk_dev}' bus='{disk_bus}'/>
"#,
        cache = cache_attr,
        io = io_attr,
        iothread = iothread_attr,
        disk_path = xml_escape(&safe_disk_path),
        disk_dev = disk_dev,
        disk_bus = disk_bus,
    ));
    // Wave 11.3 — Independent disk modes
    // - Snapshotted (default): no extra XML
    // - IndependentPersistent: emit a marker comment (Wave 11.3 needs snapshot.rs
    //   integration to exclude the disk from snapshot definitions).
    // - IndependentNonpersistent: emit <transient/>; QEMU/libvirt creates an overlay
    //   that's discarded on power-off (sandbox mode).
    match config.disk_mode {
        DiskMode::Snapshotted => {},
        DiskMode::IndependentPersistent => {
            // TODO(Wave 11.3): snapshot.rs must skip this disk in snapshot definitions.
            xml.push_str("      <!-- librevmm-disk-mode: persistent -->\n");
        },
        DiskMode::IndependentNonpersistent => {
            xml.push_str("      <transient/>\n");
        },
    }
    // Disk I/O throttle
    if config.resource_limits.disk_io.has_any() {
        xml.push_str(&config.resource_limits.disk_iotune_xml());
    }
    // LUKS encryption — reference the libvirt secret if disk is encrypted
    if config.disk_encrypted {
        xml.push_str(&format!(
            r#"      <encryption format='luks'>
        <secret type='passphrase' uuid='{uuid}'/>
      </encryption>
"#,
            uuid = config.id,
        ));
    }
    xml.push_str("    </disk>\n");

    // CDROM (ISO)
    // SECURITY: Validate ISO path to prevent symlink/traversal attacks (CWE-59)
    if let Some(ref iso) = config.iso_path {
        let iso_safe = validate_iso_path(iso);
        if let Some(safe_iso) = iso_safe {
            xml.push_str(&format!(
                r#"    <disk type='file' device='cdrom'>
      <driver name='qemu' type='raw'/>
      <source file='{iso}'/>
      <target dev='{cdrom}' bus='sata'/>
      <readonly/>
    </disk>
"#,
                iso = xml_escape(&safe_iso),
                cdrom = cdrom_dev,
            ));
        } else {
            tracing::warn!("Blocked unsafe ISO path: {}", iso);
        }
    }

    // VirtIO drivers ISO for Windows
    if config.os_type == OsType::Windows {
        xml.push_str(&format!(
            r#"    <disk type='file' device='cdrom'>
      <driver name='qemu' type='raw'/>
      <target dev='{cdrom2}' bus='sata'/>
      <readonly/>
    </disk>
"#,
            cdrom2 = cdrom2_dev,
        ));
    }

    // Network — supports multiple NICs via effective_nics().
    // Wave 12.5: when firewall_rules is non-empty, attach a per-VM nwfilter
    // reference. The host-side filter must be defined separately (see
    // `build_nwfilter_xml` + `firewall_filter_name`).
    // Wave 12.5 follow-up: auto-create matching nwfilter via
    // `virsh nwfilter-define` on VM start so users don't do it manually.
    let firewall_filter = if !config.firewall_rules.is_empty() {
        Some(firewall_filter_name(&config.id))
    } else {
        None
    };
    for nic in config.effective_nics() {
        build_nic_xml(
            &mut xml,
            &nic,
            &config.resource_limits,
            firewall_filter.as_deref(),
        );
    }

    // Display protocol — determined by the per-VM display_protocol setting.
    // SPICE provides clipboard, audio, USB redirect, and display auto-resize.
    // VNC is stable/universal and supports noVNC browser access.
    let proto = config.display_protocol;
    if proto.has_spice() && config.qemu_arch.has_spice_support() {
        xml.push_str("    <graphics type='spice' autoport='yes'>\n");
        xml.push_str("      <listen type='address' address='127.0.0.1'/>\n");
        xml.push_str("      <image compression='auto_glz'/>\n");
        xml.push_str("      <streaming mode='filter'/>\n");
        xml.push_str("      <clipboard copypaste='yes'/>\n");
        xml.push_str("      <mouse mode='client'/>\n");
        xml.push_str("    </graphics>\n");
        xml.push_str("    <channel type='spicevmc'>\n");
        xml.push_str("      <target type='virtio' name='com.redhat.spice.0'/>\n");
        xml.push_str("    </channel>\n");
    }
    if proto.has_vnc() || !config.qemu_arch.has_spice_support() {
        // VNC — bound to localhost only, no password.
        xml.push_str("    <graphics type='vnc' autoport='yes' listen='127.0.0.1'/>\n");
    }

    // Video — user-selectable GPU model
    if config.qemu_arch.has_virtio_support() {
        let libvirt_model = config.gpu_model.libvirt_model(&config.os_type);
        if libvirt_model != "none" {
            // Wave 11.5 — display heads bumped 4 → 8.
            let heads = config.display_count.max(1).min(8);
            let accel3d = if config.gpu_accel
                && config.gpu_model.supports_3d()
                && libvirt_model == "virtio"
            {
                "yes"
            } else {
                "no"
            };
            // Convert MiB to KiB for libvirt XML
            let vram_kib = (config.video_ram_mb as u64) * 1024;
            xml.push_str(&format!(
                r#"    <video>
      <model type='{model}' heads='{heads}' primary='yes' vram='{vram}'>
        <acceleration accel3d='{accel3d}'/>
      </model>
    </video>
"#,
                model = libvirt_model,
                heads = heads,
                vram = vram_kib,
                accel3d = accel3d,
            ));
        }
    }

    // Audio — architecture-aware
    if config.audio && config.qemu_arch.has_audio_support() {
        let sound_model = config
            .qemu_arch
            .default_sound_device(machine_type)
            .unwrap_or("ich9");
        // Route audio through SPICE when it's the active display protocol.
        // Falls back to "none" when using VNC-only or exotic arches.
        let audio_type = if proto.has_spice() && config.qemu_arch.has_spice_support() {
            "spice"
        } else {
            "none"
        };
        xml.push_str(&format!(
            r#"    <sound model='{model}'>
      <audio id='1'/>
    </sound>
    <audio id='1' type='{audio_type}'/>
"#,
            model = sound_model,
            audio_type = audio_type,
        ));
    }

    // USB — user-selectable controller version
    if config.usb_support && config.qemu_arch.has_usb_support() {
        let usb_model = config.usb_controller.libvirt_model();
        xml.push_str(&format!(
            "    <controller type='usb' model='{}' ports='8'/>\n    <input type='tablet' bus='usb'/>\n",
            usb_model,
        ));
        // USB redirection via SPICE (only when SPICE is the active protocol)
        if proto.has_spice() && config.qemu_arch.has_spice_support() {
            xml.push_str("    <redirdev bus='usb' type='spicevmc'/>\n");
            xml.push_str("    <redirdev bus='usb' type='spicevmc'/>\n");
        }
    }

    // Shared folder via virtiofs
    // SECURITY: Validate shared folder path to prevent host filesystem escape
    if let Some(ref folder) = config.shared_folder {
        let folder_path = std::path::Path::new(folder);
        // SECURITY (CWE-59): Resolve symlinks and validate the REAL path.
        // If canonicalize fails (path doesn't exist), we MUST reject it —
        // a non-existent path could be a TOCTOU setup where an attacker creates
        // a symlink between validation and use.
        let resolved = match folder_path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    "Blocked shared folder path (cannot resolve, possible TOCTOU symlink attack): {} — error: {}",
                    folder, e
                );
                // SECURITY (CWE-367): Do NOT fall through with an empty path.
                // A non-existent path could be a TOCTOU setup where an attacker
                // creates a symlink between validation and use. Reject entirely.
                std::path::PathBuf::new()
            },
        };
        let resolved_str = resolved.to_string_lossy();
        // If canonicalize failed, resolved_str is empty — skip the entire shared folder
        if resolved_str.is_empty() {
            // Already logged above; do not emit any XML for this folder
        } else {
            let is_safe = folder_path.is_absolute()
                && !folder.contains("..")
                && resolved_str != "/"
                && !resolved_str.starts_with("/etc")
                && !resolved_str.starts_with("/root")
                && !resolved_str.starts_with("/proc")
                && !resolved_str.starts_with("/sys")
                && !resolved_str.starts_with("/dev")
                && !resolved_str.starts_with("/boot")
                && !resolved_str.starts_with("/run")
                && !resolved_str.starts_with("/var/run")
                && !resolved_str.starts_with("/var/log")
                && !resolved_str.starts_with("/var/lib")
                && !resolved_str.starts_with("/var/spool")
                && !resolved_str.starts_with("/var/tmp")
                && !resolved_str.starts_with("/tmp")
                && !resolved_str.starts_with("/usr")
                && !resolved_str.starts_with("/sbin")
                && !resolved_str.starts_with("/bin")
                && !resolved_str.starts_with("/snap")
                && !resolved_str.starts_with("/lost+found");

            if is_safe {
                // SECURITY: CWE-91 — Validate virtiofs target name is alphanumeric
                // to prevent XML injection. Currently hardcoded but validated defensively.
                let target_name = "shared";
                if !target_name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                {
                    tracing::warn!("Blocked invalid virtiofs target name: {}", target_name);
                } else {
                    xml.push_str(&format!(
                        r#"    <filesystem type='mount' accessmode='mapped'>
      <driver type='virtiofs'/>
      <source dir='{folder}'/>
      <target dir='{target}'/>
    </filesystem>
"#,
                        folder = xml_escape(&resolved_str),
                        target = xml_escape(target_name),
                    ));
                }
            } else {
                tracing::warn!(
                    "Blocked unsafe shared folder path: {} (resolved to: {})",
                    folder,
                    resolved_str
                );
            }
        }
    }

    // TPM device (swtpm emulation — essential for Windows 11)
    if config.tpm_enabled {
        let tpm_state = crate::tpm::TpmState::new(&config.id, config.tpm_version.clone());
        if let Ok(state) = tpm_state {
            xml.push_str(&crate::tpm::tpm_device_xml(&state));
        }
    }

    // Guest agent channel, memory balloon, and RNG — only for architectures with virtio
    if config.qemu_arch.has_virtio_support() {
        xml.push_str(
            r#"    <channel type='unix'>
      <target type='virtio' name='org.qemu.guest_agent.0'/>
    </channel>
    <memballoon model='virtio'/>
    <rng model='virtio'>
      <backend model='random'>/dev/urandom</backend>
    </rng>
"#,
        );
    }

    // Wave 11.6 — Serial / parallel ports.
    // Emitted after the virtio channels so they appear adjacent to libvirt's
    // auto-generated <console> element in the final domain XML.
    for (idx, port) in config.serial_ports.iter().take(4).enumerate() {
        build_serial_xml(&mut xml, idx, port);
    }
    for (idx, port) in config.parallel_ports.iter().take(3).enumerate() {
        build_parallel_xml(&mut xml, idx, port);
    }

    // VFIO PCI passthrough devices (Power User feature)
    for vfio_dev in &config.vfio_devices {
        if let Some((domain, bus, slot, function)) = parse_pci_address(&vfio_dev.pci_address) {
            xml.push_str(&format!(
                r#"    <hostdev mode='subsystem' type='pci' managed='yes'>
      <source>
        <address domain='0x{domain}' bus='0x{bus}' slot='0x{slot}' function='0x{function}'/>
      </source>
      <rom bar='{rom_bar}'/>
    </hostdev>
"#,
                domain = domain,
                bus = bus,
                slot = slot,
                function = function,
                rom_bar = if vfio_dev.rom_bar { "on" } else { "off" },
            ));
        }
    }

    // Looking Glass IVSHMEM device for near-zero-latency GPU passthrough display
    if config.looking_glass.enabled && !config.vfio_devices.is_empty() {
        xml.push_str(&crate::looking_glass::ivshmem_xml(
            config.looking_glass.ivshmem_size_mib,
        ));
    }

    xml.push_str("  </devices>\n");

    // Security labeling: use 'dynamic' relabeling so libvirt's security driver
    // (SELinux/AppArmor) can confine the QEMU process while still allowing
    // access to user disk images. The 'relabel' flag tells libvirt to
    // adjust file labels as needed rather than requiring pre-labeled files.
    // NOTE: Previously used type='none' which disabled ALL mandatory access
    // control — a critical security gap (CVE-class: CWE-693).
    xml.push_str("  <seclabel type='dynamic' relabel='yes'/>\n");

    // QEMU command-line arguments: merge custom user args + LibreUEFI fw_cfg passthrough.
    // All fw_cfg and custom args go into a single <qemu:commandline> block.
    xml.push_str("  <qemu:commandline>\n");

    // --- Custom QEMU command-line arguments (Power User feature) ---
    // SECURITY: Use ALLOWLIST approach — only permit known-safe argument prefixes.
    // Blocklist approach is fundamentally flawed (CWE-184) since new QEMU args are
    // added regularly and any missed arg can enable host escape.
    if !config.custom_qemu_args.is_empty() {
        // SECURITY: STRICT allowlist — only permit QEMU args that cannot escape the VM (CWE-184).
        // REMOVED (host escape risk):
        //   -device     — can mount host filesystem via virtio-9p/virtiofs with security_model=none
        //   -global     — can override any QEMU device property, disable security features
        //   -smbios     — can read arbitrary host files for SMBIOS data
        //   -machine    — can change machine type and security properties
        // Device passthrough MUST go through structured config fields (vfio_devices)
        // with PCI address validation, not raw QEMU args.
        let allowed_prefixes = [
            "-cpu",
            "-smp",
            "-m ",
            "-audiodev",
            "-display",
            "-usb",
            "-usbdevice",
            "-overcommit",
            "-rtc",
            "-boot",
            "-accel",
            "-no-hpet",
            "-no-shutdown",
            "-no-reboot",
            "-enable-kvm",
        ];

        for arg in &config.custom_qemu_args {
            let arg_trimmed = arg.trim();
            let is_allowed = allowed_prefixes
                .iter()
                .any(|prefix| arg_trimmed.starts_with(prefix));
            if !is_allowed {
                tracing::warn!("Blocked non-allowlisted QEMU arg: {}", arg);
                continue;
            }
            let parts: Vec<&str> = arg_trimmed.splitn(2, ' ').collect();
            if parts.len() == 2 {
                xml.push_str(&format!(
                    "    <qemu:arg value='{}'/>\n",
                    xml_escape(parts[0])
                ));
                xml.push_str(&format!(
                    "    <qemu:arg value='{}'/>\n",
                    xml_escape(parts[1])
                ));
            } else {
                xml.push_str(&format!(
                    "    <qemu:arg value='{}'/>\n",
                    xml_escape(arg_trimmed)
                ));
            }
        }
    }

    // --- LibreUEFI fw_cfg passthrough ---
    // Pass VM metadata via QEMU fw_cfg so LibreUEFI firmware can read it.
    let safe_vm_name = sanitize_fw_cfg_value(&config.name);
    xml.push_str("    <qemu:arg value='-fw_cfg'/>\n");
    xml.push_str(&format!(
        "    <qemu:arg value='name=opt/libre-vmm/vm-name,string={}'/>\n",
        xml_escape(&safe_vm_name)
    ));
    xml.push_str("    <qemu:arg value='-fw_cfg'/>\n");
    xml.push_str(&format!(
        "    <qemu:arg value='name=opt/libre-vmm/boot-timeout,string={}'/>\n",
        boot_timeout
    ));

    // Preferred display resolution (optional)
    if let Some((width, height)) = config.preferred_resolution {
        // Clamp to sane values
        let w = width.max(640).min(7680);
        let h = height.max(480).min(4320);
        xml.push_str("    <qemu:arg value='-fw_cfg'/>\n");
        xml.push_str(&format!(
            "    <qemu:arg value='name=opt/libre-vmm/display-width,string={}'/>\n",
            w
        ));
        xml.push_str("    <qemu:arg value='-fw_cfg'/>\n");
        xml.push_str(&format!(
            "    <qemu:arg value='name=opt/libre-vmm/display-height,string={}'/>\n",
            h
        ));
    }

    // Battery reporting preference — passed via fw_cfg so LibreUEFI firmware
    // can conditionally enable/disable the battery SSDT in ACPI tables.
    if config.report_battery {
        xml.push_str("    <qemu:arg value='-fw_cfg'/>\n");
        xml.push_str("    <qemu:arg value='name=opt/libre-vmm/report-battery,string=1'/>\n");
    }

    // macOS: Apple SMC emulation device with OSK key + global compatibility settings
    if config.os_type == OsType::MacOS && config.qemu_arch.can_use_kvm_on_x86() {
        // Apple SMC emulation (required for macOS boot)
        xml.push_str("    <qemu:arg value='-device'/>\n");
        xml.push_str("    <qemu:arg value='isa-applesmc,osk=ourhardworkbythesewordsguardedpleasedontsteal(c)AppleComputerInc'/>\n");
        // Global settings for macOS compatibility
        xml.push_str("    <qemu:arg value='-global'/>\n");
        xml.push_str("    <qemu:arg value='kvm-pit.lost_tick_policy=delay'/>\n");
        xml.push_str("    <qemu:arg value='-global'/>\n");
        xml.push_str("    <qemu:arg value='ICH9-LPC.acpi-pci-hotplug-with-bridge-support=off'/>\n");
    }

    xml.push_str("  </qemu:commandline>\n");

    xml.push_str("</domain>\n");

    xml
}

/// Build XML for a single network interface.
/// Validate a MAC address format (XX:XX:XX:XX:XX:XX where XX is hex).
fn is_valid_mac(mac: &str) -> bool {
    let parts: Vec<&str> = mac.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn build_nic_xml(
    xml: &mut String,
    nic: &NicConfig,
    limits: &crate::resource_limits::ResourceLimits,
    firewall_filter: Option<&str>,
) {
    // SECURITY: Allowlist valid NIC models to prevent arbitrary device instantiation (CWE-20)
    let valid_models = [
        "virtio", "e1000", "e1000e", "rtl8139", "ne2k_pci", "pcnet", "vmxnet3",
    ];
    let safe_model = if valid_models.contains(&nic.model.as_str()) {
        nic.model.clone()
    } else {
        tracing::warn!("Invalid NIC model '{}', defaulting to virtio", nic.model);
        "virtio".to_string()
    };
    match &nic.mode {
        NetworkMode::Nat => {
            xml.push_str(&format!(
                r#"    <interface type='network'>
      <source network='default'/>
      <model type='{model}'/>
"#,
                model = xml_escape(&safe_model),
            ));
        },
        NetworkMode::Bridged => {
            xml.push_str(&format!(
                r#"    <interface type='bridge'>
      <source bridge='br0'/>
      <model type='{model}'/>
"#,
                model = xml_escape(&safe_model),
            ));
        },
        NetworkMode::HostOnly => {
            xml.push_str(&format!(
                r#"    <interface type='network'>
      <source network='isolated'/>
      <model type='{model}'/>
"#,
                model = xml_escape(&safe_model),
            ));
        },
        NetworkMode::LanSegment(name) => {
            // Wave 11.2 — isolated VM-to-VM network. VMs sharing the same
            // segment name can talk to each other but to nothing else (including the host).
            // TODO(Wave 11.2): Auto-create the libvirt network `libre-vmm-lan-{sanitized}`
            // here so users don't have to define it manually via `network_editor`.
            // Currently we emit the XML assuming the network already exists.
            let sanitized = crate::config::sanitize_lan_segment_name(name);
            xml.push_str(&format!(
                r#"    <interface type='network'>
      <source network='libre-vmm-lan-{seg}'/>
      <model type='{model}'/>
"#,
                seg = xml_escape(&sanitized),
                model = xml_escape(&safe_model),
            ));
        },
        NetworkMode::None => return,
    }
    // Optional MAC address — validate format to prevent duplicates and injection
    if !nic.mac.is_empty() {
        if is_valid_mac(&nic.mac) {
            xml.push_str(&format!(
                "      <mac address='{}'/>\n",
                xml_escape(&nic.mac),
            ));
        } else {
            tracing::warn!(
                "Invalid MAC address '{}', letting libvirt auto-generate",
                nic.mac
            );
        }
    }
    // Network bandwidth limits
    if limits.network.has_any() {
        xml.push_str(&limits.network_bandwidth_xml());
    }
    // Wave 12.5 — per-VM firewall via libvirt nwfilter (generates nftables rules).
    if let Some(name) = firewall_filter {
        xml.push_str(&format!(
            "      <filterref filter='{}'/>\n",
            xml_escape(name),
        ));
    }
    xml.push_str("    </interface>\n");
}

/// Build the libvirt nwfilter name for a given VM. Uses the first 8 hex
/// characters of the VM's UUID for a short, stable identifier.
pub fn firewall_filter_name(id: &uuid::Uuid) -> String {
    let s = id.simple().to_string();
    // Take first 8 chars for a short prefix.
    let prefix: String = s.chars().take(8).collect();
    format!("librevmm-fw-{}", prefix)
}

/// Generate a libvirt nwfilter XML for the given rules. The caller is
/// responsible for `virsh nwfilter-define`-ing this XML before starting the VM.
///
/// The filter is `chain='root'` so it sits at the top of the per-interface
/// filter chain. Each rule is emitted as a `<rule>` element with the protocol
/// element (`<tcp/>`, `<udp/>`, `<icmp/>`, or `<all/>`) containing optional
/// `srcipaddr`/`dstipaddr` and port range attributes.
///
/// SECURITY (CWE-91): All user-controlled fields are validated by
/// `validate_config_bounds` AND XML-escaped here. The `description` field
/// is deliberately NOT emitted (libvirt has no place for it in nwfilter XML),
/// so user notes can't leak into the rule.
pub fn build_nwfilter_xml(filter_name: &str, rules: &[FirewallRule]) -> String {
    let mut xml = String::new();
    xml.push_str("<filter name='");
    xml.push_str(&xml_escape(filter_name));
    xml.push_str("' chain='root'>\n");
    for rule in rules {
        // Per-rule sanitization is defensive: caller should have already
        // validated, but we re-check so a stray malformed value can never
        // produce broken XML.
        let addr = if crate::config::is_valid_firewall_addr(&rule.remote_addr) {
            rule.remote_addr.as_str()
        } else {
            ""
        };
        let lport = if crate::config::is_valid_firewall_port(&rule.local_port) {
            rule.local_port.as_str()
        } else {
            ""
        };
        let rport = if crate::config::is_valid_firewall_port(&rule.remote_port) {
            rule.remote_port.as_str()
        } else {
            ""
        };
        let priority = rule.priority.max(0).min(1000);

        xml.push_str(&format!(
            "  <rule action='{}' direction='{}' priority='{}'>\n",
            rule.action.libvirt_action(),
            rule.direction.libvirt_direction(),
            priority,
        ));
        let elem = rule.protocol.libvirt_element();
        // Build optional attributes for the protocol element.
        let mut attrs = String::new();
        // Remote address → srcipaddr (with /CIDR splitting if present).
        if !addr.is_empty() {
            let (ip, mask) = split_addr_mask(addr);
            attrs.push_str(&format!(" srcipaddr='{}'", xml_escape(ip)));
            if let Some(m) = mask {
                attrs.push_str(&format!(" srcipmask='{}'", xml_escape(m)));
            }
        }
        if rule.protocol.has_ports() {
            if let Some((s, e)) = port_range(lport) {
                attrs.push_str(&format!(" dstportstart='{}'", xml_escape(s)));
                if let Some(end) = e {
                    attrs.push_str(&format!(" dstportend='{}'", xml_escape(end)));
                }
            }
            if let Some((s, e)) = port_range(rport) {
                attrs.push_str(&format!(" srcportstart='{}'", xml_escape(s)));
                if let Some(end) = e {
                    attrs.push_str(&format!(" srcportend='{}'", xml_escape(end)));
                }
            }
        }
        xml.push_str(&format!("    <{}{}/>\n", elem, attrs));
        xml.push_str("  </rule>\n");
    }
    xml.push_str("</filter>\n");
    xml
}

/// Split an "addr" or "addr/mask" CIDR-style string into (ip, optional mask).
fn split_addr_mask(s: &str) -> (&str, Option<&str>) {
    match s.split_once('/') {
        Some((ip, mask)) => (ip, Some(mask)),
        None => (s, None),
    }
}

/// Parse a port specification: returns (start, optional end) for single ports
/// or "start-end" ranges. Returns None for empty/invalid input.
fn port_range(s: &str) -> Option<(&str, Option<&str>)> {
    if s.is_empty() {
        return None;
    }
    match s.split_once('-') {
        Some((start, end)) if !start.is_empty() && !end.is_empty() => Some((start, Some(end))),
        Some(_) => None,
        None => Some((s, None)),
    }
}

/// Validate a serial/parallel port target path or address.
/// SECURITY (CWE-22, CWE-91):
/// - File / UnixSocket: must be absolute, no ".." components, no null bytes.
/// - Tcp: must be "host:port" without control chars / quotes; minimal sanity check.
/// - Pty / Null: target is ignored.
/// Returns None for invalid targets.
fn validate_port_target(backend: SerialBackend, target: &str) -> Option<String> {
    if target.contains('\0') {
        return None;
    }
    match backend {
        SerialBackend::Pty | SerialBackend::Null => Some(String::new()),
        SerialBackend::File | SerialBackend::UnixSocket => {
            if target.is_empty() {
                return None;
            }
            let p = std::path::Path::new(target);
            if !p.is_absolute() {
                return None;
            }
            for component in p.components() {
                if matches!(component, std::path::Component::ParentDir) {
                    return None;
                }
            }
            Some(target.to_string())
        },
        SerialBackend::Tcp => {
            if target.is_empty() {
                return None;
            }
            // Disallow quote/angle/control chars that could break XML.
            if target.chars().any(|c| {
                c.is_control() || c == '<' || c == '>' || c == '\'' || c == '"' || c == '&'
            }) {
                return None;
            }
            // Expect "host:port" — at least one ':' and a port portion that parses.
            let parts: Vec<&str> = target.rsplitn(2, ':').collect();
            if parts.len() != 2 {
                return None;
            }
            if parts[0].parse::<u16>().is_err() {
                return None;
            }
            Some(target.to_string())
        },
    }
}

fn build_serial_xml(xml: &mut String, idx: usize, port: &SerialPortConfig) {
    let target = match validate_port_target(port.backend, &port.target) {
        Some(t) => t,
        None => {
            tracing::warn!(
                "Blocked invalid serial port target (port {}, backend {}): {}",
                idx,
                port.backend,
                port.target
            );
            return;
        },
    };
    let type_str = port.backend.libvirt_type();
    xml.push_str(&format!("    <serial type='{}'>\n", type_str));
    match port.backend {
        SerialBackend::Pty | SerialBackend::Null => {},
        SerialBackend::File => {
            xml.push_str(&format!(
                "      <source path='{}' append='on'/>\n",
                xml_escape(&target)
            ));
        },
        SerialBackend::UnixSocket => {
            xml.push_str(&format!(
                "      <source mode='bind' path='{}'/>\n",
                xml_escape(&target)
            ));
        },
        SerialBackend::Tcp => {
            // Parse "host:port" — we already validated that the port parses.
            let last_colon = target.rfind(':').unwrap_or(0);
            let (host, port_part) = target.split_at(last_colon);
            let port_only = &port_part[1..];
            xml.push_str(&format!(
                "      <source mode='connect' host='{}' service='{}'/>\n      <protocol type='raw'/>\n",
                xml_escape(host),
                xml_escape(port_only),
            ));
        },
    }
    xml.push_str(&format!(
        "      <target type='isa-serial' port='{}'/>\n",
        idx
    ));
    xml.push_str("    </serial>\n");
}

fn build_parallel_xml(xml: &mut String, idx: usize, port: &ParallelPortConfig) {
    let target = match validate_port_target(port.backend, &port.target) {
        Some(t) => t,
        None => {
            tracing::warn!(
                "Blocked invalid parallel port target (port {}, backend {}): {}",
                idx,
                port.backend,
                port.target
            );
            return;
        },
    };
    let type_str = port.backend.libvirt_type();
    xml.push_str(&format!("    <parallel type='{}'>\n", type_str));
    match port.backend {
        SerialBackend::Pty | SerialBackend::Null => {},
        SerialBackend::File => {
            xml.push_str(&format!(
                "      <source path='{}' append='on'/>\n",
                xml_escape(&target)
            ));
        },
        SerialBackend::UnixSocket => {
            xml.push_str(&format!(
                "      <source mode='bind' path='{}'/>\n",
                xml_escape(&target)
            ));
        },
        SerialBackend::Tcp => {
            let last_colon = target.rfind(':').unwrap_or(0);
            let (host, port_part) = target.split_at(last_colon);
            let port_only = &port_part[1..];
            xml.push_str(&format!(
                "      <source mode='connect' host='{}' service='{}'/>\n      <protocol type='raw'/>\n",
                xml_escape(host),
                xml_escape(port_only),
            ));
        },
    }
    xml.push_str(&format!("      <target port='{}'/>\n", idx));
    xml.push_str("    </parallel>\n");
}

/// Validate a PCI address component is valid hex.
fn is_valid_pci_hex(s: &str, max_len: usize) -> bool {
    !s.is_empty() && s.len() <= max_len && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Parse a PCI address like "0000:01:00.0" into (domain, bus, slot, function).
/// SECURITY: Validates all components are valid hex to prevent XML injection (CWE-91).
fn parse_pci_address(addr: &str) -> Option<(String, String, String, String)> {
    // Format: DDDD:BB:SS.F
    let parts: Vec<&str> = addr.split(':').collect();
    if parts.len() == 3 {
        let domain = parts[0].to_string();
        let bus = parts[1].to_string();
        let sf: Vec<&str> = parts[2].split('.').collect();
        if sf.len() == 2 {
            let slot = sf[0].to_string();
            let function = sf[1].to_string();
            // Validate all parts are hex
            if is_valid_pci_hex(&domain, 4)
                && is_valid_pci_hex(&bus, 2)
                && is_valid_pci_hex(&slot, 2)
                && is_valid_pci_hex(&function, 1)
            {
                return Some((domain, bus, slot, function));
            }
            return None;
        }
    }
    // Try without domain: BB:SS.F
    if parts.len() == 2 {
        let bus = parts[0].to_string();
        let sf: Vec<&str> = parts[1].split('.').collect();
        if sf.len() == 2 {
            let slot = sf[0].to_string();
            let function = sf[1].to_string();
            if is_valid_pci_hex(&bus, 2)
                && is_valid_pci_hex(&slot, 2)
                && is_valid_pci_hex(&function, 1)
            {
                return Some(("0000".to_string(), bus, slot, function));
            }
        }
    }
    None
}

/// Validate an ISO path for safety: must be absolute, no symlinks, no traversal.
/// Returns the validated path or None if unsafe.
/// SECURITY: CWE-22 (Path Traversal), CWE-59 (Symlink Following)
fn validate_iso_path(path: &str) -> Option<String> {
    // Reuse the same validation logic as disk paths
    validate_disk_path(path)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(name: &str) -> VmConfig {
        VmConfig {
            name: name.into(),
            disk_path: "/home/test/vm-disks/test.qcow2".into(),
            ..VmConfig::default()
        }
    }

    #[test]
    fn test_generate_linux_xml() {
        let config = test_config("Test Linux VM");
        let xml = build_domain_xml(&config);
        assert!(xml.contains("<name>Test Linux VM</name>"));
        assert!(xml.contains("type='kvm'"));
        assert!(xml.contains("virtio"));
    }

    #[test]
    fn test_generate_windows_xml() {
        let config = VmConfig {
            os_type: OsType::Windows,
            ..test_config("Test Windows VM")
        };
        let xml = build_domain_xml(&config);
        assert!(xml.contains("hyperv"));
        assert!(xml.contains("localtime"));
    }

    #[test]
    fn test_generate_arm64_xml() {
        use crate::qemu_archs::{BoxType, QemuArch};
        let config = VmConfig {
            box_type: BoxType::HardwareLab,
            qemu_arch: QemuArch::Aarch64,
            machine_type: "virt".into(),
            cpu_model: "cortex-a72".into(),
            use_kvm: false,
            ..test_config("Test ARM64 VM")
        };
        let xml = build_domain_xml(&config);
        assert!(
            xml.contains("type='qemu'"),
            "ARM64 should use QEMU type not KVM"
        );
        assert!(xml.contains("qemu-system-aarch64"));
        assert!(xml.contains("cortex-a72"));
        assert!(xml.contains("machine='virt'"));
    }

    #[test]
    fn test_generate_riscv_xml() {
        use crate::qemu_archs::{BoxType, QemuArch};
        let config = VmConfig {
            box_type: BoxType::HardwareLab,
            qemu_arch: QemuArch::Riscv64,
            machine_type: "virt".into(),
            use_kvm: false,
            ..test_config("Test RISC-V VM")
        };
        let xml = build_domain_xml(&config);
        assert!(xml.contains("type='qemu'"));
        assert!(xml.contains("qemu-system-riscv64"));
    }

    // ───── Wave 11.2 — LAN segments ────────────────────────────────

    #[test]
    fn lan_segment_emits_libre_vmm_lan_network() {
        let mut config = test_config("Test LAN VM");
        config.network = NetworkMode::LanSegment("lab-frontend".to_string());
        let xml = build_domain_xml(&config);
        assert!(
            xml.contains("<source network='libre-vmm-lan-lab-frontend'/>"),
            "expected sanitized LAN segment network reference, got:\n{}",
            xml
        );
    }

    #[test]
    fn lan_segment_sanitizes_unsafe_chars() {
        let mut config = test_config("Test LAN VM");
        config.network = NetworkMode::LanSegment("Lab/Front<end>".to_string());
        let xml = build_domain_xml(&config);
        // Sanitization replaces / and < > with -, then collapses runs.
        assert!(
            xml.contains("<source network='libre-vmm-lan-lab-front-end'/>"),
            "expected sanitized name, got:\n{}",
            xml
        );
        // Raw < > / must not leak into the XML interface source.
        assert!(!xml.contains("libre-vmm-lan-Lab/Front<end>"));
    }

    // ───── Wave 11.3 — DiskMode ─────────────────────────────────────

    #[test]
    fn disk_mode_snapshotted_no_marker() {
        let mut config = test_config("Snap VM");
        config.disk_mode = crate::config::DiskMode::Snapshotted;
        let xml = build_domain_xml(&config);
        assert!(!xml.contains("<transient/>"));
        assert!(!xml.contains("librevmm-disk-mode"));
    }

    #[test]
    fn disk_mode_independent_persistent_marker_comment() {
        let mut config = test_config("PersistentVM");
        config.disk_mode = crate::config::DiskMode::IndependentPersistent;
        let xml = build_domain_xml(&config);
        assert!(
            xml.contains("librevmm-disk-mode: persistent"),
            "expected persistent marker comment, got:\n{}",
            xml
        );
        assert!(!xml.contains("<transient/>"));
    }

    #[test]
    fn disk_mode_independent_nonpersistent_emits_transient() {
        let mut config = test_config("NonPersistVM");
        config.disk_mode = crate::config::DiskMode::IndependentNonpersistent;
        let xml = build_domain_xml(&config);
        assert!(
            xml.contains("<transient/>"),
            "expected <transient/> for nonpersistent disk, got:\n{}",
            xml
        );
        assert!(!xml.contains("librevmm-disk-mode"));
    }

    // ───── Wave 11.4 — Side-channel mitigations ─────────────────────

    #[test]
    fn side_channel_mitigations_on_no_disable_features() {
        let mut config = test_config("Safe VM");
        config.side_channel_mitigations = true;
        let xml = build_domain_xml(&config);
        assert!(!xml.contains("name='spec-ctrl'"));
        assert!(!xml.contains("name='ssbd'"));
        assert!(!xml.contains("name='md-clear'"));
    }

    #[test]
    fn side_channel_mitigations_off_emits_disable_features() {
        let mut config = test_config("FastVM");
        config.side_channel_mitigations = false;
        // Ensure KVM/x86 path is taken (test_config defaults already set this).
        let xml = build_domain_xml(&config);
        assert!(
            xml.contains("<feature policy='disable' name='spec-ctrl'/>"),
            "expected spec-ctrl disable feature, got:\n{}",
            xml
        );
        assert!(xml.contains("<feature policy='disable' name='ssbd'/>"));
        assert!(xml.contains("<feature policy='disable' name='md-clear'/>"));
    }

    // ───── Wave 11.5 — display heads bump 4 → 8 ─────────────────────

    #[test]
    fn display_count_eight_heads_in_xml() {
        let mut config = test_config("MultiHeadVM");
        config.display_count = 8;
        let xml = build_domain_xml(&config);
        assert!(
            xml.contains("heads='8'"),
            "expected heads='8' in XML, got:\n{}",
            xml
        );
    }

    #[test]
    fn display_count_clamped_at_eight_in_xml() {
        let mut config = test_config("OverHeadVM");
        config.display_count = 20;
        let xml = build_domain_xml(&config);
        // xml_builder clamps to 8 even if validate_config_bounds didn't run.
        assert!(xml.contains("heads='8'"));
        assert!(!xml.contains("heads='20'"));
    }

    // ───── Wave 11.6 — Serial / Parallel ports ──────────────────────

    #[test]
    fn serial_port_pty_emits_basic_xml() {
        use crate::config::{SerialBackend, SerialPortConfig};
        let mut config = test_config("SerialVM");
        config.serial_ports.push(SerialPortConfig {
            backend: SerialBackend::Pty,
            target: String::new(),
        });
        let xml = build_domain_xml(&config);
        assert!(
            xml.contains("<serial type='pty'>"),
            "expected pty serial port, got:\n{}",
            xml
        );
        assert!(xml.contains("<target type='isa-serial' port='0'/>"));
        assert!(xml.contains("</serial>"));
    }

    #[test]
    fn serial_port_file_emits_source_path() {
        use crate::config::{SerialBackend, SerialPortConfig};
        let mut config = test_config("SerialFileVM");
        config.serial_ports.push(SerialPortConfig {
            backend: SerialBackend::File,
            target: "/var/log/vm-serial0.log".to_string(),
        });
        let xml = build_domain_xml(&config);
        assert!(xml.contains("<serial type='file'>"));
        assert!(xml.contains("<source path='/var/log/vm-serial0.log' append='on'/>"));
    }

    #[test]
    fn serial_port_unix_socket_emits_source_path() {
        use crate::config::{SerialBackend, SerialPortConfig};
        let mut config = test_config("SerialUnixVM");
        config.serial_ports.push(SerialPortConfig {
            backend: SerialBackend::UnixSocket,
            target: "/tmp/vm-serial0.sock".to_string(),
        });
        let xml = build_domain_xml(&config);
        assert!(xml.contains("<serial type='unix'>"));
        assert!(xml.contains("<source mode='bind' path='/tmp/vm-serial0.sock'/>"));
    }

    #[test]
    fn serial_port_tcp_emits_host_port() {
        use crate::config::{SerialBackend, SerialPortConfig};
        let mut config = test_config("SerialTcpVM");
        config.serial_ports.push(SerialPortConfig {
            backend: SerialBackend::Tcp,
            target: "127.0.0.1:4555".to_string(),
        });
        let xml = build_domain_xml(&config);
        assert!(xml.contains("<serial type='tcp'>"));
        assert!(xml.contains("host='127.0.0.1'"));
        assert!(xml.contains("service='4555'"));
    }

    #[test]
    fn serial_port_null_emits_minimal_xml() {
        use crate::config::{SerialBackend, SerialPortConfig};
        let mut config = test_config("SerialNullVM");
        config.serial_ports.push(SerialPortConfig {
            backend: SerialBackend::Null,
            target: String::new(),
        });
        let xml = build_domain_xml(&config);
        assert!(xml.contains("<serial type='null'>"));
        assert!(xml.contains("<target type='isa-serial' port='0'/>"));
    }

    #[test]
    fn serial_port_rejects_relative_path() {
        use crate::config::{SerialBackend, SerialPortConfig};
        let mut config = test_config("SerialBadVM");
        config.serial_ports.push(SerialPortConfig {
            backend: SerialBackend::File,
            target: "relative/path.log".to_string(),
        });
        let xml = build_domain_xml(&config);
        // Should not emit the serial port at all.
        assert!(!xml.contains("relative/path.log"));
        assert!(!xml.contains("<serial type='file'>"));
    }

    #[test]
    fn serial_port_rejects_traversal() {
        use crate::config::{SerialBackend, SerialPortConfig};
        let mut config = test_config("SerialTravVM");
        config.serial_ports.push(SerialPortConfig {
            backend: SerialBackend::File,
            target: "/var/log/../etc/shadow".to_string(),
        });
        let xml = build_domain_xml(&config);
        assert!(!xml.contains("shadow"));
        assert!(!xml.contains("<serial type='file'>"));
    }

    #[test]
    fn serial_port_rejects_bad_tcp_target() {
        use crate::config::{SerialBackend, SerialPortConfig};
        let mut config = test_config("SerialBadTcpVM");
        config.serial_ports.push(SerialPortConfig {
            backend: SerialBackend::Tcp,
            target: "not-a-host-port".to_string(),
        });
        let xml = build_domain_xml(&config);
        assert!(!xml.contains("<serial type='tcp'>"));
    }

    #[test]
    fn parallel_port_pty_emits_basic_xml() {
        use crate::config::{ParallelPortConfig, SerialBackend};
        let mut config = test_config("ParallelVM");
        config.parallel_ports.push(ParallelPortConfig {
            backend: SerialBackend::Pty,
            target: String::new(),
        });
        let xml = build_domain_xml(&config);
        assert!(xml.contains("<parallel type='pty'>"));
        assert!(xml.contains("<target port='0'/>"));
        assert!(xml.contains("</parallel>"));
    }

    #[test]
    fn parallel_port_file_emits_source_path() {
        use crate::config::{ParallelPortConfig, SerialBackend};
        let mut config = test_config("ParallelFileVM");
        config.parallel_ports.push(ParallelPortConfig {
            backend: SerialBackend::File,
            target: "/var/log/vm-lp0.log".to_string(),
        });
        let xml = build_domain_xml(&config);
        assert!(xml.contains("<parallel type='file'>"));
        assert!(xml.contains("<source path='/var/log/vm-lp0.log' append='on'/>"));
    }

    #[test]
    fn parallel_port_truncates_to_three() {
        use crate::config::{ParallelPortConfig, SerialBackend};
        let mut config = test_config("ManyParallelVM");
        for _ in 0..5 {
            config.parallel_ports.push(ParallelPortConfig {
                backend: SerialBackend::Pty,
                target: String::new(),
            });
        }
        let xml = build_domain_xml(&config);
        // Builder takes only the first 3.
        let count = xml.matches("<parallel type='pty'>").count();
        assert_eq!(
            count, 3,
            "expected 3 parallel ports, got {}:\n{}",
            count, xml
        );
    }

    // ───── Wave 12.5 — Per-VM firewall (nwfilter) ───────────────────

    fn fw_rule(
        action: crate::config::FirewallAction,
        proto: crate::config::FirewallProtocol,
        local_port: &str,
    ) -> FirewallRule {
        FirewallRule {
            action,
            direction: crate::config::FirewallDirection::In,
            protocol: proto,
            remote_addr: String::new(),
            local_port: local_port.to_string(),
            remote_port: String::new(),
            priority: 100,
            description: String::new(),
        }
    }

    #[test]
    fn firewall_filter_name_uses_uuid_prefix() {
        let id = uuid::Uuid::parse_str("abc12345-0000-0000-0000-000000000000").unwrap();
        let name = firewall_filter_name(&id);
        assert_eq!(name, "librevmm-fw-abc12345");
    }

    #[test]
    fn nic_xml_no_filterref_without_rules() {
        let config = test_config("NoFwVM");
        let xml = build_domain_xml(&config);
        assert!(!xml.contains("<filterref"));
    }

    #[test]
    fn nic_xml_emits_filterref_when_rules_present() {
        use crate::config::{FirewallAction, FirewallProtocol};
        let mut config = test_config("FwVM");
        config
            .firewall_rules
            .push(fw_rule(FirewallAction::Accept, FirewallProtocol::Tcp, "22"));
        let xml = build_domain_xml(&config);
        let expected = format!("<filterref filter='{}'/>", firewall_filter_name(&config.id));
        assert!(
            xml.contains(&expected),
            "expected filterref in interface XML, got:\n{}",
            xml
        );
    }

    #[test]
    fn nwfilter_xml_empty_rules_produces_empty_filter() {
        let xml = build_nwfilter_xml("librevmm-fw-test", &[]);
        assert!(xml.contains("<filter name='librevmm-fw-test' chain='root'>"));
        assert!(xml.contains("</filter>"));
        assert!(!xml.contains("<rule"));
    }

    #[test]
    fn nwfilter_xml_tcp_rule_well_formed() {
        use crate::config::{FirewallAction, FirewallProtocol};
        let rules = vec![fw_rule(FirewallAction::Accept, FirewallProtocol::Tcp, "22")];
        let xml = build_nwfilter_xml("librevmm-fw-x", &rules);
        assert!(xml.contains("<rule action='accept' direction='in' priority='100'>"));
        assert!(xml.contains("<tcp dstportstart='22'/>"));
        assert!(xml.contains("</rule>"));
        assert!(xml.contains("</filter>"));
    }

    #[test]
    fn nwfilter_xml_udp_rule_with_port_range() {
        use crate::config::{FirewallAction, FirewallProtocol};
        let mut rule = fw_rule(FirewallAction::Drop, FirewallProtocol::Udp, "1000-2000");
        rule.priority = 250;
        let xml = build_nwfilter_xml("librevmm-fw-x", &[rule]);
        assert!(xml.contains("action='drop'"));
        assert!(xml.contains("priority='250'"));
        assert!(
            xml.contains("<udp dstportstart='1000' dstportend='2000'/>"),
            "expected port range in UDP rule, got:\n{}",
            xml
        );
    }

    #[test]
    fn nwfilter_xml_icmp_rule_no_ports() {
        use crate::config::{FirewallAction, FirewallProtocol};
        let rules = vec![fw_rule(FirewallAction::Reject, FirewallProtocol::Icmp, "")];
        let xml = build_nwfilter_xml("librevmm-fw-x", &rules);
        assert!(xml.contains("action='reject'"));
        assert!(xml.contains("<icmp/>"));
        // ICMP has no ports, so no port attrs even if specified.
        assert!(!xml.contains("dstportstart"));
        assert!(!xml.contains("srcportstart"));
    }

    #[test]
    fn nwfilter_xml_any_protocol_emits_all_element() {
        use crate::config::{FirewallAction, FirewallDirection, FirewallProtocol};
        let mut rule = fw_rule(FirewallAction::Drop, FirewallProtocol::Any, "");
        rule.direction = FirewallDirection::Both;
        let xml = build_nwfilter_xml("librevmm-fw-x", &[rule]);
        assert!(xml.contains("direction='inout'"));
        assert!(
            xml.contains("<all/>"),
            "expected <all/> for Any protocol, got:\n{}",
            xml
        );
    }

    #[test]
    fn nwfilter_xml_remote_addr_with_cidr_splits_into_ip_and_mask() {
        use crate::config::{FirewallAction, FirewallProtocol};
        let mut rule = fw_rule(FirewallAction::Accept, FirewallProtocol::Tcp, "443");
        rule.remote_addr = "10.0.0.0/8".to_string();
        let xml = build_nwfilter_xml("librevmm-fw-x", &[rule]);
        assert!(
            xml.contains("srcipaddr='10.0.0.0'"),
            "expected split srcipaddr, got:\n{}",
            xml
        );
        assert!(
            xml.contains("srcipmask='8'"),
            "expected srcipmask, got:\n{}",
            xml
        );
    }

    #[test]
    fn nwfilter_xml_remote_addr_without_cidr_just_ip() {
        use crate::config::{FirewallAction, FirewallProtocol};
        let mut rule = fw_rule(FirewallAction::Accept, FirewallProtocol::Tcp, "443");
        rule.remote_addr = "192.168.1.5".to_string();
        let xml = build_nwfilter_xml("librevmm-fw-x", &[rule]);
        assert!(xml.contains("srcipaddr='192.168.1.5'"));
        assert!(!xml.contains("srcipmask"));
    }

    #[test]
    fn nwfilter_xml_invalid_addr_is_dropped() {
        use crate::config::{FirewallAction, FirewallProtocol};
        let mut rule = fw_rule(FirewallAction::Accept, FirewallProtocol::Tcp, "22");
        // Bypass validate_config_bounds by injecting directly. The xml builder
        // must still defensively reject this.
        rule.remote_addr = "<inject>".to_string();
        let xml = build_nwfilter_xml("librevmm-fw-x", &[rule]);
        // The injected angle brackets must NOT appear unescaped, and the
        // srcipaddr attribute must be omitted entirely (defensive validation).
        assert!(!xml.contains("<inject>"));
        assert!(
            !xml.contains("srcipaddr"),
            "invalid addr should be dropped, got:\n{}",
            xml
        );
    }

    #[test]
    fn nwfilter_xml_priority_clamped() {
        use crate::config::{FirewallAction, FirewallProtocol};
        let mut rule = fw_rule(FirewallAction::Accept, FirewallProtocol::Tcp, "22");
        rule.priority = 99999;
        let xml = build_nwfilter_xml("librevmm-fw-x", &[rule]);
        assert!(xml.contains("priority='1000'"));
        let mut rule2 = fw_rule(FirewallAction::Accept, FirewallProtocol::Tcp, "22");
        rule2.priority = -50;
        let xml2 = build_nwfilter_xml("librevmm-fw-x", &[rule2]);
        assert!(xml2.contains("priority='0'"));
    }

    #[test]
    fn nwfilter_xml_filter_name_escaped() {
        // Even though firewall_filter_name produces safe names, the function
        // should escape the input defensively in case callers pass crafted strings.
        let xml = build_nwfilter_xml("evil&'name", &[]);
        assert!(!xml.contains("name='evil&'name'"));
        assert!(xml.contains("evil&amp;&apos;name"));
    }

    #[test]
    fn nwfilter_xml_multiple_rules_all_emitted() {
        use crate::config::{FirewallAction, FirewallProtocol};
        let rules = vec![
            fw_rule(FirewallAction::Accept, FirewallProtocol::Tcp, "22"),
            fw_rule(FirewallAction::Accept, FirewallProtocol::Tcp, "443"),
            fw_rule(FirewallAction::Drop, FirewallProtocol::Any, ""),
        ];
        let xml = build_nwfilter_xml("librevmm-fw-x", &rules);
        let rule_count = xml.matches("<rule ").count();
        assert_eq!(rule_count, 3);
    }

    #[test]
    fn serial_port_truncates_to_four() {
        use crate::config::{SerialBackend, SerialPortConfig};
        let mut config = test_config("ManySerialVM");
        for _ in 0..10 {
            config.serial_ports.push(SerialPortConfig {
                backend: SerialBackend::Pty,
                target: String::new(),
            });
        }
        let xml = build_domain_xml(&config);
        let count = xml.matches("<serial type='pty'>").count();
        assert_eq!(count, 4, "expected 4 serial ports, got {}:\n{}", count, xml);
    }
}
