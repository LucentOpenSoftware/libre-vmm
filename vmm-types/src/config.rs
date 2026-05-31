//! Pure data types from `vmm-core/src/config.rs`. No I/O, no platform code.
//!
//! ## Wave 16.A1 (Windows port foundation)
//! Extracted from vmm-core so vmm-gui can compile on any platform.
//!
//! The high-level VM configuration — what a non-technical user cares about.
//!
//! ## What stays in vmm-core::config
//!
//! - `VmConfig::save() / load() / list_all() / delete_config()` (filesystem)
//! - `VmConfig::config_dir() / default_vm_dir() / iso_dir()` (filesystem + `dirs`)
//! - `VmConfig::from_template() / from_arch() / for_power_user()` (depend on
//!   `template` / `qemu_archs` runtime helpers that aren't pure)
//! - `VmConfig::to_toml() / to_yaml() / save_toml() / save_yaml()` (touch disk)
//! - The atomic-write helper

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auto_snapshot::AutoSnapshotConfig;
use crate::looking_glass::LookingGlassConfig;
use crate::qemu_archs::{BoxType, QemuArch};
use crate::resource_limits::ResourceLimits;
use crate::tpm::TpmVersion;

/// High-level VM configuration — what a non-technical user cares about.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    /// Unique identifier
    pub id: Uuid,
    /// Human-readable name (e.g. "My Ubuntu Desktop")
    pub name: String,
    /// Number of virtual CPUs
    pub vcpus: u32,
    /// Memory in MiB
    pub memory_mib: u64,
    /// Disk size in GiB
    pub disk_size_gib: u64,
    /// Path to the disk image
    pub disk_path: String,
    /// ISO path for installation (optional after install)
    pub iso_path: Option<String>,
    /// Operating system type hint
    pub os_type: OsType,
    /// Enable UEFI boot (recommended for modern OSes)
    pub uefi: bool,
    /// Enable 3D acceleration
    pub gpu_accel: bool,
    /// Network mode (legacy — kept for backward compat, superseded by network_interfaces)
    pub network: NetworkMode,
    /// Display protocol used for the VM console.
    /// VNC is the stable default; SPICE adds clipboard, audio, USB redirect.
    #[serde(default, deserialize_with = "deserialize_display_protocol")]
    pub display_protocol: DisplayProtocol,
    /// Enable USB passthrough support
    pub usb_support: bool,
    /// Enable audio
    pub audio: bool,
    /// Shared folder path (optional)
    pub shared_folder: Option<String>,
    /// Description / notes
    pub description: String,

    // ===== Wave 1 additions =====
    /// Boot order — devices tried in sequence (default: cdrom, hd)
    #[serde(default = "default_boot_order")]
    pub boot_order: Vec<BootDevice>,

    /// Network interfaces — allows multiple NICs per VM
    /// If empty, the legacy `network` field is used to build a single NIC.
    #[serde(default)]
    pub network_interfaces: Vec<NicConfig>,

    /// Auto-start VM when the hypervisor starts
    #[serde(default)]
    pub autostart: bool,

    // ===== Wave 3 additions =====
    /// User-defined tags for organizing VMs (e.g., "development", "production")
    #[serde(default)]
    pub tags: Vec<String>,

    /// Folder/group name for organizing VMs in the sidebar
    #[serde(default)]
    pub folder: Option<String>,

    /// Whether this VM is marked as a favorite (pinned to top)
    #[serde(default)]
    pub favorite: bool,

    /// AutoProtect: scheduled auto-snapshot configuration.
    #[serde(default)]
    pub auto_snapshot: AutoSnapshotConfig,

    // ===== Wave 4 additions =====
    /// Number of display heads (1-8). More heads = more virtual monitors.
    #[serde(default = "default_display_count")]
    pub display_count: u8,

    /// Whether the disk image uses LUKS encryption.
    #[serde(default)]
    pub disk_encrypted: bool,
    /// UUID of the libvirt secret storing the disk encryption passphrase.
    #[serde(default)]
    pub encryption_secret_uuid: Option<uuid::Uuid>,

    // ===== Wave 6 additions =====
    /// Enable TPM emulation (via swtpm). Required for Windows 11.
    #[serde(default)]
    pub tpm_enabled: bool,

    /// TPM version to emulate (default: 2.0).
    #[serde(default)]
    pub tpm_version: TpmVersion,

    /// Port forwarding rules for NAT networking.
    /// Each rule maps a host port to a guest port.
    #[serde(default)]
    pub port_forwards: Vec<PortForwardRule>,

    /// VM notes (Markdown-formatted user notes).
    #[serde(default)]
    pub notes: String,

    // ===== Wave 6+ additions =====
    /// Resource limits (CPU pinning, memory tuning, I/O throttle, network bandwidth).
    #[serde(default)]
    pub resource_limits: ResourceLimits,

    /// Performance profile preset name (e.g., "default", "gaming", "development", "server").
    #[serde(default = "default_performance_profile")]
    pub performance_profile: String,

    // ===== Wave 7 additions (Parallels-inspired) =====
    /// Enable rollback mode (auto-snapshot before each VM start).
    #[serde(default)]
    pub rollback_enabled: bool,

    /// Maximum number of rollback points to keep.
    #[serde(default = "default_rollback_max_points")]
    pub rollback_max_points: usize,

    /// Active network condition preset name, or None for no conditioning.
    #[serde(default)]
    pub network_condition: Option<String>,

    // ===== Power User features (Box 3) =====
    /// CPU topology: sockets × cores × threads.
    /// If None, QEMU uses a flat topology (all vCPUs as separate sockets).
    #[serde(default)]
    pub cpu_topology: Option<CpuTopology>,

    /// Enable hugepages for memory allocation (reduces TLB misses).
    /// Maps to `<memoryBacking><hugepages/></memoryBacking>` in libvirt XML.
    #[serde(default)]
    pub hugepages: bool,

    /// Disk cache mode: none, writeback, writethrough, unsafe, directsync.
    /// "none" is best for data safety, "writeback" for performance.
    #[serde(default = "default_disk_cache")]
    pub disk_cache: String,

    /// Disk I/O mode: native, threads (for async I/O).
    #[serde(default = "default_disk_io")]
    pub disk_io_mode: String,

    /// Number of I/O threads for disk (0 = disabled).
    #[serde(default)]
    pub io_threads: u32,

    /// PCI devices for VFIO passthrough (e.g., GPU, NVMe, NIC).
    /// Each entry is a PCI address like "0000:01:00.0".
    #[serde(default)]
    pub vfio_devices: Vec<VfioDeviceConfig>,

    /// Looking Glass configuration for near-zero-latency GPU passthrough display.
    #[serde(default)]
    pub looking_glass: LookingGlassConfig,

    /// Custom QEMU command-line arguments (appended as `<qemu:arg>`).
    /// Power users can inject arbitrary QEMU flags.
    #[serde(default)]
    pub custom_qemu_args: Vec<String>,

    /// Enable virtio-mem (hot-pluggable memory, instead of dimm).
    #[serde(default)]
    pub virtio_mem: bool,

    /// Enable IO-uring for disk backend (Linux 5.1+).
    #[serde(default)]
    pub iouring: bool,

    /// CPU feature flags to enable (e.g., "avx2", "aes", "sve").
    /// Each entry maps to `<feature policy='require' name='...'/>` in libvirt XML.
    #[serde(default)]
    pub cpu_features: Vec<String>,

    // ===== Boxes system — multi-architecture support =====
    /// Which "Box" type this VM belongs to (Standard, HardwareLab, PowerUser).
    #[serde(default)]
    pub box_type: BoxType,

    /// Target CPU architecture (e.g., x86_64, aarch64, riscv64).
    /// Determines which qemu-system binary is used.
    #[serde(default)]
    pub qemu_arch: QemuArch,

    /// QEMU machine type (e.g., "q35", "virt", "malta").
    #[serde(default = "default_machine_type")]
    pub machine_type: String,

    /// QEMU CPU model override (e.g., "host", "cortex-a72", "qemu64").
    /// Empty string means use the architecture default.
    #[serde(default)]
    pub cpu_model: String,

    // ===== LibreUEFI integration =====
    /// Custom UEFI firmware code (OVMF_CODE) path override.
    /// If set, used instead of the architecture default.
    #[serde(default)]
    pub custom_firmware_code: Option<String>,

    /// Custom UEFI firmware vars (OVMF_VARS) template path override.
    /// If set, used instead of the architecture default.
    #[serde(default)]
    pub custom_firmware_vars: Option<String>,

    /// Boot menu timeout in milliseconds (0 = no timeout).
    #[serde(default = "default_boot_timeout")]
    pub boot_timeout: u32,

    /// Preferred display resolution (width, height) passed to LibreUEFI via fw_cfg.
    #[serde(default)]
    pub preferred_resolution: Option<(u32, u32)>,

    /// Whether to use KVM acceleration (only for same-arch emulation).
    /// Falls back to TCG if KVM is unavailable.
    #[serde(default = "default_use_kvm")]
    pub use_kvm: bool,

    // ===== Security / firmware additions =====
    /// Enable UEFI Secure Boot (requires UEFI enabled).
    /// Uses OVMF_CODE_4M.secboot.fd firmware variant.
    #[serde(default)]
    pub secure_boot: bool,

    /// Report battery information to guest via fw_cfg.
    /// Used by LibreUEFI ACPI driver to conditionally enable battery SSDT.
    #[serde(default)]
    pub report_battery: bool,

    // ===== Display & USB hardware selection =====
    /// GPU/video device model (Auto selects based on OS type).
    #[serde(default)]
    pub gpu_model: GpuModel,

    /// Video RAM in MiB (16-256). Only used when gpu_model is not None.
    #[serde(default = "default_video_ram_mb")]
    pub video_ram_mb: u32,

    /// USB controller version (USB 1.1, 2.0, or 3.0).
    #[serde(default)]
    pub usb_controller: UsbControllerVersion,

    // ===== Wave 11 additions =====
    /// Disk persistence mode — controls how disk writes interact with snapshots.
    /// Snapshotted: normal behavior, snapshots include disk state.
    /// IndependentPersistent: writes are real but excluded from snapshots.
    /// IndependentNonpersistent: writes discarded on power-off (sandbox mode).
    #[serde(default)]
    pub disk_mode: DiskMode,

    /// Disable per-VM side-channel mitigations (Spectre/Meltdown/L1TF) for ~10-30% perf.
    /// SECURITY (CWE-1037): Only safe when the guest is trusted (no untrusted workloads inside).
    /// Default is `true` (mitigations ON — safe default).
    #[serde(default = "default_side_channel_mitigations")]
    pub side_channel_mitigations: bool,

    /// Serial port configurations. Each entry maps a guest serial port (0-3) to a host backend.
    #[serde(default)]
    pub serial_ports: Vec<SerialPortConfig>,

    /// Parallel port configurations. Each entry maps a guest parallel port (0-2) to a host backend.
    #[serde(default)]
    pub parallel_ports: Vec<ParallelPortConfig>,

    // ===== Wave 12.5 additions =====
    /// Per-VM firewall rules (nftables via libvirt nwfilter).
    #[serde(default)]
    pub firewall_rules: Vec<FirewallRule>,

    // ===== Wave 12.2 additions =====
    /// Path to the directory containing this VM's libvirt hook scripts
    /// (before-start.sh / after-stop.sh). Used for the single-GPU passthrough
    /// flow that detaches the host display while the VM runs.
    #[serde(default)]
    pub vfio_hook_dir: Option<String>,

    // ===== Wave 12.6 additions — Lima-style automatic port forwarding =====
    /// Enable Lima-style automatic guest port forwarding. When true and qemu-ga
    /// is running, listening guest TCP ports are forwarded to localhost
    /// automatically. Off by default — opt in per VM.
    #[serde(default)]
    pub auto_port_forward: bool,

    /// Skip privileged ports (<1024) when auto-forwarding. Defaults to true so
    /// SSH/RDP and similar aren't silently exposed unless the user opts in.
    #[serde(default = "default_true")]
    pub auto_port_forward_skip_privileged: bool,
}

/// Boot device types for boot order configuration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum BootDevice {
    /// Boot from CD/DVD (ISO)
    Cdrom,
    /// Boot from hard disk
    Hd,
    /// Boot from network (PXE)
    Network,
    /// Boot from floppy (legacy)
    Floppy,
}

impl std::fmt::Display for BootDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootDevice::Cdrom => write!(f, "CD/DVD"),
            BootDevice::Hd => write!(f, "Hard Disk"),
            BootDevice::Network => write!(f, "Network (PXE)"),
            BootDevice::Floppy => write!(f, "Floppy"),
        }
    }
}

impl BootDevice {
    /// libvirt XML boot device name
    pub fn xml_name(&self) -> &str {
        match self {
            BootDevice::Cdrom => "cdrom",
            BootDevice::Hd => "hd",
            BootDevice::Network => "network",
            BootDevice::Floppy => "fd",
        }
    }

    pub fn all() -> &'static [BootDevice] {
        &[
            BootDevice::Cdrom,
            BootDevice::Hd,
            BootDevice::Network,
            BootDevice::Floppy,
        ]
    }
}

fn default_boot_order() -> Vec<BootDevice> {
    vec![BootDevice::Cdrom, BootDevice::Hd]
}

fn default_display_count() -> u8 {
    1
}

fn default_performance_profile() -> String {
    "default".to_string()
}

fn default_rollback_max_points() -> usize {
    5
}

fn default_disk_cache() -> String {
    "writeback".to_string()
}

fn default_disk_io() -> String {
    "threads".to_string()
}

fn default_machine_type() -> String {
    "q35".to_string()
}

fn default_boot_timeout() -> u32 {
    3000 // 3 seconds
}

fn default_use_kvm() -> bool {
    true
}

fn default_video_ram_mb() -> u32 {
    64
}

fn default_side_channel_mitigations() -> bool {
    // SECURITY (CWE-1037): Spectre/Meltdown/L1TF mitigations enabled by default.
    true
}

/// Public default boot order for use from other modules (e.g., OVA import).
pub fn default_boot_order_public() -> Vec<BootDevice> {
    default_boot_order()
}

/// Network interface configuration for multi-NIC support.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NicConfig {
    /// Network mode
    pub mode: NetworkMode,
    /// NIC model (virtio, e1000e, rtl8139)
    #[serde(default = "default_nic_model")]
    pub model: String,
    /// MAC address (auto-generated if empty)
    #[serde(default)]
    pub mac: String,
}

impl Default for NicConfig {
    fn default() -> Self {
        Self {
            mode: NetworkMode::Nat,
            model: "virtio".to_string(),
            mac: String::new(),
        }
    }
}

fn default_nic_model() -> String {
    "virtio".to_string()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum OsType {
    Linux,
    Windows,
    MacOS,
    FreeBSD,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NetworkMode {
    /// NAT — VM can access internet, host can reach VM via port forwarding
    Nat,
    /// Bridged — VM appears as a separate device on the network
    Bridged,
    /// Host-only — VM can only talk to the host
    HostOnly,
    /// LAN segment — isolated VM-to-VM network. VMs sharing the same segment
    /// name can talk to each other but to nothing else (including the host).
    /// The libvirt network must exist as `libre-vmm-lan-{sanitized_name}`.
    LanSegment(String),
    /// No networking
    None,
}

impl std::fmt::Display for NetworkMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkMode::Nat => write!(f, "NAT"),
            NetworkMode::Bridged => write!(f, "Bridged"),
            NetworkMode::HostOnly => write!(f, "Host Only"),
            NetworkMode::LanSegment(name) => write!(f, "LAN: {}", name),
            NetworkMode::None => write!(f, "None"),
        }
    }
}

/// Sanitize a LAN segment name to alphanumeric + hyphens (lowercase).
/// SECURITY: CWE-91 — prevents XML injection via crafted segment names.
pub fn sanitize_lan_segment_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else if c == '-' || c == '_' || c == ' ' {
                '-'
            } else {
                '-'
            }
        })
        .collect();
    // Collapse multiple hyphens and trim leading/trailing hyphens.
    let mut result = String::with_capacity(sanitized.len());
    let mut last_hyphen = true; // trim leading hyphens
    for c in sanitized.chars() {
        if c == '-' {
            if !last_hyphen {
                result.push('-');
                last_hyphen = true;
            }
        } else {
            result.push(c);
            last_hyphen = false;
        }
    }
    while result.ends_with('-') {
        result.pop();
    }
    if result.is_empty() {
        "default".to_string()
    } else {
        result
    }
}

/// GPU/video device model for the VM.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum GpuModel {
    /// Automatic — QXL for Windows, virtio-gpu for Linux
    Auto,
    /// VGA — basic, universal compatibility
    Vga,
    /// QXL — best for Windows guests (WDDM driver in virtio-win)
    Qxl,
    /// virtio-gpu — best for Linux guests, supports VirGL 3D
    VirtioGpu,
    /// virtio-gpu with OpenGL — hardware-accelerated 3D rendering
    VirtioGpuGl,
    /// VMware SVGA — required for macOS guests (no virtio GPU driver)
    VmwareSvga,
    /// No video device (headless server)
    None,
}

impl Default for GpuModel {
    fn default() -> Self {
        GpuModel::Auto
    }
}

impl std::fmt::Display for GpuModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpuModel::Auto => write!(f, "Auto"),
            GpuModel::Vga => write!(f, "VGA"),
            GpuModel::Qxl => write!(f, "QXL"),
            GpuModel::VirtioGpu => write!(f, "virtio-gpu"),
            GpuModel::VirtioGpuGl => write!(f, "virtio-gpu-gl (3D)"),
            GpuModel::VmwareSvga => write!(f, "VMware SVGA"),
            GpuModel::None => write!(f, "None (headless)"),
        }
    }
}

impl GpuModel {
    pub const ALL: &'static [GpuModel] = &[
        GpuModel::Auto,
        GpuModel::Vga,
        GpuModel::Qxl,
        GpuModel::VirtioGpu,
        GpuModel::VirtioGpuGl,
        GpuModel::VmwareSvga,
        GpuModel::None,
    ];

    /// Returns the libvirt video model type string.
    pub fn libvirt_model(&self, os_type: &OsType) -> &'static str {
        match self {
            GpuModel::Auto => {
                if *os_type == OsType::MacOS {
                    "vmvga"
                } else if *os_type == OsType::Windows {
                    "qxl"
                } else {
                    "virtio"
                }
            },
            GpuModel::Vga => "vga",
            GpuModel::Qxl => "qxl",
            GpuModel::VirtioGpu | GpuModel::VirtioGpuGl => "virtio",
            GpuModel::VmwareSvga => "vmvga",
            GpuModel::None => "none",
        }
    }

    /// Whether this model supports 3D acceleration.
    /// Note: VmwareSvga does NOT support 3D acceleration.
    pub fn supports_3d(&self) -> bool {
        matches!(
            self,
            GpuModel::VirtioGpu | GpuModel::VirtioGpuGl | GpuModel::Auto
        )
    }
}

/// Disk persistence mode — controls how disk writes interact with snapshots.
///
/// * `Snapshotted`: normal behavior, snapshots include disk state.
/// * `IndependentPersistent`: writes are real but excluded from snapshots.
/// * `IndependentNonpersistent`: writes discarded on power-off (sandbox mode).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum DiskMode {
    Snapshotted,
    IndependentPersistent,
    IndependentNonpersistent,
}

impl Default for DiskMode {
    fn default() -> Self {
        DiskMode::Snapshotted
    }
}

impl std::fmt::Display for DiskMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiskMode::Snapshotted => write!(f, "Snapshotted"),
            DiskMode::IndependentPersistent => write!(f, "Independent - Persistent"),
            DiskMode::IndependentNonpersistent => write!(f, "Independent - Nonpersistent"),
        }
    }
}

impl DiskMode {
    pub const ALL: &'static [DiskMode] = &[
        DiskMode::Snapshotted,
        DiskMode::IndependentPersistent,
        DiskMode::IndependentNonpersistent,
    ];
}

/// USB controller version for the VM.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum UsbControllerVersion {
    /// USB 1.1 (OHCI) — maximum compatibility
    Usb1,
    /// USB 2.0 (EHCI + companion UHCI) — good balance
    Usb2,
    /// USB 3.0 (xHCI) — fastest, recommended
    Usb3,
}

impl Default for UsbControllerVersion {
    fn default() -> Self {
        UsbControllerVersion::Usb3
    }
}

impl std::fmt::Display for UsbControllerVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsbControllerVersion::Usb1 => write!(f, "USB 1.1 (OHCI)"),
            UsbControllerVersion::Usb2 => write!(f, "USB 2.0 (EHCI)"),
            UsbControllerVersion::Usb3 => write!(f, "USB 3.0 (xHCI)"),
        }
    }
}

impl UsbControllerVersion {
    pub const ALL: &'static [UsbControllerVersion] = &[
        UsbControllerVersion::Usb1,
        UsbControllerVersion::Usb2,
        UsbControllerVersion::Usb3,
    ];

    /// Returns the libvirt USB controller model string.
    pub fn libvirt_model(&self) -> &'static str {
        match self {
            UsbControllerVersion::Usb1 => "pci-ohci",
            UsbControllerVersion::Usb2 => "ehci",
            UsbControllerVersion::Usb3 => "qemu-xhci",
        }
    }
}

/// Display protocol for VM console.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum DisplayProtocol {
    /// VNC only — stable, universal, works with noVNC browser console.
    Vnc,
    /// SPICE only — richer protocol: clipboard, audio, USB redirect, display resize.
    Spice,
    /// SPICE primary with VNC fallback — best of both worlds.
    /// SPICE handles the interactive session, VNC available for noVNC/remote.
    SpiceWithVnc,
}

impl Default for DisplayProtocol {
    fn default() -> Self {
        // Default to VNC while SPICE is being stabilized
        DisplayProtocol::Vnc
    }
}

impl std::fmt::Display for DisplayProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisplayProtocol::Vnc => write!(f, "VNC"),
            DisplayProtocol::Spice => write!(f, "SPICE"),
            DisplayProtocol::SpiceWithVnc => write!(f, "SPICE + VNC"),
        }
    }
}

impl DisplayProtocol {
    /// All available protocol options (for dropdown menus).
    pub const ALL: &'static [DisplayProtocol] = &[
        DisplayProtocol::Vnc,
        DisplayProtocol::Spice,
        DisplayProtocol::SpiceWithVnc,
    ];

    /// Short description for each protocol (for UI tooltips).
    pub fn description(&self) -> &'static str {
        match self {
            DisplayProtocol::Vnc => "Stable, universal (noVNC compatible)",
            DisplayProtocol::Spice => "Clipboard, audio, USB, display resize",
            DisplayProtocol::SpiceWithVnc => "SPICE session + VNC for remote access",
        }
    }

    /// Whether this protocol emits a SPICE graphics device.
    pub fn has_spice(&self) -> bool {
        matches!(self, DisplayProtocol::Spice | DisplayProtocol::SpiceWithVnc)
    }

    /// Whether this protocol emits a VNC graphics device.
    pub fn has_vnc(&self) -> bool {
        matches!(self, DisplayProtocol::Vnc | DisplayProtocol::SpiceWithVnc)
    }
}

/// Backward-compatible deserializer: accepts both the old `spice_display: bool`
/// format and the new `display_protocol: "Vnc"/"Spice"/"SpiceWithVnc"` enum.
pub fn deserialize_display_protocol<'de, D>(deserializer: D) -> Result<DisplayProtocol, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct DisplayProtocolVisitor;

    impl<'de> de::Visitor<'de> for DisplayProtocolVisitor {
        type Value = DisplayProtocol;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a display protocol string or a boolean (legacy)")
        }

        fn visit_bool<E: de::Error>(self, v: bool) -> Result<DisplayProtocol, E> {
            // Legacy: spice_display: true → SpiceWithVnc, false → Vnc
            Ok(if v {
                DisplayProtocol::SpiceWithVnc
            } else {
                DisplayProtocol::Vnc
            })
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<DisplayProtocol, E> {
            match v {
                "Vnc" | "vnc" | "VNC" => Ok(DisplayProtocol::Vnc),
                "Spice" | "spice" | "SPICE" => Ok(DisplayProtocol::Spice),
                "SpiceWithVnc" | "spice_with_vnc" | "SPICE + VNC" => {
                    Ok(DisplayProtocol::SpiceWithVnc)
                },
                other => Err(de::Error::unknown_variant(
                    other,
                    &["Vnc", "Spice", "SpiceWithVnc"],
                )),
            }
        }
    }

    deserializer.deserialize_any(DisplayProtocolVisitor)
}

/// A port forwarding rule for NAT networking.
/// Maps a host port to a guest port (TCP or UDP).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PortForwardRule {
    /// Protocol (tcp or udp).
    pub protocol: PortProtocol,
    /// Port on the host to listen on.
    pub host_port: u16,
    /// Port on the guest to forward to.
    pub guest_port: u16,
    /// Optional description (e.g., "SSH", "HTTP", "RDP").
    #[serde(default)]
    pub description: String,
}

impl std::fmt::Display for PortForwardRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} host:{} → guest:{}",
            self.protocol, self.host_port, self.guest_port,
        )?;
        if !self.description.is_empty() {
            write!(f, " ({})", self.description)?;
        }
        Ok(())
    }
}

/// Protocol for port forwarding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PortProtocol {
    Tcp,
    Udp,
}

impl Default for PortProtocol {
    fn default() -> Self {
        PortProtocol::Tcp
    }
}

impl std::fmt::Display for PortProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortProtocol::Tcp => write!(f, "TCP"),
            PortProtocol::Udp => write!(f, "UDP"),
        }
    }
}

// ───── Wave 12.5 — Per-VM firewall rules (nftables via libvirt nwfilter) ─────

/// Per-VM firewall rule. Maps to libvirt `<filterref filter='...'/>` with
/// inline rule parameters, generating nftables rules at the host level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FirewallRule {
    /// Rule action.
    pub action: FirewallAction,
    /// Direction: in (ingress to VM), out (egress from VM), or both.
    pub direction: FirewallDirection,
    /// Protocol — tcp, udp, icmp, or any.
    pub protocol: FirewallProtocol,
    /// Optional remote address (IP or CIDR). Empty = any.
    #[serde(default)]
    pub remote_addr: String,
    /// Optional local port (or range "1000-2000"). Empty = any.
    #[serde(default)]
    pub local_port: String,
    /// Optional remote port (or range). Empty = any.
    #[serde(default)]
    pub remote_port: String,
    /// Priority (lower = evaluated first). 0-1000.
    #[serde(default)]
    pub priority: i32,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum FirewallAction {
    Accept,
    Drop,
    Reject,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum FirewallDirection {
    In,
    Out,
    Both,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum FirewallProtocol {
    Tcp,
    Udp,
    Icmp,
    Any,
}

impl Default for FirewallAction {
    fn default() -> Self {
        FirewallAction::Accept
    }
}

impl Default for FirewallDirection {
    fn default() -> Self {
        FirewallDirection::Both
    }
}

impl Default for FirewallProtocol {
    fn default() -> Self {
        FirewallProtocol::Any
    }
}

impl std::fmt::Display for FirewallAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FirewallAction::Accept => write!(f, "Accept"),
            FirewallAction::Drop => write!(f, "Drop"),
            FirewallAction::Reject => write!(f, "Reject"),
        }
    }
}

impl std::fmt::Display for FirewallDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FirewallDirection::In => write!(f, "In"),
            FirewallDirection::Out => write!(f, "Out"),
            FirewallDirection::Both => write!(f, "Both"),
        }
    }
}

impl std::fmt::Display for FirewallProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FirewallProtocol::Tcp => write!(f, "TCP"),
            FirewallProtocol::Udp => write!(f, "UDP"),
            FirewallProtocol::Icmp => write!(f, "ICMP"),
            FirewallProtocol::Any => write!(f, "Any"),
        }
    }
}

impl FirewallAction {
    pub const ALL: &'static [FirewallAction] = &[
        FirewallAction::Accept,
        FirewallAction::Drop,
        FirewallAction::Reject,
    ];

    /// libvirt nwfilter action attribute value.
    pub fn libvirt_action(&self) -> &'static str {
        match self {
            FirewallAction::Accept => "accept",
            FirewallAction::Drop => "drop",
            FirewallAction::Reject => "reject",
        }
    }
}

impl FirewallDirection {
    pub const ALL: &'static [FirewallDirection] = &[
        FirewallDirection::In,
        FirewallDirection::Out,
        FirewallDirection::Both,
    ];

    /// libvirt nwfilter direction attribute value.
    /// "inout" matches both ingress and egress.
    pub fn libvirt_direction(&self) -> &'static str {
        match self {
            FirewallDirection::In => "in",
            FirewallDirection::Out => "out",
            FirewallDirection::Both => "inout",
        }
    }
}

impl FirewallProtocol {
    pub const ALL: &'static [FirewallProtocol] = &[
        FirewallProtocol::Tcp,
        FirewallProtocol::Udp,
        FirewallProtocol::Icmp,
        FirewallProtocol::Any,
    ];

    /// libvirt nwfilter protocol element name.
    /// `Any` maps to the `all` element which matches everything.
    pub fn libvirt_element(&self) -> &'static str {
        match self {
            FirewallProtocol::Tcp => "tcp",
            FirewallProtocol::Udp => "udp",
            FirewallProtocol::Icmp => "icmp",
            FirewallProtocol::Any => "all",
        }
    }

    /// Whether the protocol supports port attributes (srcport*, dstport*).
    pub fn has_ports(&self) -> bool {
        matches!(self, FirewallProtocol::Tcp | FirewallProtocol::Udp)
    }
}

impl Default for FirewallRule {
    fn default() -> Self {
        Self {
            action: FirewallAction::default(),
            direction: FirewallDirection::default(),
            protocol: FirewallProtocol::default(),
            remote_addr: String::new(),
            local_port: String::new(),
            remote_port: String::new(),
            priority: 500,
            description: String::new(),
        }
    }
}

/// Validate a firewall remote address (IP or CIDR).
/// Allows IPv4, IPv6, and CIDR forms — checked via the character allowlist
/// (hex digits, dots, colons, slash). Returns true if syntactically plausible.
/// SECURITY (CWE-91): Strict allowlist prevents XML injection via crafted
/// addresses, even if libvirt parsing accepts loose input.
pub fn is_valid_firewall_addr(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    if s.len() > 64 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_hexdigit() || c == '.' || c == ':' || c == '/')
}

/// Validate a firewall port specification (single port or "start-end" range).
/// Empty string means "any" and is accepted.
pub fn is_valid_firewall_port(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    if s.len() > 11 {
        return false;
    }
    s.chars().all(|c| c.is_ascii_digit() || c == '-')
}

/// CPU topology (sockets × cores × threads).
/// Total vCPUs = sockets * cores * threads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CpuTopology {
    /// Number of CPU sockets.
    pub sockets: u32,
    /// Number of cores per socket.
    pub cores: u32,
    /// Number of threads per core (SMT/Hyper-Threading).
    pub threads: u32,
}

impl CpuTopology {
    /// Total vCPU count for this topology.
    pub fn total_vcpus(&self) -> u32 {
        // Use checked multiplication to prevent integer overflow (CWE-190)
        self.sockets
            .checked_mul(self.cores)
            .and_then(|v| v.checked_mul(self.threads))
            .unwrap_or(u32::MAX)
    }

    /// Common topologies for quick selection.
    pub fn presets() -> Vec<(&'static str, CpuTopology)> {
        vec![
            (
                "1S × 2C × 1T (2 vCPUs)",
                CpuTopology {
                    sockets: 1,
                    cores: 2,
                    threads: 1,
                },
            ),
            (
                "1S × 4C × 1T (4 vCPUs)",
                CpuTopology {
                    sockets: 1,
                    cores: 4,
                    threads: 1,
                },
            ),
            (
                "1S × 4C × 2T (8 vCPUs)",
                CpuTopology {
                    sockets: 1,
                    cores: 4,
                    threads: 2,
                },
            ),
            (
                "1S × 8C × 2T (16 vCPUs)",
                CpuTopology {
                    sockets: 1,
                    cores: 8,
                    threads: 2,
                },
            ),
            (
                "2S × 4C × 2T (16 vCPUs)",
                CpuTopology {
                    sockets: 2,
                    cores: 4,
                    threads: 2,
                },
            ),
            (
                "2S × 8C × 2T (32 vCPUs)",
                CpuTopology {
                    sockets: 2,
                    cores: 8,
                    threads: 2,
                },
            ),
        ]
    }

    /// libvirt XML fragment for `<cpu>` section.
    /// SECURITY (CWE-20): Clamps all values to 1..=256 to prevent invalid XML
    /// from zero values or absurdly large topology that could crash libvirt.
    pub fn to_xml(&self) -> String {
        let sockets = self.sockets.max(1).min(256);
        let cores = self.cores.max(1).min(256);
        let threads = self.threads.max(1).min(256);
        format!(
            "      <topology sockets='{}' cores='{}' threads='{}'/>\n",
            sockets, cores, threads
        )
    }
}

impl std::fmt::Display for CpuTopology {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}S × {}C × {}T ({} vCPUs)",
            self.sockets,
            self.cores,
            self.threads,
            self.total_vcpus()
        )
    }
}

/// Backend type for serial / parallel port emulation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SerialBackend {
    /// libvirt allocates a pty on the host (default).
    Pty,
    /// Append to a file on the host (path in `target`).
    File,
    /// Connect to a Unix socket on the host (path in `target`).
    UnixSocket,
    /// Connect to a TCP host:port (in `target`).
    Tcp,
    /// Discard all output (no-op backend).
    Null,
}

impl Default for SerialBackend {
    fn default() -> Self {
        SerialBackend::Pty
    }
}

impl std::fmt::Display for SerialBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SerialBackend::Pty => write!(f, "PTY"),
            SerialBackend::File => write!(f, "File"),
            SerialBackend::UnixSocket => write!(f, "Unix Socket"),
            SerialBackend::Tcp => write!(f, "TCP"),
            SerialBackend::Null => write!(f, "Null"),
        }
    }
}

impl SerialBackend {
    pub const ALL: &'static [SerialBackend] = &[
        SerialBackend::Pty,
        SerialBackend::File,
        SerialBackend::UnixSocket,
        SerialBackend::Tcp,
        SerialBackend::Null,
    ];

    /// libvirt `<source>` element type string for this backend.
    pub fn libvirt_type(&self) -> &'static str {
        match self {
            SerialBackend::Pty => "pty",
            SerialBackend::File => "file",
            SerialBackend::UnixSocket => "unix",
            SerialBackend::Tcp => "tcp",
            SerialBackend::Null => "null",
        }
    }
}

/// Serial port configuration — maps a guest serial port to a host backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SerialPortConfig {
    /// Backend type: pty, file, unix-socket, tcp, null.
    pub backend: SerialBackend,
    /// Path / address — interpretation depends on backend.
    /// File: absolute path. UnixSocket: absolute socket path. Tcp: "host:port".
    #[serde(default)]
    pub target: String,
}

impl Default for SerialPortConfig {
    fn default() -> Self {
        Self {
            backend: SerialBackend::Pty,
            target: String::new(),
        }
    }
}

/// Parallel port configuration — maps a guest parallel port to a host backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParallelPortConfig {
    /// Backend type (reuses SerialBackend variants).
    pub backend: SerialBackend,
    /// Path / address — interpretation depends on backend.
    #[serde(default)]
    pub target: String,
}

impl Default for ParallelPortConfig {
    fn default() -> Self {
        Self {
            backend: SerialBackend::Pty,
            target: String::new(),
        }
    }
}

/// A PCI device configured for VFIO passthrough.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VfioDeviceConfig {
    /// PCI address (e.g., "0000:01:00.0")
    pub pci_address: String,
    /// Human-readable description (e.g., "NVIDIA GeForce RTX 3080")
    #[serde(default)]
    pub description: String,
    /// Whether to include the ROM bar
    #[serde(default = "default_true")]
    pub rom_bar: bool,
}

fn default_true() -> bool {
    true
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: String::from("New Virtual Machine"),
            vcpus: 2,
            memory_mib: 2048,
            disk_size_gib: 20,
            disk_path: String::new(),
            iso_path: None,
            os_type: OsType::Linux,
            uefi: true,
            gpu_accel: false,
            network: NetworkMode::Nat,
            display_protocol: DisplayProtocol::default(),
            usb_support: true,
            audio: true,
            shared_folder: None,
            description: String::new(),
            boot_order: default_boot_order(),
            network_interfaces: Vec::new(),
            autostart: false,
            tags: Vec::new(),
            folder: None,
            favorite: false,
            auto_snapshot: AutoSnapshotConfig::default(),
            display_count: 1,
            disk_encrypted: false,
            encryption_secret_uuid: None,
            tpm_enabled: true, // default on when UEFI is enabled
            tpm_version: TpmVersion::V2_0,
            port_forwards: Vec::new(),
            notes: String::new(),
            resource_limits: ResourceLimits::default(),
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
            looking_glass: LookingGlassConfig::default(),
            custom_qemu_args: Vec::new(),
            virtio_mem: false,
            iouring: false,
            cpu_features: Vec::new(),
            box_type: BoxType::Standard,
            qemu_arch: QemuArch::X86_64,
            machine_type: "q35".to_string(),
            cpu_model: String::new(),
            custom_firmware_code: None,
            custom_firmware_vars: None,
            boot_timeout: default_boot_timeout(),
            preferred_resolution: None,
            use_kvm: true,
            secure_boot: false,
            report_battery: false,
            gpu_model: GpuModel::default(),
            video_ram_mb: default_video_ram_mb(),
            usb_controller: UsbControllerVersion::default(),
            disk_mode: DiskMode::default(),
            side_channel_mitigations: default_side_channel_mitigations(),
            serial_ports: Vec::new(),
            parallel_ports: Vec::new(),
            firewall_rules: Vec::new(),
            vfio_hook_dir: None,
            auto_port_forward: false,
            auto_port_forward_skip_privileged: true,
        }
    }
}

/// Validate a VM name for safe use in libvirt XML, virsh commands, and file paths.
/// Returns an error message if invalid, or None if the name is acceptable.
pub fn validate_vm_name(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("VM name cannot be empty");
    }
    if name.len() > 128 {
        return Some("VM name must be 128 characters or less");
    }
    // SECURITY (SVE #11): Strict allowlist — alphanumeric, spaces, hyphens, underscores, dots only.
    // Parentheses and '+' removed to minimize attack surface in shell/XML contexts.
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || " -_.".contains(c))
    {
        return Some(
            "VM name can only contain letters, numbers, spaces, hyphens, underscores, and dots",
        );
    }
    // Must not start or end with whitespace
    if name != name.trim() {
        return Some("VM name cannot start or end with whitespace");
    }
    // Must not start with a dot or hyphen (hidden files / flag confusion)
    if name.starts_with('.') || name.starts_with('-') {
        return Some("VM name cannot start with a dot or hyphen");
    }
    None
}

/// Sanitize a VM name by stripping dangerous characters.
/// Use `validate_vm_name()` first — this is a fallback for imported names.
pub fn sanitize_vm_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || " -_.()".contains(*c))
        .collect();
    let trimmed = sanitized.trim().to_string();
    if trimmed.is_empty() {
        "Unnamed-VM".to_string()
    } else {
        trimmed
    }
}

impl VmConfig {
    /// Get effective network interfaces.
    /// If network_interfaces is populated, use those.
    /// Otherwise, build a single NIC from the legacy `network` field.
    pub fn effective_nics(&self) -> Vec<NicConfig> {
        if !self.network_interfaces.is_empty() {
            return self.network_interfaces.clone();
        }
        // Legacy fallback — build from single `network` field
        if self.network == NetworkMode::None {
            return Vec::new();
        }
        let model = match self.os_type {
            OsType::Windows => "e1000e".to_string(),
            OsType::MacOS => "vmxnet3".to_string(),
            _ => "virtio".to_string(),
        };
        vec![NicConfig {
            mode: self.network.clone(),
            model,
            mac: String::new(),
        }]
    }

    /// Validate and clamp resource fields to safe ranges after deserialization.
    /// Prevents tampered JSON configs from causing integer overflow or resource
    /// exhaustion when values are passed to QEMU/libvirt (CWE-400, CWE-190, CWE-20).
    /// Logs warnings via `tracing::warn!` when clamping occurs.
    pub fn validate_config_bounds(&mut self) {
        // vCPUs: must be between 1 and 512
        const MIN_VCPUS: u32 = 1;
        const MAX_VCPUS: u32 = 512;
        if self.vcpus < MIN_VCPUS {
            tracing::warn!(
                vm = %self.name, field = "vcpus", value = self.vcpus, min = MIN_VCPUS,
                "Config vcpus below minimum, clamping"
            );
            self.vcpus = MIN_VCPUS;
        }
        if self.vcpus > MAX_VCPUS {
            tracing::warn!(
                vm = %self.name, field = "vcpus", value = self.vcpus, max = MAX_VCPUS,
                "Config vcpus above maximum, clamping"
            );
            self.vcpus = MAX_VCPUS;
        }

        // Memory: must be between 128 MiB and 1,048,576 MiB (1 TiB)
        const MIN_MEMORY_MIB: u64 = 128;
        const MAX_MEMORY_MIB: u64 = 1_048_576;
        if self.memory_mib < MIN_MEMORY_MIB {
            tracing::warn!(
                vm = %self.name, field = "memory_mib", value = self.memory_mib, min = MIN_MEMORY_MIB,
                "Config memory_mib below minimum, clamping"
            );
            self.memory_mib = MIN_MEMORY_MIB;
        }
        if self.memory_mib > MAX_MEMORY_MIB {
            tracing::warn!(
                vm = %self.name, field = "memory_mib", value = self.memory_mib, max = MAX_MEMORY_MIB,
                "Config memory_mib above maximum, clamping"
            );
            self.memory_mib = MAX_MEMORY_MIB;
        }

        // Disk size: must be between 1 GiB and 65,536 GiB (64 TiB)
        const MIN_DISK_GIB: u64 = 1;
        const MAX_DISK_GIB: u64 = 65_536;
        if self.disk_size_gib < MIN_DISK_GIB {
            tracing::warn!(
                vm = %self.name, field = "disk_size_gib", value = self.disk_size_gib, min = MIN_DISK_GIB,
                "Config disk_size_gib below minimum, clamping"
            );
            self.disk_size_gib = MIN_DISK_GIB;
        }
        if self.disk_size_gib > MAX_DISK_GIB {
            tracing::warn!(
                vm = %self.name, field = "disk_size_gib", value = self.disk_size_gib, max = MAX_DISK_GIB,
                "Config disk_size_gib above maximum, clamping"
            );
            self.disk_size_gib = MAX_DISK_GIB;
        }

        // Display count: 1-8
        if self.display_count == 0 {
            self.display_count = 1;
        }
        if self.display_count > 8 {
            self.display_count = 8;
        }

        // Rollback points: 1-100
        if self.rollback_max_points == 0 {
            self.rollback_max_points = 1;
        }
        if self.rollback_max_points > 100 {
            self.rollback_max_points = 100;
        }

        // IO threads: max 16
        if self.io_threads > 16 {
            self.io_threads = 16;
        }

        // CPU topology: validate product doesn't exceed max vCPUs
        if let Some(ref topo) = self.cpu_topology {
            if topo.total_vcpus() > MAX_VCPUS || topo.total_vcpus() == 0 {
                self.cpu_topology = None;
            }
        }

        // Port forwards: cap at 256 rules
        if self.port_forwards.len() > 256 {
            self.port_forwards.truncate(256);
        }

        // VFIO devices: cap at 32
        if self.vfio_devices.len() > 32 {
            self.vfio_devices.truncate(32);
        }

        // Custom QEMU args: cap at 64
        if self.custom_qemu_args.len() > 64 {
            self.custom_qemu_args.truncate(64);
        }

        // CPU features: cap at 64
        if self.cpu_features.len() > 64 {
            self.cpu_features.truncate(64);
        }

        // Tags: cap at 32
        if self.tags.len() > 32 {
            self.tags.truncate(32);
        }

        // Notes: cap at 64KB
        if self.notes.len() > 65536 {
            self.notes.truncate(65536);
        }

        // Description: cap at 4KB
        if self.description.len() > 4096 {
            self.description.truncate(4096);
        }

        // Network interfaces: cap at 8
        if self.network_interfaces.len() > 8 {
            self.network_interfaces.truncate(8);
        }

        // Validate disk cache mode is a known value
        const VALID_CACHE_MODES: &[&str] =
            &["none", "writeback", "writethrough", "unsafe", "directsync"];
        if !VALID_CACHE_MODES.contains(&self.disk_cache.as_str()) {
            self.disk_cache = "writeback".to_string();
        }

        // Validate disk I/O mode
        const VALID_IO_MODES: &[&str] = &["native", "threads"];
        if !VALID_IO_MODES.contains(&self.disk_io_mode.as_str()) {
            self.disk_io_mode = "threads".to_string();
        }

        // Video RAM: 16-256 MiB
        self.video_ram_mb = self.video_ram_mb.clamp(16, 256);

        // Wave 11.6: Serial ports — cap at 4 (QEMU/libvirt limit).
        if self.serial_ports.len() > 4 {
            self.serial_ports.truncate(4);
        }

        // Wave 11.6: Parallel ports — cap at 3 (QEMU/libvirt limit).
        if self.parallel_ports.len() > 3 {
            self.parallel_ports.truncate(3);
        }

        // Wave 12.5: Firewall rules — cap at 64 rules, sanitize each.
        if self.firewall_rules.len() > 64 {
            tracing::warn!(
                vm = %self.name, count = self.firewall_rules.len(), max = 64,
                "Config firewall_rules above maximum, truncating"
            );
            self.firewall_rules.truncate(64);
        }
        for rule in self.firewall_rules.iter_mut() {
            // Clamp priority to 0..=1000.
            if rule.priority < 0 {
                rule.priority = 0;
            }
            if rule.priority > 1000 {
                rule.priority = 1000;
            }
            // Sanitize remote_addr: empty or hex/dot/colon/slash only, max 64.
            if !is_valid_firewall_addr(&rule.remote_addr) {
                tracing::warn!(
                    vm = %self.name, addr = %rule.remote_addr,
                    "Invalid firewall remote_addr, clearing"
                );
                rule.remote_addr.clear();
            }
            // Sanitize local_port: empty or digits/hyphen only, max 11.
            if !is_valid_firewall_port(&rule.local_port) {
                tracing::warn!(
                    vm = %self.name, port = %rule.local_port,
                    "Invalid firewall local_port, clearing"
                );
                rule.local_port.clear();
            }
            // Sanitize remote_port likewise.
            if !is_valid_firewall_port(&rule.remote_port) {
                tracing::warn!(
                    vm = %self.name, port = %rule.remote_port,
                    "Invalid firewall remote_port, clearing"
                );
                rule.remote_port.clear();
            }
            // Truncate description to 256 chars.
            if rule.description.len() > 256 {
                rule.description.truncate(256);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ───── validate_vm_name ─────────────────────────────────────────

    #[test]
    fn valid_name_simple() {
        assert!(validate_vm_name("My VM").is_none());
    }

    #[test]
    fn valid_name_alphanumeric_with_symbols() {
        assert!(validate_vm_name("Ubuntu_22.04-Desktop").is_none());
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
    fn sanitize_strips_shell_chars() {
        assert_eq!(sanitize_vm_name("vm; rm -rf /"), "vm rm -rf");
    }

    #[test]
    fn sanitize_empty_becomes_unnamed() {
        assert_eq!(sanitize_vm_name(""), "Unnamed-VM");
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
    fn cpu_topology_total_vcpus_overflow_saturates() {
        let t = CpuTopology {
            sockets: u32::MAX,
            cores: u32::MAX,
            threads: 2,
        };
        assert_eq!(t.total_vcpus(), u32::MAX);
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
    fn cpu_topology_display() {
        let t = CpuTopology {
            sockets: 1,
            cores: 4,
            threads: 2,
        };
        assert_eq!(format!("{}", t), "1S × 4C × 2T (8 vCPUs)");
    }

    // ───── DiskMode ─────────────────────────────────────────────────

    #[test]
    fn disk_mode_default_snapshotted() {
        assert_eq!(DiskMode::default(), DiskMode::Snapshotted);
    }

    #[test]
    fn disk_mode_serialize_roundtrip() {
        for &m in DiskMode::ALL {
            let json = serde_json::to_string(&m).unwrap();
            let back: DiskMode = serde_json::from_str(&json).unwrap();
            assert_eq!(m, back);
        }
    }

    // ───── DisplayProtocol ──────────────────────────────────────────

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

    // ───── BootDevice ───────────────────────────────────────────────

    #[test]
    fn boot_device_xml_names() {
        assert_eq!(BootDevice::Cdrom.xml_name(), "cdrom");
        assert_eq!(BootDevice::Hd.xml_name(), "hd");
        assert_eq!(BootDevice::Network.xml_name(), "network");
        assert_eq!(BootDevice::Floppy.xml_name(), "fd");
    }

    // ───── NetworkMode + LAN segments ───────────────────────────────

    #[test]
    fn sanitize_lan_segment_name_basic() {
        assert_eq!(sanitize_lan_segment_name("lab-frontend"), "lab-frontend");
    }

    #[test]
    fn sanitize_lan_segment_name_empty_becomes_default() {
        assert_eq!(sanitize_lan_segment_name(""), "default");
        assert_eq!(sanitize_lan_segment_name("///"), "default");
    }

    // ───── validate_config_bounds ───────────────────────────────────

    #[test]
    fn bounds_vcpus_below_min_clamped() {
        let mut c = VmConfig::default();
        c.vcpus = 0;
        c.validate_config_bounds();
        assert_eq!(c.vcpus, 1);
    }

    #[test]
    fn bounds_vcpus_above_max_clamped() {
        let mut c = VmConfig::default();
        c.vcpus = 1000;
        c.validate_config_bounds();
        assert_eq!(c.vcpus, 512);
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
    fn effective_nics_network_none_returns_empty() {
        let c = VmConfig {
            network: NetworkMode::None,
            network_interfaces: Vec::new(),
            ..VmConfig::default()
        };
        let nics = c.effective_nics();
        assert!(nics.is_empty());
    }

    // ───── Firewall ─────────────────────────────────────────────────

    #[test]
    fn firewall_action_defaults_to_accept() {
        assert_eq!(FirewallAction::default(), FirewallAction::Accept);
    }

    #[test]
    fn firewall_rule_default_priority_500() {
        let r = FirewallRule::default();
        assert_eq!(r.priority, 500);
    }
}
