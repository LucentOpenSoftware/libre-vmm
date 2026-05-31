//! OVA/OVF export and import support.
//!
//! OVA = tar archive containing OVF descriptor + VMDK disk(s).
//! Export: qcow2 → VMDK via qemu-img, generate OVF XML, TAR into .ova.
//! Import: untar, parse OVF, VMDK → qcow2, create VmConfig.

use crate::config::{NetworkMode, OsType, VmConfig, VmConfigIo};
use crate::connection::HypervisorConnection;
use crate::error::{VmmError, VmmResult};
use std::path::{Path, PathBuf};
use tracing::info;
use uuid::Uuid;

/// Maximum total extracted size for OVA imports: 256 GiB (CWE-400).
const MAX_OVA_EXTRACTED_SIZE: u64 = 256 * 1024 * 1024 * 1024;
/// Maximum number of entries in an OVA archive (CWE-400).
const MAX_OVA_ENTRIES: usize = 100;

/// Export a VM as an OVA file.
pub fn export_ova(config: &VmConfig, output_path: &str) -> VmmResult<()> {
    info!("Exporting VM '{}' to OVA: {}", config.name, output_path);

    // SECURITY: Use random UUID for temp dir, not config.id which is predictable (CWE-377).
    // An attacker who knows config.id could pre-create a symlink at the predictable path.
    let temp_dir = std::env::temp_dir().join(format!("libre-vmm-export-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir)?;
    // SECURITY: Set temp directory to owner-only before writing files (CWE-377)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temp_dir, std::fs::Permissions::from_mode(0o700));
    }

    let result = export_ova_inner(config, output_path, &temp_dir);

    // Clean up temp directory
    let _ = std::fs::remove_dir_all(&temp_dir);

    result
}

fn export_ova_inner(config: &VmConfig, output_path: &str, temp_dir: &Path) -> VmmResult<()> {
    let safe_name = sanitize_filename(&config.name);

    // 1. Convert qcow2 → VMDK
    let vmdk_name = format!("{}-disk1.vmdk", safe_name);
    let vmdk_path = temp_dir.join(&vmdk_name);
    info!("Converting disk to VMDK...");
    crate::disk::convert_disk(&config.disk_path, &vmdk_path.display().to_string(), "vmdk")?;

    let vmdk_size = std::fs::metadata(&vmdk_path).map(|m| m.len()).unwrap_or(0);

    // 2. Generate OVF descriptor
    let ovf_name = format!("{}.ovf", safe_name);
    let ovf_content = generate_ovf(config, &vmdk_name, vmdk_size);
    let ovf_path = temp_dir.join(&ovf_name);
    std::fs::write(&ovf_path, &ovf_content)?;

    // 3. Generate MF (manifest) file
    let mf_name = format!("{}.mf", safe_name);
    let mf_path = temp_dir.join(&mf_name);
    // Simple SHA256 manifest (skip for now — many importers don't require it)
    let mf_content = format!(
        "SHA256({})= {}\nSHA256({})= {}\n",
        ovf_name, "placeholder", vmdk_name, "placeholder",
    );
    std::fs::write(&mf_path, &mf_content)?;

    // 4. Create TAR archive (.ova)
    // SECURITY: CWE-732 — Create OVA file with restrictive permissions (0o600).
    // File::create() uses default umask which may leave files world-readable.
    // VM disk images may contain sensitive data (encryption keys, user data).
    #[cfg(unix)]
    let ova_file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(output_path)?
    };
    #[cfg(not(unix))]
    let ova_file = std::fs::File::create(output_path)?;
    let mut tar_builder = tar::Builder::new(ova_file);

    // OVF must be the first file in the archive
    tar_builder
        .append_path_with_name(&ovf_path, &ovf_name)
        .map_err(|e| VmmError::Other(format!("Failed to add OVF to OVA: {}", e)))?;
    tar_builder
        .append_path_with_name(&mf_path, &mf_name)
        .map_err(|e| VmmError::Other(format!("Failed to add MF to OVA: {}", e)))?;
    tar_builder
        .append_path_with_name(&vmdk_path, &vmdk_name)
        .map_err(|e| VmmError::Other(format!("Failed to add VMDK to OVA: {}", e)))?;

    tar_builder
        .finish()
        .map_err(|e| VmmError::Other(format!("Failed to finalize OVA: {}", e)))?;

    info!("OVA export complete: {}", output_path);
    Ok(())
}

/// Import a VM from an OVA file.
pub fn import_ova(
    conn: &HypervisorConnection,
    ova_path: &str,
    new_name: Option<&str>,
) -> VmmResult<VmConfig> {
    info!("Importing OVA: {}", ova_path);

    // Create temp directory for extraction
    let temp_dir = std::env::temp_dir().join(format!("libre-vmm-import-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir)?;
    // SECURITY: Set temp directory to owner-only before extracting files (CWE-377).
    // Without this, other local users could place symlinks in the directory
    // between creation and extraction.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| VmmError::Other(format!("Failed to secure temp directory: {}", e)))?;
    }

    let result = import_ova_inner(conn, ova_path, new_name, &temp_dir);

    // Clean up temp directory
    let _ = std::fs::remove_dir_all(&temp_dir);

    result
}

fn import_ova_inner(
    conn: &HypervisorConnection,
    ova_path: &str,
    new_name: Option<&str>,
    temp_dir: &Path,
) -> VmmResult<VmConfig> {
    // SECURITY: Single-pass entry-by-entry extraction with inline validation.
    // Previous two-pass approach (validate then re-open) had a TOCTOU race (CWE-367):
    // the archive is read from disk twice, and the second pass could see different entries.
    // Now we validate and extract each entry atomically in one pass.
    let ova_file = std::fs::File::open(ova_path)
        .map_err(|e| VmmError::Other(format!("Failed to open OVA: {}", e)))?;
    let mut archive = tar::Archive::new(ova_file);

    let canonical_temp = temp_dir
        .canonicalize()
        .map_err(|e| VmmError::Other(format!("Failed to resolve temp dir: {}", e)))?;

    let mut total_extracted: u64 = 0;
    let mut entry_count: usize = 0;

    for entry_result in archive
        .entries()
        .map_err(|e| VmmError::Other(format!("Failed to read OVA entries: {}", e)))?
    {
        let mut entry =
            entry_result.map_err(|e| VmmError::Other(format!("Bad OVA entry: {}", e)))?;

        // SECURITY: Enforce max entry count to prevent DoS (CWE-400)
        entry_count += 1;
        if entry_count > MAX_OVA_ENTRIES {
            return Err(VmmError::Other(format!(
                "OVA contains too many entries (>{}) — possible archive bomb",
                MAX_OVA_ENTRIES
            )));
        }

        // SECURITY: Reject symlinks, hardlinks, and non-regular entries (CWE-59).
        // OVA archives should only contain regular files (OVF, VMDK, MF).
        // Symlinks could point outside temp_dir; hardlinks could reference
        // arbitrary inodes.
        let entry_type = entry.header().entry_type();
        if !entry_type.is_file() {
            // Allow directories (they're harmless and sometimes present)
            if entry_type.is_dir() {
                continue;
            }
            return Err(VmmError::Other(format!(
                "OVA contains non-regular entry type {:?} — blocked for security (CWE-59)",
                entry_type
            )));
        }

        let entry_path = entry
            .path()
            .map_err(|e| VmmError::Other(format!("Bad OVA entry path: {}", e)))?
            .into_owned();

        // SECURITY: Path traversal prevention (CWE-22 / "Zip Slip").
        // Check for ".." components, absolute paths, and paths that escape temp_dir.
        if entry_path.is_absolute() {
            return Err(VmmError::Other(format!(
                "OVA contains absolute path entry: {} (CWE-22)",
                entry_path.display()
            )));
        }
        for component in entry_path.components() {
            match component {
                std::path::Component::ParentDir => {
                    return Err(VmmError::Other(format!(
                        "OVA contains path traversal entry: {} (CWE-22)",
                        entry_path.display()
                    )));
                },
                std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                    return Err(VmmError::Other(format!(
                        "OVA contains absolute/prefix path: {} (CWE-22)",
                        entry_path.display()
                    )));
                },
                _ => {},
            }
        }

        // SECURITY: Enforce max total extracted size to prevent disk exhaustion (CWE-400).
        let entry_size = entry
            .header()
            .size()
            .map_err(|e| VmmError::Other(format!("Cannot read entry size: {}", e)))?;
        total_extracted = total_extracted.saturating_add(entry_size);
        if total_extracted > MAX_OVA_EXTRACTED_SIZE {
            return Err(VmmError::Other(format!(
                "OVA extracted size exceeds {} bytes — possible archive bomb (CWE-400)",
                MAX_OVA_EXTRACTED_SIZE
            )));
        }

        // SECURITY: Flatten the path — only use the filename component.
        // OVA files should only contain flat files (no subdirectories).
        // This prevents any residual traversal via crafted directory components.
        let file_name = entry_path.file_name().ok_or_else(|| {
            VmmError::Other(format!(
                "OVA entry has no filename: {}",
                entry_path.display()
            ))
        })?;
        let dest_path = canonical_temp.join(file_name);

        // Final check: destination must be within temp_dir
        if !dest_path.starts_with(&canonical_temp) {
            return Err(VmmError::Other(format!(
                "OVA entry resolves outside temp dir: {} (CWE-22)",
                entry_path.display()
            )));
        }

        // SECURITY: Don't overwrite existing files (prevents tar entry ordering attacks)
        if dest_path.exists() {
            return Err(VmmError::Other(format!(
                "OVA contains duplicate filename: {} — possible attack",
                file_name.to_string_lossy()
            )));
        }

        // Extract this single entry to the validated destination
        let mut dest_file = std::fs::File::create(&dest_path).map_err(|e| {
            VmmError::Other(format!("Failed to create {}: {}", dest_path.display(), e))
        })?;

        // SECURITY: Set restrictive permissions on extracted files (CWE-732)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest_path, std::fs::Permissions::from_mode(0o600));
        }

        std::io::copy(&mut entry, &mut dest_file).map_err(|e| {
            VmmError::Other(format!("Failed to extract {}: {}", dest_path.display(), e))
        })?;
    }

    // 2. Find OVF and VMDK files
    let ovf_path = find_file_by_ext(temp_dir, "ovf")?;
    let vmdk_path = find_file_by_ext(temp_dir, "vmdk")?;

    // 3. Parse OVF to extract VM settings
    // SECURITY: Limit OVF file size to prevent XML parsing DoS (CWE-400).
    // OVF files are small XML descriptors; anything over 1 MiB is suspicious.
    let ovf_meta = std::fs::metadata(&ovf_path)
        .map_err(|e| VmmError::Other(format!("Failed to stat OVF: {}", e)))?;
    if ovf_meta.len() > 1024 * 1024 {
        return Err(VmmError::Other(format!(
            "OVF file is too large ({} bytes) — max 1 MiB (CWE-400)",
            ovf_meta.len()
        )));
    }
    let ovf_content = std::fs::read_to_string(&ovf_path)
        .map_err(|e| VmmError::Other(format!("Failed to read OVF: {}", e)))?;
    let parsed = parse_ovf(&ovf_content);

    // 4. Convert VMDK → qcow2
    let new_id = Uuid::new_v4();
    let disk_dir = VmConfig::default_vm_dir();
    std::fs::create_dir_all(&disk_dir)?;
    let qcow2_path = format!("{}/{}.qcow2", disk_dir, new_id);

    info!("Converting VMDK to qcow2...");
    crate::disk::convert_disk(&vmdk_path.display().to_string(), &qcow2_path, "qcow2")?;

    // Fix permissions
    fix_disk_permissions(&qcow2_path);

    // Get disk size
    let disk_info = crate::disk::disk_info(&qcow2_path)?;
    let disk_size_gib = disk_info.virtual_size / (1024 * 1024 * 1024);

    // 5. Build VmConfig
    let vm_name = crate::config::sanitize_vm_name(
        &new_name
            .map(String::from)
            .unwrap_or_else(|| parsed.name.clone()),
    );

    // SECURITY: Bound OVA-parsed values to prevent host DoS (CWE-20)
    let safe_cpus = parsed.cpus.max(1).min(256);
    let safe_memory = parsed.memory_mib.max(512).min(1024 * 1024); // max 1 TiB

    let config = VmConfig {
        id: new_id,
        name: vm_name,
        vcpus: safe_cpus,
        memory_mib: safe_memory,
        disk_size_gib: disk_size_gib.max(1),
        disk_path: qcow2_path,
        iso_path: None,
        os_type: parsed.os_type,
        uefi: false,
        gpu_accel: false,
        network: NetworkMode::Nat,
        display_protocol: crate::config::DisplayProtocol::default(),
        usb_support: true,
        audio: true,
        shared_folder: None,
        description: format!("Imported from {}", ova_path),
        boot_order: crate::config::default_boot_order_public(),
        network_interfaces: Vec::new(),
        autostart: false,
        tags: Vec::new(),
        folder: None,
        favorite: false,
        display_count: 1,
        disk_encrypted: false,
        encryption_secret_uuid: None,
        tpm_enabled: false,
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
    };

    // 6. Create VM in libvirt
    // SECURITY: Clean up orphaned disk if VM creation fails (CWE-459)
    if let Err(e) = conn.create_vm_from_existing(&config) {
        tracing::error!("VM creation failed, cleaning up converted disk: {}", e);
        let _ = std::fs::remove_file(&config.disk_path);
        return Err(e);
    }

    info!("OVA import complete: VM '{}'", config.name);
    Ok(config)
}

/// Generate OVF descriptor XML for export.
fn generate_ovf(config: &VmConfig, vmdk_name: &str, vmdk_size: u64) -> String {
    let os_type_id = match config.os_type {
        OsType::Linux => "101",
        OsType::Windows => "67",
        OsType::MacOS => "101",
        OsType::FreeBSD => "42",
        OsType::Other => "1",
    };

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope xmlns="http://schemas.dmtf.org/ovf/envelope/1"
          xmlns:rasd="http://schemas.dmtf.org/wbem/wscim/1/cim-schema/2/CIM_ResourceAllocationSettingData"
          xmlns:vssd="http://schemas.dmtf.org/wbem/wscim/1/cim-schema/2/CIM_VirtualSystemSettingData"
          xmlns:ovf="http://schemas.dmtf.org/ovf/envelope/1">
  <References>
    <File ovf:href="{vmdk_name}" ovf:id="file1" ovf:size="{vmdk_size}"/>
  </References>
  <DiskSection>
    <Info>Virtual disk information</Info>
    <Disk ovf:capacity="{disk_gib}" ovf:capacityAllocationUnits="byte * 2^30"
          ovf:diskId="vmdisk1" ovf:fileRef="file1" ovf:format="http://www.vmware.com/interfaces/specifications/vmdk.html#streamOptimized"/>
  </DiskSection>
  <NetworkSection>
    <Info>The list of logical networks</Info>
    <Network ovf:name="NAT">
      <Description>NAT network</Description>
    </Network>
  </NetworkSection>
  <VirtualSystem ovf:id="{name}">
    <Info>A virtual machine</Info>
    <Name>{name}</Name>
    <OperatingSystemSection ovf:id="{os_type_id}">
      <Info>The operating system</Info>
    </OperatingSystemSection>
    <VirtualHardwareSection>
      <Info>Virtual hardware requirements</Info>
      <System>
        <vssd:ElementName>Virtual Hardware Family</vssd:ElementName>
        <vssd:InstanceID>0</vssd:InstanceID>
        <vssd:VirtualSystemType>vmx-21</vssd:VirtualSystemType>
      </System>
      <Item>
        <rasd:AllocationUnits>hertz * 10^6</rasd:AllocationUnits>
        <rasd:Description>Number of Virtual CPUs</rasd:Description>
        <rasd:ElementName>{cpus} virtual CPU(s)</rasd:ElementName>
        <rasd:InstanceID>1</rasd:InstanceID>
        <rasd:ResourceType>3</rasd:ResourceType>
        <rasd:VirtualQuantity>{cpus}</rasd:VirtualQuantity>
      </Item>
      <Item>
        <rasd:AllocationUnits>byte * 2^20</rasd:AllocationUnits>
        <rasd:Description>Memory Size</rasd:Description>
        <rasd:ElementName>{memory_mib}MB of memory</rasd:ElementName>
        <rasd:InstanceID>2</rasd:InstanceID>
        <rasd:ResourceType>4</rasd:ResourceType>
        <rasd:VirtualQuantity>{memory_mib}</rasd:VirtualQuantity>
      </Item>
      <Item>
        <rasd:AddressOnParent>0</rasd:AddressOnParent>
        <rasd:ElementName>Hard Disk 1</rasd:ElementName>
        <rasd:HostResource>ovf:/disk/vmdisk1</rasd:HostResource>
        <rasd:InstanceID>3</rasd:InstanceID>
        <rasd:ResourceType>17</rasd:ResourceType>
      </Item>
    </VirtualHardwareSection>
  </VirtualSystem>
</Envelope>"#,
        vmdk_name = vmdk_name,
        vmdk_size = vmdk_size,
        disk_gib = config.disk_size_gib,
        name = xml_escape(&config.name),
        os_type_id = os_type_id,
        cpus = config.vcpus,
        memory_mib = config.memory_mib,
    )
}

/// Parsed OVF data.
struct ParsedOvf {
    name: String,
    cpus: u32,
    memory_mib: u64,
    os_type: OsType,
}

/// Simple OVF parser (extracts essential values via string matching).
fn parse_ovf(ovf: &str) -> ParsedOvf {
    let name = extract_xml_value(ovf, "Name").unwrap_or_else(|| "Imported VM".to_string());

    let mut cpus = 2u32;
    let mut memory_mib = 4096u64;

    // Parse Items — find CPU (ResourceType=3) and Memory (ResourceType=4)
    for item_block in ovf.split("<Item>").skip(1) {
        let end = item_block.find("</Item>").unwrap_or(item_block.len());
        let item = &item_block[..end];

        let res_type = extract_xml_value(item, "rasd:ResourceType")
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);

        let quantity = extract_xml_value(item, "rasd:VirtualQuantity")
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);

        match res_type {
            // SECURITY: CWE-681/CWE-190 — OVF CPU quantity is external data (u64).
            // Truncating to u32 could silently discard high bits from a malicious OVF.
            // Cap to a sane maximum (1024 vCPUs) to prevent nonsensical configs.
            3 => cpus = u32::try_from(quantity.min(1024)).unwrap_or(1), // CPU
            4 => memory_mib = quantity,                                 // Memory
            _ => {},
        }
    }

    // Try to determine OS type from OperatingSystemSection id
    let os_type = if let Some(os_section) = ovf.find("OperatingSystemSection") {
        let after = &ovf[os_section..];
        if let Some(id_str) = extract_attr(after, "ovf:id") {
            match id_str.as_str() {
                "67" | "69" | "70" | "112" => OsType::Windows,
                "42" => OsType::FreeBSD,
                "101" | "36" | "94" | "96" => OsType::Linux,
                _ => OsType::Other,
            }
        } else {
            OsType::Linux
        }
    } else {
        OsType::Linux
    };

    ParsedOvf {
        name,
        cpus,
        memory_mib,
        os_type,
    }
}

fn find_file_by_ext(dir: &Path, ext: &str) -> VmmResult<PathBuf> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_some_and(|e| e.to_ascii_lowercase() == ext)
        {
            return Ok(entry.path());
        }
    }
    Err(VmmError::Other(format!(
        "No .{} file found in OVA archive",
        ext
    )))
}

/// SECURITY (CWE-91): Escape all five XML special characters.
/// Missing apostrophe escaping could allow attribute breakout in single-quoted
/// XML attributes (e.g., ovf:id='{name}'), enabling XML injection.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn extract_xml_value(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml.get(start..)?.find(&close)? + start;
    // SECURITY: Bounds check before slicing (CWE-129)
    if start > end || end > xml.len() {
        return None;
    }
    Some(xml[start..end].to_string())
}

fn extract_attr(xml: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = xml.find(&pattern)? + pattern.len();
    let end = xml.get(start..)?.find('"')? + start;
    if start > end || end > xml.len() {
        return None;
    }
    Some(xml[start..end].to_string())
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// SECURITY (CWE-59, CWE-88): Fix disk permissions with symlink check and safe argument passing.
/// The previous implementation used metadata() which follows symlinks, so a symlink at
/// disk_path pointing to /etc/shadow would have its permissions changed.
/// Also, setfacl was called without `--` separator, allowing path injection as flags.
fn fix_disk_permissions(disk_path: &str) {
    use std::os::unix::fs::PermissionsExt;

    // SECURITY (CWE-59): Use symlink_metadata (lstat) to detect symlinks before
    // changing permissions. A symlink at disk_path could redirect permission
    // changes to arbitrary files (e.g., /etc/shadow).
    match std::fs::symlink_metadata(disk_path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            tracing::warn!("Refusing to fix permissions on symlink: {}", disk_path);
            return;
        },
        Ok(meta) => {
            let mut perms = meta.permissions();
            // SECURITY: Use 0o660 not 0o664 — disk images may contain sensitive guest data.
            // World-readable permissions expose VM contents to other local users (CWE-732).
            perms.set_mode(0o660);
            let _ = std::fs::set_permissions(disk_path, perms);
        },
        Err(_) => return,
    }

    // SECURITY (CWE-88): Use `--` to prevent disk_path from being interpreted as a flag.
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
    let _ = std::process::Command::new("setfacl")
        .args(["-m", "u:libvirt-qemu:rw", "--", disk_path])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
}
