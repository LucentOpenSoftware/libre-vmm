//! Main application state and logic.

use crate::spice::SpiceFramebuffer;
use crate::theme;
use crate::views;
use crate::views::backup_restore::BackupRestoreState;
use crate::views::clone_dialog::CloneDialogState;
use crate::views::disk_manage::DiskManageState;
use crate::views::encryption_dialog::EncryptionDialogState;
use crate::views::first_run::{FirstRunState, FirstRunStep};
use crate::views::guest_file_manager::GuestFileManagerState;
use crate::views::guest_tools::GuestToolsState;
use crate::views::host_guest::HostGuestState;
use crate::views::import_export::ImportExportState;
use crate::views::import_wizard::ImportWizardState;
use crate::views::iso_library::IsoLibraryState;
use crate::views::media_dialog::MediaDialogState;
use crate::views::migration::MigrationState;
use crate::views::monitor::MonitorState;
use crate::views::multi_display::MultiDisplayState;
use crate::views::network_conditioner::NetCondState;
use crate::views::novnc_panel::NoVncPanelState;
use crate::views::pci_passthrough::PciPassthroughState;
use crate::views::pip::PipState;
use crate::views::port_forward::PortForwardState;
use crate::views::remote_hosts::RemoteHostsState;
use crate::views::rollback::RollbackState;
use crate::views::screen_recording::ScreenRecordingState;
use crate::views::single_gpu_setup::SingleGpuWizardState;
use crate::views::template_manager::TemplateManagerState;
use crate::views::unattended_wizard::UnattendedWizardState;
use crate::views::usb_manager::UsbManagerState;
use crate::vnc::VncFramebuffer;
use eframe::egui;
use rust_i18n::t;
use std::collections::VecDeque;
use vmm_core::config::{NetworkMode, VmConfig, VmConfigIo};
use vmm_core::connection::HypervisorConnection;
use vmm_core::domain::{VmInfo, VmState};
use vmm_core::gpu::GpuCapabilities;
use vmm_core::guest_agent::GuestInfo;
use vmm_core::monitor::VmMonitor;
use vmm_core::notifications::NotificationSettings;
use vmm_core::qemu_archs::{BoxType, QemuArch};
use vmm_core::snapshot::SnapshotInfo;
use vmm_core::task::TaskManager;
use vmm_core::template::builtin_templates;

/// Which screen is currently displayed.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Home,
    /// Box type chooser — first screen when creating a new VM.
    BoxSelector,
    /// Standard/Power User template wizard (Box 1 & 3).
    CreateWizard(WizardStep),
    /// Architecture wizard for Hardware Lab (Box 2).
    ArchWizard(ArchWizardStep),
    /// Power User wizard (Box 3).
    PowerWizard(PowerWizardStep),
    VmSettings(String),
    Settings,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WizardStep {
    ChooseTemplate,
    Configure,
    Review,
}

/// Steps in the Box 2 (Hardware Lab) architecture wizard.
#[derive(Debug, Clone, PartialEq)]
pub enum ArchWizardStep {
    /// Choose target CPU architecture (x86_64, ARM64, RISC-V, etc.)
    ChooseArch,
    /// Choose QEMU machine type and CPU model.
    ChooseMachine,
    /// Configure hardware (CPUs, RAM, disk, network).
    Configure,
    /// Review summary and create.
    Review,
}

/// Steps in the Box 3 (Power User) wizard.
#[derive(Debug, Clone, PartialEq)]
pub enum PowerWizardStep {
    /// Choose OS template.
    ChooseTemplate,
    /// CPU topology, hugepages, memory.
    CpuMemory,
    /// Disk cache, I/O threads, VFIO passthrough.
    StoragePassthrough,
    /// Network, boot options, custom QEMU args.
    NetworkExtras,
    /// Review summary and create.
    Review,
}

/// All mutable state for the Power User wizard (Box 3).
pub struct PowerWizardState {
    pub cpu_topology: Option<vmm_core::config::CpuTopology>,
    pub hugepages: bool,
    pub disk_cache: String,
    pub disk_io_mode: String,
    pub io_threads: u32,
    pub vfio_devices: Vec<vmm_core::config::VfioDeviceConfig>,
    pub vfio_input: String,
    pub custom_args: Vec<String>,
    pub custom_arg_input: String,
    pub nic_model: String,
    pub tpm_enabled: bool,
    pub gpu_accel: bool,
    pub display_protocol: vmm_core::config::DisplayProtocol,
}

impl Default for PowerWizardState {
    fn default() -> Self {
        Self {
            cpu_topology: None,
            hugepages: false,
            disk_cache: "none".to_string(),
            disk_io_mode: "native".to_string(),
            io_threads: 0,
            vfio_devices: Vec::new(),
            vfio_input: String::new(),
            custom_args: Vec::new(),
            custom_arg_input: String::new(),
            nic_model: "virtio".to_string(),
            tpm_enabled: false,
            gpu_accel: false,
            display_protocol: vmm_core::config::DisplayProtocol::default(),
        }
    }
}

/// All mutable state for the Box 2 (Hardware Lab) architecture wizard.
pub struct ArchWizardState {
    pub arch: Option<QemuArch>,
    pub machine: String,
    pub cpu: String,
    pub use_kvm: bool,
    pub show_all: bool,
    pub cpu_topology: Option<vmm_core::config::CpuTopology>,
    pub cpu_features: Vec<String>,
}

impl Default for ArchWizardState {
    fn default() -> Self {
        Self {
            arch: None,
            machine: String::new(),
            cpu: String::new(),
            use_kvm: true,
            show_all: false,
            cpu_topology: None,
            cpu_features: Vec::new(),
        }
    }
}

/// Primary view mode — Console-First UX.
/// Console is the default workspace for running VMs.
/// Manage mode provides admin tabs (Summary, Snapshots, Performance).
#[derive(Debug, Clone, PartialEq)]
pub enum ViewMode {
    /// Console is the primary workspace — minimal UI, full-screen VNC.
    Console,
    /// Management mode — tabs for Summary, Snapshots, Performance.
    Manage(ManageTab),
}

/// Which management tab is active.
#[derive(Debug, Clone, PartialEq)]
pub enum ManageTab {
    Summary,
    Snapshots,
    Performance,
}

/// Sidebar sorting mode.
#[derive(Debug, Clone, PartialEq)]
pub enum SidebarSort {
    Name,
    State,
    Favorites,
}

/// Which destructive snapshot operation a pending confirmation refers to.
#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotOp {
    Revert,
    Delete,
}

/// A status / event entry for the event log.
#[derive(Debug, Clone)]
pub struct EventEntry {
    pub text: String,
    pub is_error: bool,
    pub timestamp: std::time::Instant,
    pub time_str: String,
}

/// Shared wizard state for all VM creation wizards (Standard, Power User, Hardware Lab).
pub struct WizardState {
    pub name: String,
    pub iso: String,
    pub template_idx: usize,
    pub cpus: u32,
    pub memory_mib: u64,
    pub disk_gib: u64,
    pub network: NetworkMode,
    pub uefi: bool,
    pub description: String,
    pub template_search: String,
    pub box_type: BoxType,
}

/// Main application state.
pub struct LibreVmmApp {
    screen: Screen,
    vms: Vec<VmInfo>,
    conn: Option<HypervisorConnection>,
    connection_error: Option<String>,

    // Create wizard state
    pub wizard: WizardState,

    // UI state
    selected_vm: Option<String>,
    view_mode: ViewMode,
    sidebar_visible: bool,
    show_event_log: bool,
    event_log: VecDeque<EventEntry>,
    confirm_delete: Option<String>,
    /// Pending snapshot operation that requires user confirmation.
    /// Tuple of (vm_name, snapshot_name, operation_kind).
    confirm_snapshot_op: Option<(String, String, SnapshotOp)>,
    /// Pending force-stop request awaiting confirmation (VM name).
    confirm_force_stop: Option<String>,
    search_query: String,

    // Embedded console (SPICE preferred, VNC fallback)
    console_fb: Option<VncFramebuffer>,
    console_spice_fb: Option<SpiceFramebuffer>,
    console_vm_name: Option<String>,
    console_is_spice: bool,

    // Snapshot state
    snapshots: Vec<SnapshotInfo>,
    snapshot_name: String,
    snapshot_description: String,
    /// Name of the snapshot currently selected in the tree view (None = none).
    /// Used for highlighting and showing per-snapshot action affordances.
    selected_snapshot: Option<String>,

    // VM config cache (for summary / settings views)
    selected_vm_config: Option<VmConfig>,
    editing_config: Option<VmConfig>,

    // ISO library
    iso_library_state: IsoLibraryState,
    show_iso_picker: bool,

    // Wave 2: Clone, Import/Export, USB
    clone_dialog: CloneDialogState,
    import_export: ImportExportState,
    import_wizard: Option<ImportWizardState>,
    usb_manager: UsbManagerState,

    // Wave 3: Performance Monitoring, Guest Agent, Library Organization
    monitor_state: MonitorState,
    vm_monitor: Option<VmMonitor>,
    guest_info: Option<GuestInfo>,
    sidebar_sort: SidebarSort,

    // Wave 4: Advanced VM Features
    gpu_capabilities: Option<GpuCapabilities>,
    managed_save_cache: std::collections::HashMap<String, bool>,
    encryption_passphrase: String,

    // Wave 5: Templates & Remote
    template_manager: TemplateManagerState,
    remote_hosts: RemoteHostsState,
    /// Name of the currently connected remote host (None = local).
    remote_host_name: Option<String>,

    // Wave 6: Task system, port forwarding, TPM
    task_manager: TaskManager,
    show_task_panel: bool,
    port_forward: PortForwardState,

    // Wave 6+: Notifications, Host-Guest Integration, Live Migration
    notification_settings: NotificationSettings,
    host_guest: HostGuestState,
    migration: MigrationState,

    // UI scale factor (user-adjustable, 0.0 = auto)
    ui_scale: f32,

    // Parallels-inspired features
    pip: PipState,
    rollback: RollbackState,
    disk_manage: DiskManageState,
    net_cond: NetCondState,
    auto_pause_enabled: bool,
    auto_pause_was_running: bool,

    // Wave 1 VMware parity: Guest Tools, Media, Boot, Auto-Resize
    guest_tools: GuestToolsState,
    media_dialog: MediaDialogState,
    /// Last console panel size we requested from VNC (for debouncing).
    last_requested_console_size: (u16, u16),
    /// Timestamp of last console resize request (for debouncing).
    last_resize_request_time: std::time::Instant,
    /// Whether display auto-resize is enabled.
    display_auto_resize: bool,

    // Wave 2: Seamless Integration
    /// Drop transfer feedback message (shown as overlay on console).
    drop_transfer_msg: Option<String>,
    /// Whether the drop transfer message is an error.
    drop_transfer_err: bool,
    /// Timestamp when the drop message was shown (auto-clear after 5s).
    drop_transfer_time: Option<std::time::Instant>,
    /// User preferences for default hardware, auto-suspend, etc.
    preferences: vmm_core::Preferences,
    /// Systemd shutdown inhibitor handle.
    #[allow(dead_code)]
    host_inhibitor: Option<vmm_core::host_inhibitor::HostInhibitor>,

    // Wave 3: Power Management
    network_editor: views::network_editor::NetworkEditorState,
    sidebar_collapsed_groups: std::collections::HashSet<String>,
    /// Cached VM configs (refreshed alongside VM list to avoid per-frame disk I/O).
    vm_configs_cache: Vec<VmConfig>,

    // Wave 4: Enterprise & Security
    encryption_dialog: EncryptionDialogState,
    novnc_panel: NoVncPanelState,
    unattended_wizard: UnattendedWizardState,
    screen_recording: ScreenRecordingState,

    // Wave 5: Feature Parity
    balloon_stats: Option<vmm_core::balloon::BalloonStats>,
    balloon_target_mib: u64,
    guest_file_manager: GuestFileManagerState,
    #[allow(dead_code)]
    multi_display: MultiDisplayState,

    // PCI/GPU Passthrough
    pci_passthrough: PciPassthroughState,

    // Wave 12.2: Single-GPU passthrough wizard
    single_gpu: SingleGpuWizardState,

    // Backup & Restore
    backup_restore: BackupRestoreState,

    // Wave 13: First-Run Setup Wizard
    first_run: FirstRunState,

    // Open VM tabs (shown in the tab bar above the main content)
    open_vm_tabs: Vec<String>,

    // ===== Boxes system =====
    /// The "active" box type for the selected VM (drives theme accent).
    active_box_type: BoxType,

    // Power User wizard state (Box 3)
    pub power_wizard: PowerWizardState,

    // Architecture wizard state (Box 2: Hardware Lab)
    pub arch_wizard: ArchWizardState,

    // Refresh timer
    last_refresh: std::time::Instant,

    // Console framebuffer cache (avoids cloning 8+ MB pixels every frame)
    console_cache: views::console::ConsoleCache,
}

impl LibreVmmApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Load saved UI scale, or detect a good default
        let ui_scale = load_ui_scale().unwrap_or(0.0);
        if ui_scale > 0.0 {
            cc.egui_ctx.set_zoom_factor(ui_scale);
        } else {
            // Auto-detect: on Linux, if the native pixels_per_point looks wrong,
            // apply a sensible default. Many Linux DEs report wrong DPI to eframe.
            let native_ppp = cc.egui_ctx.pixels_per_point();
            if native_ppp > 2.5 || native_ppp < 0.5 {
                // Clearly wrong — reset to 1.0
                cc.egui_ctx.set_zoom_factor(1.0 / native_ppp);
            }
        }

        theme::apply_theme(&cc.egui_ctx);

        let (conn, connection_error) = match HypervisorConnection::connect_best() {
            Ok(c) => (Some(c), None),
            Err(e) => (None, Some(e.to_string())),
        };

        let prefs = vmm_core::Preferences::load();

        let mut app = Self {
            screen: Screen::Home,
            vms: Vec::new(),
            conn,
            connection_error,
            wizard: WizardState {
                name: String::new(),
                iso: String::new(),
                template_idx: 0,
                cpus: prefs.default_cpus,
                memory_mib: prefs.default_memory_mib,
                disk_gib: prefs.default_disk_gib,
                network: NetworkMode::Nat,
                uefi: prefs.default_uefi,
                description: String::new(),
                template_search: String::new(),
                box_type: BoxType::Standard,
            },
            selected_vm: None,
            view_mode: ViewMode::Manage(ManageTab::Summary),
            sidebar_visible: true,
            show_event_log: false,
            event_log: VecDeque::new(),
            confirm_delete: None,
            confirm_snapshot_op: None,
            confirm_force_stop: None,
            search_query: String::new(),
            console_fb: None,
            console_spice_fb: None,
            console_vm_name: None,
            console_is_spice: false,
            snapshots: Vec::new(),
            snapshot_name: String::new(),
            snapshot_description: String::new(),
            selected_snapshot: None,
            selected_vm_config: None,
            editing_config: None,
            iso_library_state: IsoLibraryState::default(),
            show_iso_picker: false,
            clone_dialog: CloneDialogState::default(),
            import_export: ImportExportState::default(),
            import_wizard: None,
            usb_manager: UsbManagerState::default(),
            monitor_state: MonitorState::default(),
            vm_monitor: None,
            guest_info: None,
            sidebar_sort: SidebarSort::Favorites,
            gpu_capabilities: None,
            managed_save_cache: std::collections::HashMap::new(),
            encryption_passphrase: String::new(),
            template_manager: TemplateManagerState::default(),
            remote_hosts: RemoteHostsState::default(),
            remote_host_name: None,
            task_manager: TaskManager::new(),
            show_task_panel: false,
            port_forward: PortForwardState::default(),
            notification_settings: NotificationSettings::default(),
            host_guest: HostGuestState::default(),
            migration: MigrationState::default(),
            ui_scale,
            pip: PipState::default(),
            rollback: RollbackState::default(),
            disk_manage: DiskManageState::default(),
            net_cond: NetCondState::default(),
            auto_pause_enabled: false,
            auto_pause_was_running: false,
            guest_tools: GuestToolsState::default(),
            media_dialog: MediaDialogState::default(),
            last_requested_console_size: (0, 0),
            last_resize_request_time: std::time::Instant::now(),
            display_auto_resize: true,
            drop_transfer_msg: None,
            drop_transfer_err: false,
            drop_transfer_time: None,
            preferences: prefs,
            host_inhibitor: vmm_core::host_inhibitor::HostInhibitor::acquire().ok(),
            network_editor: views::network_editor::NetworkEditorState::default(),
            sidebar_collapsed_groups: std::collections::HashSet::new(),
            vm_configs_cache: VmConfig::list_all().unwrap_or_default(),
            encryption_dialog: EncryptionDialogState::default(),
            novnc_panel: NoVncPanelState::default(),
            unattended_wizard: UnattendedWizardState::default(),
            screen_recording: ScreenRecordingState::default(),
            balloon_stats: None,
            balloon_target_mib: 1024,
            guest_file_manager: GuestFileManagerState::default(),
            multi_display: MultiDisplayState::default(),
            pci_passthrough: PciPassthroughState::default(),
            single_gpu: SingleGpuWizardState::default(),
            backup_restore: BackupRestoreState::default(),
            first_run: FirstRunState::default(),
            open_vm_tabs: Vec::new(),
            active_box_type: BoxType::Standard,
            power_wizard: PowerWizardState::default(),
            arch_wizard: ArchWizardState::default(),
            last_refresh: std::time::Instant::now(),
            console_cache: views::console::ConsoleCache::default(),
        };

        app.refresh_vms();

        // Wave 13.6 — first-run wizard: open automatically when this is the
        // user's first launch AND there are no existing VMs to manage.
        if !app.preferences.first_run_completed && app.vms.is_empty() {
            app.first_run.open = true;
            app.first_run.step = FirstRunStep::Welcome;
        }

        app
    }

    pub fn refresh_vms(&mut self) {
        // Throttle: skip if refreshed less than 2 seconds ago (except the auto-refresh
        // at 5s intervals already guards itself; this protects against rapid-fire calls
        // from action_* methods).
        if self.last_refresh.elapsed() < std::time::Duration::from_secs(2) {
            return;
        }
        if let Some(ref conn) = self.conn {
            match conn.list_vms() {
                Ok(vms) => {
                    // Update managed save cache for Off VMs
                    for vm in &vms {
                        if vm.state == VmState::Off {
                            let has_save = conn.has_managed_save(&vm.name);
                            self.managed_save_cache.insert(vm.name.clone(), has_save);
                        } else {
                            self.managed_save_cache.remove(&vm.name);
                        }
                    }
                    self.vms = vms;

                    // Auto-fallback: if selected VM is no longer running and we're
                    // in Console mode, switch back to management view.
                    if self.view_mode == ViewMode::Console {
                        if let Some(ref selected) = self.selected_vm {
                            let is_running = self
                                .vms
                                .iter()
                                .find(|v| &v.name == selected)
                                .map(|v| v.state == VmState::Running)
                                .unwrap_or(false);
                            if !is_running {
                                self.view_mode = ViewMode::Manage(ManageTab::Summary);
                                self.disconnect_console_inner();
                            }
                        }
                    }

                    // Framebuffer-leak guard: regardless of the current view
                    // mode, if we still hold a live VNC/SPICE framebuffer for a
                    // VM that has transitioned out of Running (or vanished
                    // entirely), drop the connection so we don't keep a stale
                    // socket open. This catches cases where the user navigated
                    // away from the Console view between the VM stopping and
                    // refresh_vms() observing the new state, or where the VM
                    // was stopped externally (virsh, libvirt) while we were on
                    // another screen.
                    if self.console_vm_name.is_some() {
                        let cleanup = match self.console_vm_name.as_deref() {
                            Some(name) => self
                                .vms
                                .iter()
                                .find(|v| v.name == name)
                                .map(|v| v.state != VmState::Running)
                                .unwrap_or(true),
                            None => false,
                        };
                        if cleanup {
                            self.disconnect_console_inner();
                        }
                    }
                },
                Err(e) => self.push_event(true, format!("Failed to list VMs: {}", e)),
            }
        }
        // Refresh cached VM configs (favorite, folder, etc.) from disk.
        self.vm_configs_cache = VmConfig::list_all().unwrap_or_default();
        self.last_refresh = std::time::Instant::now();
    }

    /// Hard cap on the in-memory event log to bound memory use over long sessions.
    const EVENT_LOG_CAP: usize = 200;

    fn push_event(&mut self, is_error: bool, text: impl Into<String>) {
        let now = chrono::Local::now();
        self.event_log.push_back(EventEntry {
            text: text.into(),
            is_error,
            timestamp: std::time::Instant::now(),
            time_str: now.format("%H:%M:%S").to_string(),
        });
        // Enforce the cap strictly: drain the oldest entries until we are at or
        // below `EVENT_LOG_CAP`. A loop (rather than a single pop) makes the
        // invariant hold even if the deque were ever populated by a future code
        // path that bypasses this function.
        while self.event_log.len() > Self::EVENT_LOG_CAP {
            self.event_log.pop_front();
        }
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.push_event(false, msg);
    }

    fn set_error(&mut self, msg: impl Into<String>) {
        self.push_event(true, msg);
    }

    /// Public error reporter for use by view modules (e.g., input validation).
    pub fn report_validation_error(&mut self, msg: impl Into<String>) {
        self.push_event(true, msg);
    }

    fn start_vm(&mut self, name: &str) {
        // macOS pre-flight check: KVM ignore_msrs must be enabled
        if let Some(ref cfg) = self.selected_vm_config {
            if cfg.os_type == vmm_core::config::OsType::MacOS {
                match std::fs::read_to_string("/sys/module/kvm/parameters/ignore_msrs") {
                    Ok(val) if val.trim() == "Y" || val.trim() == "1" => {
                        // Good — ignore_msrs is enabled
                    },
                    _ => {
                        self.set_error(format!(
                            "macOS VMs require KVM ignore_msrs. Run:\n\
                             echo 1 | sudo tee /sys/module/kvm/parameters/ignore_msrs\n\
                             To make permanent, add 'options kvm ignore_msrs=1' to /etc/modprobe.d/kvm.conf"
                        ));
                        return;
                    },
                }
            }
        }
        if let Some(ref conn) = self.conn {
            match conn.start_vm(name) {
                Ok(()) => {
                    self.set_status(format!("VM '{}' started", name));
                    vmm_core::notifications::notify_vm_power(
                        name,
                        "Started",
                        &self.notification_settings,
                    );
                    self.refresh_vms();
                    // Console-First: auto-connect after successful start
                    self.auto_connect_console(name);
                    // Looking Glass: create SHM file and auto-launch client
                    if let Some(ref cfg) = self.selected_vm_config {
                        if cfg.looking_glass.enabled && !cfg.vfio_devices.is_empty() {
                            if let Err(e) = vmm_core::looking_glass::create_shm_file(
                                cfg.looking_glass.ivshmem_size_mib,
                            ) {
                                tracing::warn!("Looking Glass SHM creation failed: {}", e);
                            }
                            if cfg.looking_glass.auto_launch {
                                match vmm_core::looking_glass::launch_client(&cfg.looking_glass) {
                                    Ok(_child) => {
                                        tracing::info!(
                                            "Looking Glass client launched for '{}'",
                                            name
                                        );
                                    },
                                    Err(e) => {
                                        tracing::warn!("Looking Glass client launch failed: {}", e);
                                    },
                                }
                            }
                        }
                    }
                    // Wave 2: Auto-mount shared folder after boot delay
                    if self.preferences.shared_folder_auto_mount {
                        if let Some(ref cfg) = self.selected_vm_config {
                            if cfg.shared_folder.is_some() {
                                let vm_name = name.to_string();
                                std::thread::Builder::new()
                                    .name("auto-mount".into())
                                    .spawn(move || {
                                        // Wait for guest agent to be ready
                                        std::thread::sleep(std::time::Duration::from_secs(15));
                                        if let Err(e) =
                                            vmm_core::host_guest::auto_mount_shared_folder(&vm_name)
                                        {
                                            tracing::warn!(
                                                "Auto-mount shared folder failed for '{}': {}",
                                                vm_name,
                                                e
                                            );
                                        }
                                    })
                                    .ok();
                            }
                        }
                    }
                },
                Err(e) => {
                    self.set_error(format!("Failed to start '{}': {}", name, e));
                    vmm_core::notifications::notify_error(
                        "VM Start Failed",
                        &e.to_string(),
                        &self.notification_settings,
                    );
                    self.refresh_vms();
                },
            }
        }
    }

    fn shutdown_vm(&mut self, name: &str) {
        if let Some(ref conn) = self.conn {
            match conn.shutdown_vm(name) {
                Ok(()) => {
                    self.set_status(format!("Shutdown signal sent to '{}'", name));
                    vmm_core::notifications::notify_vm_power(
                        name,
                        "Shutting Down",
                        &self.notification_settings,
                    );
                },
                Err(e) => self.set_error(format!("Failed to shut down '{}': {}", name, e)),
            }
            self.refresh_vms();
        }
    }

    fn force_stop_vm(&mut self, name: &str) {
        if let Some(ref conn) = self.conn {
            match conn.force_stop_vm(name) {
                Ok(()) => self.set_status(format!("VM '{}' force-stopped", name)),
                Err(e) => self.set_error(format!("Failed to stop '{}': {}", name, e)),
            }
            // Force-stop terminates the VM instantly — drop any open VNC/SPICE
            // framebuffer immediately so the socket isn't left dangling waiting
            // for the next refresh_vms() tick.
            if self.console_vm_name.as_deref() == Some(name) {
                self.disconnect_console_inner();
            }
            self.refresh_vms();
        }
    }

    fn pause_vm(&mut self, name: &str) {
        if let Some(ref conn) = self.conn {
            match conn.pause_vm(name) {
                Ok(()) => self.set_status(format!("VM '{}' paused", name)),
                Err(e) => self.set_error(format!("Failed to pause '{}': {}", name, e)),
            }
            self.refresh_vms();
        }
    }

    fn resume_vm(&mut self, name: &str) {
        if let Some(ref conn) = self.conn {
            match conn.resume_vm(name) {
                Ok(()) => self.set_status(format!("VM '{}' resumed", name)),
                Err(e) => self.set_error(format!("Failed to resume '{}': {}", name, e)),
            }
            self.refresh_vms();
        }
    }

    fn open_console(&mut self, name: &str) {
        self.disconnect_console_inner();

        // Determine protocol preference from VM config
        let proto = self
            .selected_vm_config
            .as_ref()
            .map(|c| c.display_protocol)
            .unwrap_or_default();

        if let Some(ref conn) = self.conn {
            // Try SPICE if the VM's protocol includes it
            if proto.has_spice() {
                if let Ok(Some(port)) = conn.get_spice_port(name) {
                    let fb = SpiceFramebuffer::new();
                    fb.connect("127.0.0.1", port);
                    self.console_spice_fb = Some(fb);
                    self.console_is_spice = true;
                    self.console_vm_name = Some(name.to_string());
                    self.selected_vm = Some(name.to_string());
                    self.view_mode = ViewMode::Console;
                    self.screen = Screen::Home;
                    self.set_status(format!(
                        "Console connected to '{}' (SPICE port {})",
                        name, port
                    ));
                    return;
                }
            }

            // VNC — either preferred or as fallback
            match conn.get_vnc_port(name) {
                Ok(Some(port)) => {
                    let fb = VncFramebuffer::new();
                    fb.connect("127.0.0.1", port);
                    self.console_fb = Some(fb);
                    self.console_is_spice = false;
                    self.console_vm_name = Some(name.to_string());
                    self.selected_vm = Some(name.to_string());
                    self.view_mode = ViewMode::Console;
                    self.screen = Screen::Home;
                    self.set_status(format!(
                        "Console connected to '{}' (VNC port {})",
                        name, port
                    ));
                },
                Ok(None) => match conn.open_console(name) {
                    Ok(()) => self.set_status(format!("External console opened for '{}'", name)),
                    Err(e) => self.set_error(format!("Failed to open console: {}", e)),
                },
                Err(e) => self.set_error(format!("Failed to get VNC port: {}", e)),
            }
        }
    }

    /// Auto-connect console for a running VM without re-setting selected_vm.
    /// Respects the VM's display_protocol setting. Used by set_selected_vm() and start_vm().
    fn auto_connect_console(&mut self, name: &str) {
        self.disconnect_console_inner();

        // Determine protocol preference from VM config
        let proto = self
            .selected_vm_config
            .as_ref()
            .map(|c| c.display_protocol)
            .unwrap_or_default();

        if let Some(ref conn) = self.conn {
            // Try SPICE if the VM's protocol includes it
            if proto.has_spice() {
                if let Ok(Some(port)) = conn.get_spice_port(name) {
                    let fb = SpiceFramebuffer::new();
                    fb.connect("127.0.0.1", port);
                    self.console_spice_fb = Some(fb);
                    self.console_is_spice = true;
                    self.console_vm_name = Some(name.to_string());
                    self.view_mode = ViewMode::Console;
                    self.set_status(format!(
                        "Console auto-connected to '{}' (SPICE port {})",
                        name, port
                    ));
                    return;
                }
            }

            // VNC — either preferred or as fallback
            match conn.get_vnc_port(name) {
                Ok(Some(port)) => {
                    let fb = VncFramebuffer::new();
                    fb.connect("127.0.0.1", port);
                    self.console_fb = Some(fb);
                    self.console_is_spice = false;
                    self.console_vm_name = Some(name.to_string());
                    self.view_mode = ViewMode::Console;
                    self.set_status(format!(
                        "Console auto-connected to '{}' (VNC port {})",
                        name, port
                    ));
                },
                Ok(None) | Err(_) => {
                    // No display port available — fall back to management view
                    self.view_mode = ViewMode::Manage(ManageTab::Summary);
                },
            }
        }
    }

    fn disconnect_console_inner(&mut self) {
        if let Some(ref fb) = self.console_fb {
            fb.disconnect();
        }
        self.console_fb = None;
        self.console_spice_fb = None;
        self.console_is_spice = false;
        self.console_vm_name = None;
    }

    fn delete_vm(&mut self, name: &str) {
        if let Some(ref conn) = self.conn {
            match conn.delete_vm(name, true) {
                Ok(()) => {
                    self.set_status(format!("VM '{}' deleted", name));
                    self.selected_vm = None;
                    self.selected_vm_config = None;
                    self.screen = Screen::Home;
                },
                Err(e) => self.set_error(format!("Failed to delete '{}': {}", name, e)),
            }
            self.refresh_vms();
        }
    }

    fn create_vm_from_wizard(&mut self) {
        let templates = builtin_templates();
        let template = match templates.get(self.wizard.template_idx) {
            Some(t) => t,
            None => {
                self.set_error(format!(
                    "Invalid template index: {}",
                    self.wizard.template_idx
                ));
                return;
            },
        };

        // Validate VM name before creation
        if let Some(err) = vmm_core::config::validate_vm_name(&self.wizard.name) {
            self.set_error(format!("Invalid VM name: {}", err));
            return;
        }

        let mut config = VmConfig::from_template(&self.wizard.name, template, None);
        config.vcpus = self.wizard.cpus;
        config.memory_mib = self.wizard.memory_mib;
        config.disk_size_gib = self.wizard.disk_gib;
        config.network = self.wizard.network.clone();
        config.uefi = self.wizard.uefi;
        config.description = self.wizard.description.clone();

        if !self.wizard.iso.is_empty() {
            config.iso_path = Some(self.wizard.iso.clone());
        }

        if let Some(ref conn) = self.conn {
            match conn.create_vm(&config) {
                Ok(()) => {
                    self.set_status(format!("VM '{}' created successfully!", config.name));
                    self.screen = Screen::Home;
                    self.refresh_vms();
                    self.reset_wizard();
                },
                Err(e) => self.set_error(format!("Failed to create VM: {}", e)),
            }
        }
    }

    fn reset_wizard(&mut self) {
        self.wizard.name.clear();
        self.wizard.iso.clear();
        self.wizard.template_idx = 0;
        self.wizard.cpus = 2;
        self.wizard.memory_mib = 4096;
        self.wizard.disk_gib = 25;
        self.wizard.network = NetworkMode::Nat;
        self.wizard.uefi = true;
        self.wizard.description.clear();
        self.wizard.template_search.clear();
    }

    /// Load the VmConfig for the currently selected VM.
    pub fn load_selected_config(&mut self) {
        if let Some(ref name) = self.selected_vm {
            if let Some(vm) = self.vms.iter().find(|v| &v.name == name) {
                if let Ok(uuid) = uuid::Uuid::parse_str(&vm.uuid) {
                    self.selected_vm_config = VmConfig::load(&uuid).ok();
                    return;
                }
            }
        }
        self.selected_vm_config = None;
    }

    /// Refresh snapshots for the selected VM.
    pub fn refresh_snapshots(&mut self) {
        if let (Some(ref conn), Some(ref name)) = (&self.conn, &self.selected_vm) {
            match vmm_core::snapshot::list_snapshots(conn.raw_conn(), name) {
                Ok(snaps) => self.snapshots = snaps,
                Err(e) => {
                    self.snapshots.clear();
                    self.push_event(true, format!("Failed to list snapshots: {}", e));
                },
            }
        } else {
            self.snapshots.clear();
        }
    }
}

impl eframe::App for LibreVmmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Enforce saved UI scale every frame.
        // On Linux, eframe may receive ScaleFactorChanged events after construction
        // that override our zoom_factor. Re-apply if it drifts from the saved value.
        if self.ui_scale > 0.0 {
            let current = ctx.zoom_factor();
            if (current - self.ui_scale).abs() > 0.001 {
                ctx.set_zoom_factor(self.ui_scale);
            }
        }

        // Auto-refresh every 5 seconds
        if self.last_refresh.elapsed() > std::time::Duration::from_secs(5) {
            self.refresh_vms();
        }

        // Keyboard shortcuts
        self.handle_shortcuts(ctx);

        // Apply box-type accent colors
        theme::apply_box_accent(ctx, &self.active_box_type);

        // ===== LAYOUT =====

        // 1. Menu bar (top)
        views::menu_bar::render(self, ctx);

        // 1b. VM tab bar (below menu bar)
        views::tab_bar::render(self, ctx);

        // 2. Status bar (bottom)
        views::statusbar::render(self, ctx);

        // 3. Event log (bottom, above statusbar, toggle-able)
        if self.show_event_log {
            views::event_log::render(self, ctx);
        }

        // 4. Sidebar (left) — hide during wizard screens
        let show_sidebar = self.sidebar_visible
            && !matches!(
                self.screen,
                Screen::BoxSelector | Screen::ArchWizard(_) | Screen::PowerWizard(_)
            );
        if show_sidebar {
            views::sidebar::render(self, ctx);
        }

        // 5. Main content (center)
        // Snapshot screen/view_mode to avoid .clone() on every frame.
        // Screen::VmSettings contains a String, so clone only that variant.
        let screen_snapshot = self.screen.clone();
        let is_console_mode = matches!(self.view_mode, ViewMode::Console);

        egui::CentralPanel::default().show(ctx, |ui| {
            match screen_snapshot {
                Screen::BoxSelector => views::box_selector::render(self, ui),
                Screen::CreateWizard(ref step) => views::wizard::render(self, ui, step),
                Screen::ArchWizard(ref step) => views::arch_wizard::render(self, ui, step),
                Screen::PowerWizard(ref step) => views::power_wizard::render(self, ui, step),
                Screen::VmSettings(ref name) => views::vm_settings::render(self, ui, name),
                Screen::Settings => views::settings::render(self, ui),
                Screen::Home => {
                    if self.selected_vm.is_some() {
                        if is_console_mode {
                            // Console-First: minimal toolbar + full console
                            views::console_toolbar::render(self, ui);
                            if let Some(name) = self.selected_vm.clone() {
                                views::console::render(self, ui, &name);
                            }
                        } else {
                            // Management mode: powerbar + manage tabs
                            views::powerbar::render(self, ui);
                            ui.add_space(4.0);
                            views::manage_view::render(self, ui);
                        }
                    } else {
                        views::home::render(self, ui);
                    }
                },
            }
        });

        // 6. Floating dialogs (Wave 2 + Wave 5 + Wave 6)
        views::clone_dialog::render(self, ctx);
        views::import_export::render_import(self, ctx);
        views::import_export::render_export(self, ctx);
        views::import_wizard::render_import_wizard(self, ctx);
        views::usb_manager::render(self, ctx);
        views::template_manager::render(self, ctx);
        views::remote_hosts::render(self, ctx);
        views::task_panel::render(self, ctx);
        views::port_forward::render(self, ctx);
        views::host_guest::render(self, ctx);
        views::migration::render(self, ctx);
        views::pip::render(self, ctx);
        views::rollback::render(self, ctx);
        views::disk_manage::render(self, ctx);
        views::network_conditioner::render(self, ctx);
        views::guest_tools::render(self, ctx);
        views::media_dialog::render(self, ctx);
        views::network_editor::render(self, ctx);
        views::encryption_dialog::render(self, ctx);
        views::novnc_panel::render(self, ctx);
        views::unattended_wizard::render(self, ctx);
        views::screen_recording::render_settings(self, ctx);
        views::guest_file_manager::render(self, ctx);
        views::pci_passthrough::render(self, ctx);
        views::single_gpu_setup::render(self, ctx);
        views::iso_library::render(self, ctx);
        views::backup_restore::render(self, ctx);
        views::first_run::render(self, ctx);

        // Centralized confirmation modals (snapshot ops + force-stop). Rendered
        // here so they remain visible if the user switches view modes between
        // requesting confirmation and clicking Confirm/Cancel.
        self.render_snapshot_confirm_modal(ctx);
        self.render_force_stop_confirm_modal(ctx);

        // Auto-Pause: pause VM when window loses focus, resume when regained
        if self.auto_pause_enabled {
            let has_focus = ctx.input(|i| i.focused);
            if let Some(ref vm_name) = self.selected_vm.clone() {
                let is_running = self.selected_vm_state() == Some(VmState::Running);
                let is_paused = self.selected_vm_state() == Some(VmState::Paused);

                if !has_focus && is_running && !self.auto_pause_was_running {
                    // Window lost focus — pause
                    self.auto_pause_was_running = true;
                    self.action_pause(vm_name);
                } else if has_focus && is_paused && self.auto_pause_was_running {
                    // Window regained focus — resume
                    self.auto_pause_was_running = false;
                    self.action_resume(vm_name);
                }
            }
        }

        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
}

// Public accessors for views
#[allow(dead_code)]
impl LibreVmmApp {
    pub fn screen(&self) -> &Screen {
        &self.screen
    }
    pub fn set_screen(&mut self, s: Screen) {
        self.screen = s;
    }
    pub fn vms(&self) -> &[VmInfo] {
        &self.vms
    }
    pub fn selected_vm(&self) -> Option<&str> {
        self.selected_vm.as_deref()
    }
    pub fn set_selected_vm(&mut self, name: Option<String>) {
        let changed = self.selected_vm != name;
        // Auto-add to open VM tabs when selecting a VM
        if let Some(ref n) = name {
            self.add_vm_tab(n);
        }
        self.selected_vm = name;
        if changed {
            self.load_selected_config();
            self.snapshots.clear();
            // Reset monitor for new VM
            self.vm_monitor = None;
            self.guest_info = None;

            // Update active box type from VM config
            if let Some(ref cfg) = self.selected_vm_config {
                self.active_box_type = cfg.box_type.clone();
            } else {
                self.active_box_type = BoxType::Standard;
            }

            // Console-First: auto-connect for running VMs, manage view for others
            if let Some(ref vm_name) = self.selected_vm {
                let vm_name = vm_name.clone();
                let is_running = self
                    .vms
                    .iter()
                    .find(|v| v.name == vm_name)
                    .map(|v| v.state == VmState::Running)
                    .unwrap_or(false);

                if is_running {
                    self.auto_connect_console(&vm_name);
                } else {
                    self.view_mode = ViewMode::Manage(ManageTab::Summary);
                    self.disconnect_console_inner();
                }
            } else {
                self.view_mode = ViewMode::Manage(ManageTab::Summary);
                self.disconnect_console_inner();
            }
        }
    }
    pub fn view_mode(&self) -> &ViewMode {
        &self.view_mode
    }
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        // Refresh snapshots when entering snapshot tab
        if let ViewMode::Manage(ManageTab::Snapshots) = &mode {
            if !matches!(&self.view_mode, ViewMode::Manage(ManageTab::Snapshots)) {
                self.refresh_snapshots();
            }
        }
        self.view_mode = mode;
    }
    pub fn switch_to_console(&mut self) {
        if let Some(name) = self.selected_vm.clone() {
            self.open_console(&name);
        }
    }
    pub fn switch_to_manage(&mut self, tab: ManageTab) {
        self.set_view_mode(ViewMode::Manage(tab));
    }
    pub fn sidebar_visible(&self) -> bool {
        self.sidebar_visible
    }
    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
    }
    pub fn show_event_log(&self) -> bool {
        self.show_event_log
    }
    pub fn toggle_event_log(&mut self) {
        self.show_event_log = !self.show_event_log;
    }
    pub fn event_log(&self) -> &VecDeque<EventEntry> {
        &self.event_log
    }
    pub fn clear_event_log(&mut self) {
        self.event_log.clear();
    }
    pub fn latest_event(&self) -> Option<&EventEntry> {
        self.event_log.back()
    }
    pub fn connection_error(&self) -> Option<&str> {
        self.connection_error.as_deref()
    }
    pub fn is_connected(&self) -> bool {
        self.conn.is_some()
    }
    /// Returns a short hypervisor description (type + version) if connected.
    pub fn hypervisor_info(&self) -> Option<String> {
        self.conn.as_ref().and_then(|c| c.hypervisor_info().ok())
    }
    /// Whether KVM acceleration is available on this host.
    pub fn kvm_available(&self) -> bool {
        self.conn
            .as_ref()
            .map(|c| c.kvm_available())
            .unwrap_or(false)
    }
    pub fn confirm_delete(&self) -> Option<&str> {
        self.confirm_delete.as_deref()
    }
    pub fn set_confirm_delete(&mut self, name: Option<String>) {
        self.confirm_delete = name;
    }

    // === Snapshot operation confirmation ===
    pub fn confirm_snapshot_op(&self) -> Option<&(String, String, SnapshotOp)> {
        self.confirm_snapshot_op.as_ref()
    }
    pub fn request_confirm_snapshot_op(&mut self, vm: String, snap: String, op: SnapshotOp) {
        self.confirm_snapshot_op = Some((vm, snap, op));
    }
    pub fn cancel_confirm_snapshot_op(&mut self) {
        self.confirm_snapshot_op = None;
    }
    /// Take the pending snapshot op (if any) and dispatch to the corresponding
    /// action. The op is cleared regardless of success or failure so the modal
    /// closes after the user confirms.
    pub fn confirm_and_execute_snapshot_op(&mut self) {
        let Some((_vm, snap, op)) = self.confirm_snapshot_op.take() else {
            return;
        };
        // We rely on the currently selected VM (matches the `vm` field captured
        // at request time in normal use). If the selection has changed in the
        // meantime, the underlying action_* methods are no-ops, which is the
        // safe outcome.
        match op {
            SnapshotOp::Revert => self.action_revert_snapshot(&snap),
            SnapshotOp::Delete => self.action_delete_snapshot(&snap),
        }
    }

    // === Force-stop confirmation ===
    pub fn confirm_force_stop(&self) -> Option<&str> {
        self.confirm_force_stop.as_deref()
    }
    pub fn request_confirm_force_stop(&mut self, vm: String) {
        self.confirm_force_stop = Some(vm);
    }
    pub fn cancel_confirm_force_stop(&mut self) {
        self.confirm_force_stop = None;
    }
    /// Take the pending force-stop VM name and dispatch the force_stop action.
    pub fn execute_force_stop(&mut self) {
        if let Some(name) = self.confirm_force_stop.take() {
            self.force_stop_vm(&name);
        }
    }

    pub fn search_query(&self) -> &str {
        &self.search_query
    }
    pub fn search_query_mut(&mut self) -> &mut String {
        &mut self.search_query
    }

    // VM config cache
    pub fn selected_vm_config(&self) -> Option<&VmConfig> {
        self.selected_vm_config.as_ref()
    }
    pub fn editing_config(&self) -> Option<&VmConfig> {
        self.editing_config.as_ref()
    }
    pub fn editing_config_mut(&mut self) -> Option<&mut VmConfig> {
        self.editing_config.as_mut()
    }
    pub fn set_editing_config(&mut self, config: Option<VmConfig>) {
        self.editing_config = config;
    }

    // Snapshot state
    pub fn snapshots(&self) -> &[SnapshotInfo] {
        &self.snapshots
    }
    pub fn snapshot_name(&self) -> &str {
        &self.snapshot_name
    }
    pub fn snapshot_name_mut(&mut self) -> &mut String {
        &mut self.snapshot_name
    }
    pub fn snapshot_description(&self) -> &str {
        &self.snapshot_description
    }
    pub fn snapshot_description_mut(&mut self) -> &mut String {
        &mut self.snapshot_description
    }
    pub fn selected_snapshot(&self) -> Option<&str> {
        self.selected_snapshot.as_deref()
    }
    pub fn set_selected_snapshot(&mut self, name: Option<String>) {
        self.selected_snapshot = name;
    }

    /// Prefill the take-snapshot form with the description of an existing
    /// snapshot, so the user can re-take with edits. Libvirt doesn't expose
    /// an in-place description update in the bindings we use, so this is the
    /// closest approximation to "edit description" without a new core fn.
    pub fn action_prefill_snapshot_edit(&mut self, snap_name: &str) {
        if let Some(snap) = self.snapshots.iter().find(|s| s.name == snap_name) {
            self.snapshot_name = format!("{}_edit", snap.name);
            self.snapshot_description = snap.description.clone();
            self.set_status(format!(
                "Loaded '{}' into the take-snapshot form for editing",
                snap.name
            ));
        }
    }

    // Wizard accessors (delegate to wizard sub-struct)
    pub fn wizard_name(&self) -> &str {
        &self.wizard.name
    }
    pub fn wizard_name_mut(&mut self) -> &mut String {
        &mut self.wizard.name
    }
    pub fn wizard_iso(&self) -> &str {
        &self.wizard.iso
    }
    pub fn wizard_iso_mut(&mut self) -> &mut String {
        &mut self.wizard.iso
    }
    pub fn wizard_template_idx(&self) -> usize {
        self.wizard.template_idx
    }
    pub fn set_wizard_template_idx(&mut self, i: usize) {
        self.wizard.template_idx = i;
    }
    pub fn wizard_cpus(&self) -> u32 {
        self.wizard.cpus
    }
    pub fn set_wizard_cpus(&mut self, v: u32) {
        self.wizard.cpus = v;
    }
    pub fn wizard_memory_mib(&self) -> u64 {
        self.wizard.memory_mib
    }
    pub fn set_wizard_memory_mib(&mut self, v: u64) {
        self.wizard.memory_mib = v;
    }
    pub fn wizard_disk_gib(&self) -> u64 {
        self.wizard.disk_gib
    }
    pub fn set_wizard_disk_gib(&mut self, v: u64) {
        self.wizard.disk_gib = v;
    }
    pub fn wizard_network(&self) -> &NetworkMode {
        &self.wizard.network
    }
    pub fn set_wizard_network(&mut self, n: NetworkMode) {
        self.wizard.network = n;
    }
    pub fn wizard_uefi(&self) -> bool {
        self.wizard.uefi
    }
    pub fn set_wizard_uefi(&mut self, v: bool) {
        self.wizard.uefi = v;
    }
    pub fn wizard_description(&self) -> &str {
        &self.wizard.description
    }
    pub fn wizard_description_mut(&mut self) -> &mut String {
        &mut self.wizard.description
    }
    pub fn wizard_template_search(&self) -> &str {
        &self.wizard.template_search
    }
    pub fn wizard_template_search_mut(&mut self) -> &mut String {
        &mut self.wizard.template_search
    }

    // ISO library
    pub fn iso_library_state_mut(&mut self) -> &mut IsoLibraryState {
        &mut self.iso_library_state
    }
    pub fn show_iso_picker(&self) -> bool {
        self.show_iso_picker
    }
    pub fn set_show_iso_picker(&mut self, v: bool) {
        self.show_iso_picker = v;
    }

    // Console accessors
    pub fn console_framebuffer(&self) -> Option<&VncFramebuffer> {
        self.console_fb.as_ref()
    }
    pub fn console_spice_framebuffer(&self) -> Option<&SpiceFramebuffer> {
        self.console_spice_fb.as_ref()
    }
    pub fn console_is_spice(&self) -> bool {
        self.console_is_spice
    }
    pub fn console_vm_name(&self) -> Option<&str> {
        self.console_vm_name.as_deref()
    }
    pub fn console_cache_mut(&mut self) -> &mut views::console::ConsoleCache {
        &mut self.console_cache
    }
    pub fn disconnect_console(&mut self) {
        self.disconnect_console_inner();
    }
    /// Force a VNC-only console connection (bypass SPICE), used as fallback.
    pub fn force_vnc_console(&mut self, name: &str) {
        self.disconnect_console_inner();
        if let Some(ref conn) = self.conn {
            match conn.get_vnc_port(name) {
                Ok(Some(port)) => {
                    let fb = VncFramebuffer::new();
                    fb.connect("127.0.0.1", port);
                    self.console_fb = Some(fb);
                    self.console_is_spice = false;
                    self.console_vm_name = Some(name.to_string());
                    self.view_mode = ViewMode::Console;
                    self.set_status(format!(
                        "Console connected to '{}' (VNC fallback, port {})",
                        name, port
                    ));
                },
                Ok(None) => {
                    self.set_error(format!("No VNC port found for '{}'", name));
                },
                Err(e) => {
                    self.set_error(format!("Failed to get VNC port: {}", e));
                },
            }
        }
    }
    pub fn send_ctrl_alt_del(&self) {
        if let Some(ref fb) = self.console_spice_fb {
            fb.send_key(true, 0xffe3);
            fb.send_key(true, 0xffe9);
            fb.send_key(true, 0xffff);
            fb.send_key(false, 0xffff);
            fb.send_key(false, 0xffe9);
            fb.send_key(false, 0xffe3);
        } else if let Some(ref fb) = self.console_fb {
            fb.send_key(true, 0xffe3);
            fb.send_key(true, 0xffe9);
            fb.send_key(true, 0xffff);
            fb.send_key(false, 0xffff);
            fb.send_key(false, 0xffe9);
            fb.send_key(false, 0xffe3);
        }
    }

    // Action delegates
    pub fn action_start(&mut self, name: &str) {
        self.start_vm(name);
    }
    pub fn action_shutdown(&mut self, name: &str) {
        self.shutdown_vm(name);
    }
    pub fn action_force_stop(&mut self, name: &str) {
        self.force_stop_vm(name);
    }
    pub fn action_pause(&mut self, name: &str) {
        self.pause_vm(name);
    }
    pub fn action_resume(&mut self, name: &str) {
        self.resume_vm(name);
    }
    pub fn action_console(&mut self, name: &str) {
        self.open_console(name);
    }
    pub fn action_delete(&mut self, name: &str) {
        self.delete_vm(name);
    }
    pub fn action_create(&mut self) {
        self.create_vm_from_wizard();
    }
    pub fn action_refresh(&mut self) {
        self.refresh_vms();
    }

    pub fn action_reboot(&mut self, name: &str) {
        if let Some(ref conn) = self.conn {
            match conn.reboot_vm(name) {
                Ok(()) => self.set_status(format!("Reboot signal sent to '{}'", name)),
                Err(e) => self.set_error(format!("Failed to reboot '{}': {}", name, e)),
            }
            self.refresh_vms();
        }
    }

    pub fn action_power_off_all(&mut self) {
        let running: Vec<String> = self
            .vms
            .iter()
            .filter(|v| v.state == VmState::Running || v.state == VmState::Paused)
            .map(|v| v.name.clone())
            .collect();
        if running.is_empty() {
            self.set_status("No running VMs to power off");
            return;
        }
        let count = running.len();
        for name in &running {
            self.force_stop_vm(name);
        }
        self.set_status(format!("{} VMs powered off", count));
    }

    pub fn action_shutdown_all(&mut self) {
        let running: Vec<String> = self
            .vms
            .iter()
            .filter(|v| v.state == VmState::Running)
            .map(|v| v.name.clone())
            .collect();
        if running.is_empty() {
            self.set_status("No running VMs to shut down");
            return;
        }
        let count = running.len();
        for name in &running {
            self.shutdown_vm(name);
        }
        self.set_status(format!("Shutdown sent to {} VMs", count));
    }

    /// Render the centralized confirmation modal for destructive snapshot
    /// operations (revert / delete). Modal stays open until the user clicks
    /// either Confirm or Cancel.
    fn render_snapshot_confirm_modal(&mut self, ctx: &egui::Context) {
        let Some((vm, snap, op)) = self.confirm_snapshot_op.clone() else {
            return;
        };
        let mut confirmed = false;
        let mut cancelled = false;
        egui::Window::new(t!("snap.confirm-title").to_string())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let msg = match op {
                    SnapshotOp::Revert => {
                        t!("snap.confirm-revert", name = &snap, vm = &vm).to_string()
                    },
                    SnapshotOp::Delete => {
                        t!("snap.confirm-delete", name = &snap, vm = &vm).to_string()
                    },
                };
                ui.label(msg);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("common.cancel").to_string()).clicked() {
                        cancelled = true;
                    }
                    let confirm_btn = egui::Button::new(
                        egui::RichText::new(t!("common.ok").to_string())
                            .color(egui::Color32::WHITE),
                    )
                    .fill(theme::AppColors::DANGER);
                    if ui.add(confirm_btn).clicked() {
                        confirmed = true;
                    }
                });
            });
        if confirmed {
            self.confirm_and_execute_snapshot_op();
        } else if cancelled {
            self.cancel_confirm_snapshot_op();
        }
    }

    /// Render the centralized confirmation modal for VM force-stop (Power Off).
    fn render_force_stop_confirm_modal(&mut self, ctx: &egui::Context) {
        let Some(vm) = self.confirm_force_stop.clone() else {
            return;
        };
        let mut confirmed = false;
        let mut cancelled = false;
        egui::Window::new(t!("power.confirm-force-stop-title").to_string())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(t!("power.confirm-force-stop-msg", vm = &vm).to_string());
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("common.cancel").to_string()).clicked() {
                        cancelled = true;
                    }
                    let confirm_btn = egui::Button::new(
                        egui::RichText::new(t!("power.power-off").to_string())
                            .color(egui::Color32::WHITE),
                    )
                    .fill(theme::AppColors::DANGER);
                    if ui.add(confirm_btn).clicked() {
                        confirmed = true;
                    }
                });
            });
        if confirmed {
            self.execute_force_stop();
        } else if cancelled {
            self.cancel_confirm_force_stop();
        }
    }

    pub fn action_take_snapshot(&mut self) {
        let name = self.snapshot_name.clone();
        let desc = self.snapshot_description.clone();
        if name.is_empty() {
            return;
        }
        if let (Some(ref conn), Some(ref vm_name)) = (&self.conn, &self.selected_vm) {
            let vm_name_owned = vm_name.to_string();
            match vmm_core::snapshot::create_snapshot(conn.raw_conn(), &vm_name_owned, &name, &desc)
            {
                Ok(()) => {
                    self.set_status(format!("Snapshot '{}' created", name));
                    vmm_core::notifications::notify_snapshot(
                        &vm_name_owned,
                        "Created",
                        &name,
                        &self.notification_settings,
                    );
                    self.snapshot_name.clear();
                    self.snapshot_description.clear();
                    self.refresh_snapshots();
                },
                Err(e) => self.set_error(format!("Failed to create snapshot: {}", e)),
            }
        }
    }

    pub fn action_revert_snapshot(&mut self, snap_name: &str) {
        if let (Some(ref conn), Some(ref vm_name)) = (&self.conn, &self.selected_vm) {
            match vmm_core::snapshot::revert_snapshot(conn.raw_conn(), vm_name, snap_name) {
                Ok(()) => {
                    self.set_status(format!("Reverted to snapshot '{}'", snap_name));
                    self.refresh_vms();
                    self.refresh_snapshots();
                },
                Err(e) => self.set_error(format!("Failed to revert snapshot: {}", e)),
            }
        }
    }

    pub fn action_delete_snapshot(&mut self, snap_name: &str) {
        if let (Some(ref conn), Some(ref vm_name)) = (&self.conn, &self.selected_vm) {
            match vmm_core::snapshot::delete_snapshot(conn.raw_conn(), vm_name, snap_name) {
                Ok(()) => {
                    self.set_status(format!("Snapshot '{}' deleted", snap_name));
                    self.refresh_snapshots();
                },
                Err(e) => self.set_error(format!("Failed to delete snapshot: {}", e)),
            }
        }
    }

    pub fn action_update_vm(&mut self, config: &VmConfig) {
        // SECURITY: Validate VM name before saving (CWE-20).
        // The settings editor allows editing the VM name, so we must validate
        // just like create_vm_from_wizard does. Without this, a user could
        // inject special characters into the name, which flows into libvirt XML
        // and virsh commands.
        if let Some(err) = vmm_core::config::validate_vm_name(&config.name) {
            self.set_error(format!("Invalid VM name: {}", err));
            return;
        }

        if let Some(ref conn) = self.conn {
            match conn.update_vm(config) {
                Ok(()) => {
                    self.set_status(format!("VM '{}' settings saved", config.name));
                    self.selected_vm_config = Some(config.clone());
                    self.editing_config = None;
                    self.screen = Screen::Home;
                    self.refresh_vms();
                },
                Err(e) => self.set_error(format!("Failed to save settings: {}", e)),
            }
        }
    }

    /// Save metadata-only changes (config file only, no libvirt update).
    /// Safe to call while VM is running — saves description, tags, favorite,
    /// autostart, notes, folder, performance_profile to disk.
    pub fn action_save_metadata(&mut self, config: &VmConfig) {
        // SECURITY: Validate VM name even for metadata saves (defense-in-depth, CWE-20).
        // The name field is read-only in the UI when the VM is running, but we
        // validate here to guard against programmatic misuse or future UI changes.
        if let Some(err) = vmm_core::config::validate_vm_name(&config.name) {
            self.set_error(format!("Invalid VM name: {}", err));
            return;
        }

        match config.save() {
            Ok(()) => {
                self.set_status(format!("Metadata for '{}' saved", config.name));
                self.selected_vm_config = Some(config.clone());

                // Apply autostart if connected (safe while running)
                if let Some(ref conn) = self.conn {
                    let _ = conn.set_autostart(&config.name, config.autostart);
                }

                self.editing_config = None;
                self.screen = Screen::Home;
            },
            Err(e) => self.set_error(format!("Failed to save metadata: {}", e)),
        }
    }

    /// Get the state of the currently selected VM.
    pub fn selected_vm_state(&self) -> Option<VmState> {
        let sel = self.selected_vm.as_deref()?;
        self.vms
            .iter()
            .find(|v| v.name == sel)
            .map(|v| v.state.clone())
    }

    /// Get info for the currently selected VM.
    pub fn selected_vm_info(&self) -> Option<&VmInfo> {
        let sel = self.selected_vm.as_deref()?;
        self.vms.iter().find(|v| v.name == sel)
    }

    /// Whether the currently selected VM has Looking Glass enabled in its config.
    pub fn selected_vm_config_has_looking_glass(&self) -> bool {
        self.selected_vm_config
            .as_ref()
            .map(|c| c.looking_glass.enabled)
            .unwrap_or(false)
    }

    /// Whether the currently selected VM is running.
    pub fn is_selected_vm_running(&self) -> bool {
        self.selected_vm_state() == Some(VmState::Running)
    }

    /// Launch the Looking Glass client for the selected VM. Shows result in event log.
    pub fn action_launch_looking_glass(&mut self) {
        let cfg = match self.selected_vm_config.clone() {
            Some(c) => c,
            None => {
                self.set_error(
                    t!("console.looking-glass-failed", err = "no VM selected").to_string(),
                );
                return;
            },
        };
        if !cfg.looking_glass.enabled {
            self.set_error(
                t!(
                    "console.looking-glass-failed",
                    err = "Looking Glass not enabled"
                )
                .to_string(),
            );
            return;
        }
        match vmm_core::looking_glass::launch_client(&cfg.looking_glass) {
            Ok(_child) => {
                tracing::info!("Looking Glass client launched for '{}'", cfg.name);
                self.set_status(t!("console.looking-glass-launched").to_string());
            },
            Err(e) => {
                tracing::error!("Looking Glass launch failed: {}", e);
                self.set_error(t!("console.looking-glass-failed", err = e).to_string());
            },
        }
    }

    /// Handle keyboard shortcuts.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let modifiers = ctx.input(|i| i.modifiers);
        let keys_pressed: Vec<egui::Key> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| {
                    if let egui::Event::Key {
                        key, pressed: true, ..
                    } = e
                    {
                        Some(*key)
                    } else {
                        None
                    }
                })
                .collect()
        });

        for key in keys_pressed {
            if let Some(name) = self.selected_vm.clone() {
                match (modifiers.ctrl, modifiers.shift, key) {
                    (true, false, egui::Key::P) => {
                        self.force_stop_vm(&name);
                    },
                    (true, false, egui::Key::E) => {
                        self.start_vm(&name);
                    },
                    (true, false, egui::Key::D) => {
                        self.shutdown_vm(&name);
                    },
                    (true, false, egui::Key::U) => {
                        self.pause_vm(&name);
                    },
                    (true, false, egui::Key::R) => {
                        self.resume_vm(&name);
                    },
                    (true, false, egui::Key::S) => {
                        self.action_suspend_to_disk(&name);
                    },
                    (true, false, egui::Key::B) => {
                        if let Some(ref conn) = self.conn {
                            let _ = conn.reboot_vm(&name);
                            self.set_status(format!("Reboot signal sent to '{}'", name));
                            self.refresh_vms();
                        }
                    },
                    // Ctrl+Enter: switch to Console (connect if needed)
                    (true, false, egui::Key::Enter) => {
                        self.open_console(&name);
                    },
                    // Escape disabled — conflicts with UEFI/guest ESC key
                    // (false, false, egui::Key::Escape) => {
                    //     if self.view_mode == ViewMode::Console {
                    //         self.view_mode = ViewMode::Manage(ManageTab::Summary);
                    //     }
                    // }
                    _ => {},
                }
            }

            match (modifiers.ctrl, modifiers.shift, key) {
                (true, true, egui::Key::P) => {
                    self.action_power_off_all();
                },
                (true, true, egui::Key::D) => {
                    self.action_shutdown_all();
                },
                (false, false, egui::Key::F5) => {
                    self.refresh_vms();
                },
                _ => {},
            }
        }
    }

    pub fn action_apply_template(&mut self, idx: usize) {
        let templates = builtin_templates();
        if let Some(t) = templates.get(idx) {
            self.wizard.cpus = t.recommended_cpus;
            self.wizard.memory_mib = t.recommended_memory_mib;
            self.wizard.disk_gib = t.recommended_disk_gib;
            self.wizard.uefi = t.uefi;
            self.wizard.template_idx = idx;
        }
    }

    // ===== Wave 2: Clone, Import/Export, USB =====

    // Clone dialog
    pub fn clone_dialog_state(&self) -> &CloneDialogState {
        &self.clone_dialog
    }
    pub fn clone_dialog_state_mut(&mut self) -> &mut CloneDialogState {
        &mut self.clone_dialog
    }

    pub fn action_clone_vm(&mut self) {
        let source_name = self.clone_dialog.source_vm_name.clone();
        let new_name = self.clone_dialog.new_name.clone();
        let clone_type = self.clone_dialog.clone_type.clone();

        // Find source config
        let source_config = if let Some(ref cfg) = self.selected_vm_config {
            cfg.clone()
        } else {
            self.clone_dialog.error = Some("No config loaded for source VM".to_string());
            return;
        };

        self.clone_dialog.in_progress = true;

        if let Some(ref conn) = self.conn {
            match vmm_core::clone::clone_vm(conn, &source_config, &new_name, &clone_type) {
                Ok(_new_config) => {
                    self.set_status(format!(
                        "VM '{}' cloned as '{}' ({})",
                        source_name, new_name, clone_type
                    ));
                    self.clone_dialog.close();
                    self.refresh_vms();
                },
                Err(e) => {
                    self.clone_dialog.error = Some(e.to_string());
                    self.clone_dialog.in_progress = false;
                },
            }
        }
    }

    // Import/Export
    pub fn import_export_state(&self) -> &ImportExportState {
        &self.import_export
    }
    pub fn import_export_state_mut(&mut self) -> &mut ImportExportState {
        &mut self.import_export
    }

    // Import Wizard
    pub fn import_wizard_state(&self) -> &Option<ImportWizardState> {
        &self.import_wizard
    }
    pub fn import_wizard_state_mut(&mut self) -> &mut Option<ImportWizardState> {
        &mut self.import_wizard
    }
    pub fn open_import_wizard(&mut self) {
        self.import_wizard = Some(ImportWizardState::default());
    }
    pub fn close_import_wizard(&mut self) {
        self.import_wizard = None;
    }

    pub fn action_import_ova(&mut self) {
        let path = self.import_export.import_path.clone();
        let name_override = if self.import_export.import_name.is_empty() {
            None
        } else {
            Some(self.import_export.import_name.clone())
        };

        self.import_export.in_progress = true;

        if let Some(ref conn) = self.conn {
            match vmm_core::ova::import_ova(conn, &path, name_override.as_deref()) {
                Ok(config) => {
                    self.set_status(format!("VM '{}' imported from OVA", config.name));
                    self.import_export.close();
                    self.refresh_vms();
                },
                Err(e) => {
                    self.import_export.error = Some(e.to_string());
                    self.import_export.in_progress = false;
                },
            }
        }
    }

    /// Execute a VM import from the import wizard.
    ///
    /// Runs `vmm_core::import::execute_import` to copy/symlink/move/convert the
    /// source disk and write the VM config, then defines the libvirt domain
    /// from the resulting `VmConfig` so the new VM appears in the sidebar.
    pub fn action_execute_import(
        &mut self,
        imported: &vmm_core::import::ImportedVm,
        disk_action: vmm_core::import::DiskAction,
        vm_name: &str,
    ) -> Result<(), String> {
        match vmm_core::import::execute_import(imported, disk_action, vm_name) {
            Ok(config) => {
                // Register the imported VM with libvirt so it appears in the sidebar.
                if let Some(ref conn) = self.conn {
                    if let Err(e) = conn.create_vm_from_existing(&config) {
                        let msg = format!("Failed to register imported VM with libvirt: {}", e);
                        self.set_error(msg.clone());
                        return Err(msg);
                    }
                }
                self.refresh_vms();
                self.set_status(format!("VM '{}' imported successfully", vm_name));
                Ok(())
            },
            Err(e) => {
                self.set_error(format!("Import failed: {}", e));
                Err(e)
            },
        }
    }

    pub fn action_export_ova(&mut self) {
        use crate::views::import_export::ExportFormat;

        let vm_name = self.import_export.export_vm_name.clone();
        let output_path = self.import_export.export_path.clone();
        let format = self.import_export.export_format.clone();

        // Find config
        let config = if let Some(ref cfg) = self.selected_vm_config {
            cfg.clone()
        } else {
            self.import_export.error = Some("No config loaded for VM".to_string());
            return;
        };

        self.import_export.in_progress = true;

        let result = match format {
            ExportFormat::Ova => vmm_core::ova::export_ova(&config, &output_path),
            _ => {
                // Direct disk conversion for non-OVA formats
                vmm_core::disk::convert_disk(&config.disk_path, &output_path, format.qemu_format())
            },
        };

        match result {
            Ok(()) => {
                self.set_status(format!("VM '{}' exported to {}", vm_name, output_path));
                self.import_export.close();
            },
            Err(e) => {
                self.import_export.error = Some(e.to_string());
                self.import_export.in_progress = false;
            },
        }
    }

    // USB Manager
    pub fn usb_manager_state(&self) -> &UsbManagerState {
        &self.usb_manager
    }
    pub fn usb_manager_state_mut(&mut self) -> &mut UsbManagerState {
        &mut self.usb_manager
    }

    // PCI Passthrough
    pub fn pci_passthrough_state(&self) -> &PciPassthroughState {
        &self.pci_passthrough
    }
    pub fn pci_passthrough_state_mut(&mut self) -> &mut PciPassthroughState {
        &mut self.pci_passthrough
    }

    // Wave 12.2: Single-GPU passthrough wizard
    pub fn single_gpu_state(&self) -> &SingleGpuWizardState {
        &self.single_gpu
    }
    pub fn single_gpu_state_mut(&mut self) -> &mut SingleGpuWizardState {
        &mut self.single_gpu
    }

    // Backup & Restore
    pub fn backup_restore_state(&self) -> Option<&BackupRestoreState> {
        Some(&self.backup_restore)
    }
    pub fn backup_restore_state_mut(&mut self) -> &mut BackupRestoreState {
        &mut self.backup_restore
    }

    // VM tab management
    pub fn open_vm_tabs(&self) -> &[String] {
        &self.open_vm_tabs
    }
    pub fn add_vm_tab(&mut self, name: &str) {
        if !self.open_vm_tabs.iter().any(|t| t == name) {
            self.open_vm_tabs.push(name.to_string());
        }
    }
    pub fn close_vm_tab(&mut self, name: &str) {
        self.open_vm_tabs.retain(|t| t != name);
    }

    pub fn action_attach_usb(&mut self, vendor_id: &str, product_id: &str) {
        if let (Some(ref conn), Some(ref name)) = (&self.conn, &self.selected_vm) {
            match vmm_core::usb::attach_usb_device(conn.raw_conn(), name, vendor_id, product_id) {
                Ok(()) => {
                    self.set_status(format!(
                        "USB {}:{} attached to '{}'",
                        vendor_id, product_id, name
                    ));
                    self.usb_manager.refresh();
                },
                Err(e) => {
                    self.usb_manager.error = Some(e.to_string());
                    self.set_error(format!("USB attach failed: {}", e));
                },
            }
        }
    }

    pub fn action_detach_usb(&mut self, vendor_id: &str, product_id: &str) {
        if let (Some(ref conn), Some(ref name)) = (&self.conn, &self.selected_vm) {
            match vmm_core::usb::detach_usb_device(conn.raw_conn(), name, vendor_id, product_id) {
                Ok(()) => {
                    self.set_status(format!(
                        "USB {}:{} detached from '{}'",
                        vendor_id, product_id, name
                    ));
                    self.usb_manager.refresh();
                },
                Err(e) => {
                    self.usb_manager.error = Some(e.to_string());
                    self.set_error(format!("USB detach failed: {}", e));
                },
            }
        }
    }

    // ===== Wave 3: Performance Monitoring, Guest Agent, Library Org =====

    // Monitor
    pub fn monitor_state(&self) -> &MonitorState {
        &self.monitor_state
    }
    pub fn monitor_state_mut(&mut self) -> &mut MonitorState {
        &mut self.monitor_state
    }
    pub fn vm_monitor(&self) -> Option<&VmMonitor> {
        self.vm_monitor.as_ref()
    }
    pub fn vm_monitor_mut(&mut self) -> Option<&mut VmMonitor> {
        self.vm_monitor.as_mut()
    }

    pub fn poll_monitor(&mut self, vm_name: &str) {
        if let Some(ref conn) = self.conn {
            let monitor = self
                .vm_monitor
                .get_or_insert_with(|| VmMonitor::new(vm_name));

            // Reset monitor if VM changed
            if monitor.vm_name != vm_name {
                *monitor = VmMonitor::new(vm_name);
            }

            let _ = monitor.poll(conn.raw_conn());
            self.monitor_state.last_poll = std::time::Instant::now();
        }
    }

    // Guest Agent
    pub fn guest_info(&self) -> Option<&GuestInfo> {
        self.guest_info.as_ref()
    }

    pub fn refresh_guest_info(&mut self) {
        if let (Some(ref conn), Some(ref name)) = (&self.conn, &self.selected_vm) {
            self.guest_info = Some(vmm_core::guest_agent::query_guest_info(
                conn.raw_conn(),
                name,
            ));
        }
    }

    // Sidebar sorting & library
    pub fn sidebar_sort(&self) -> &SidebarSort {
        &self.sidebar_sort
    }
    pub fn set_sidebar_sort(&mut self, sort: SidebarSort) {
        self.sidebar_sort = sort;
    }

    // ===== Wave 4: Suspend to Disk, GPU, Encryption =====

    /// Suspend a running VM to disk (managed save).
    pub fn action_suspend_to_disk(&mut self, name: &str) {
        if let Some(ref conn) = self.conn {
            match conn.suspend_to_disk(name) {
                Ok(()) => {
                    self.set_status(format!("VM '{}' suspended to disk", name));
                    self.managed_save_cache.insert(name.to_string(), true);
                },
                Err(e) => self.set_error(format!("Failed to suspend '{}': {}", name, e)),
            }
            self.refresh_vms();
        }
    }

    /// Discard a managed save image (start fresh next time).
    pub fn action_discard_save(&mut self, name: &str) {
        if let Some(ref conn) = self.conn {
            match conn.discard_managed_save(name) {
                Ok(()) => {
                    self.set_status(format!("Saved state discarded for '{}'", name));
                    self.managed_save_cache.insert(name.to_string(), false);
                },
                Err(e) => self.set_error(format!("Failed to discard save: {}", e)),
            }
        }
    }

    /// Whether a VM has a managed save image.
    pub fn has_managed_save(&self, name: &str) -> bool {
        self.managed_save_cache.get(name).copied().unwrap_or(false)
    }

    /// Get cached GPU capabilities (lazy-loaded).
    pub fn gpu_capabilities(&self) -> Option<&GpuCapabilities> {
        self.gpu_capabilities.as_ref()
    }

    /// Detect and cache GPU capabilities.
    pub fn detect_gpu(&mut self) {
        if self.gpu_capabilities.is_none() {
            self.gpu_capabilities = Some(vmm_core::gpu::detect_gpu_capabilities());
        }
    }

    /// Encryption passphrase for the current dialog.
    pub fn encryption_passphrase(&self) -> &str {
        &self.encryption_passphrase
    }
    pub fn encryption_passphrase_mut(&mut self) -> &mut String {
        &mut self.encryption_passphrase
    }

    /// SECURITY: CWE-316 / CWE-244 — Zeroize the encryption passphrase from GUI memory.
    /// Must be called after the passphrase has been consumed (passed to vmm-core)
    /// and whenever the encryption dialog is closed/cancelled.
    pub fn clear_encryption_passphrase(&mut self) {
        // Overwrite the backing buffer with zeros using volatile writes
        // to prevent the compiler from optimizing away the clear.
        let bytes = unsafe { self.encryption_passphrase.as_mut_vec() };
        for byte in bytes.iter_mut() {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        self.encryption_passphrase.clear();
    }

    /// Toggle favorite status for a VM config.
    pub fn toggle_vm_favorite(&mut self, vm_name: &str) {
        // Find the VM's UUID and toggle favorite in config
        if let Some(vm) = self.vms.iter().find(|v| v.name == vm_name) {
            if let Ok(uuid) = uuid::Uuid::parse_str(&vm.uuid) {
                if let Ok(mut config) = VmConfig::load(&uuid) {
                    config.favorite = !config.favorite;
                    let _ = config.save();
                    // Refresh cached config if it's the selected one
                    if self.selected_vm.as_deref() == Some(vm_name) {
                        self.selected_vm_config = Some(config.clone());
                    }
                    // Update sidebar config cache so favorite change is visible immediately
                    if let Some(cached) =
                        self.vm_configs_cache.iter_mut().find(|c| c.name == vm_name)
                    {
                        cached.favorite = config.favorite;
                    }
                }
            }
        }
    }

    // ===== Wave 5: Templates & Remote =====

    pub fn template_manager_state(&self) -> &TemplateManagerState {
        &self.template_manager
    }
    pub fn template_manager_state_mut(&mut self) -> &mut TemplateManagerState {
        &mut self.template_manager
    }

    pub fn remote_hosts_state(&self) -> &RemoteHostsState {
        &self.remote_hosts
    }
    pub fn remote_hosts_state_mut(&mut self) -> &mut RemoteHostsState {
        &mut self.remote_hosts
    }

    /// Current remote host name (None = local).
    pub fn remote_host_name(&self) -> Option<&str> {
        self.remote_host_name.as_deref()
    }

    /// Connect to a remote hypervisor.
    pub fn action_connect_remote(&mut self, uri: &str, host_name: &str) {
        match HypervisorConnection::connect_remote(uri) {
            Ok(conn) => {
                self.conn = Some(conn);
                self.connection_error = None;
                self.remote_host_name = Some(host_name.to_string());
                self.set_status(format!("Connected to remote host '{}'", host_name));
                self.selected_vm = None;
                self.refresh_vms();
            },
            Err(e) => {
                self.set_error(format!("Failed to connect to '{}': {}", host_name, e));
                self.remote_hosts.error = Some(e.to_string());
            },
        }
    }

    // ===== Wave 6: Task System, Port Forwarding, TPM =====

    pub fn task_manager(&self) -> &TaskManager {
        &self.task_manager
    }
    pub fn task_manager_mut(&mut self) -> &mut TaskManager {
        &mut self.task_manager
    }
    pub fn show_task_panel(&self) -> bool {
        self.show_task_panel
    }
    pub fn set_show_task_panel(&mut self, v: bool) {
        self.show_task_panel = v;
    }

    pub fn port_forward_state(&self) -> &PortForwardState {
        &self.port_forward
    }
    pub fn port_forward_state_mut(&mut self) -> &mut PortForwardState {
        &mut self.port_forward
    }

    /// Open port forward editor populated from current VM config.
    pub fn action_open_port_forwards(&mut self) {
        if let Some(ref config) = self.selected_vm_config {
            self.port_forward.rules = config.port_forwards.clone();
            self.port_forward.open = true;
            self.port_forward.error = None;
        }
    }

    /// Save port forward rules back to the VM config.
    pub fn action_save_port_forwards(&mut self) {
        let rules = self.port_forward.rules.clone();
        let save_result = if let Some(ref config) = self.selected_vm_config {
            // Save to disk first — only update in-memory state on success
            // to prevent state/disk mismatch on save failure.
            let mut config_for_save = config.clone();
            config_for_save.port_forwards = rules.clone();
            match config_for_save.save() {
                Ok(()) => Ok(rules.len()),
                Err(e) => Err(e),
            }
        } else {
            return;
        };
        match save_result {
            Ok(count) => {
                if let Some(ref mut config) = self.selected_vm_config {
                    config.port_forwards = rules;
                }
                self.set_status(format!("Port forwarding rules saved ({} rules)", count));
                self.port_forward.open = false;
            },
            Err(e) => {
                self.set_error(format!("Failed to save port forward rules: {}", e));
            },
        }
    }

    // ===== Wave 6+: Notifications, Host-Guest Integration =====

    pub fn notification_settings(&self) -> &NotificationSettings {
        &self.notification_settings
    }
    pub fn notification_settings_mut(&mut self) -> &mut NotificationSettings {
        &mut self.notification_settings
    }

    pub fn host_guest_state(&self) -> &HostGuestState {
        &self.host_guest
    }
    pub fn host_guest_state_mut(&mut self) -> &mut HostGuestState {
        &mut self.host_guest
    }

    /// Open the host-guest integration panel.
    pub fn action_open_host_guest(&mut self) {
        self.host_guest.open = true;
    }

    // ===== Wave 6: Live Migration =====

    pub fn migration_state(&self) -> &MigrationState {
        &self.migration
    }
    pub fn migration_state_mut(&mut self) -> &mut MigrationState {
        &mut self.migration
    }

    /// Open the migration dialog for the selected VM.
    pub fn action_open_migration(&mut self) {
        if let Some(ref name) = self.selected_vm {
            self.migration.open(&name.clone());
        }
    }

    /// Start the migration (called from migration dialog).
    pub fn action_start_migration(&mut self) {
        let vm_name = self.migration.vm_name.clone();
        let host_idx = match self.migration.selected_host_idx {
            Some(idx) => idx,
            None => return,
        };
        let hosts = self.migration.remote_hosts.clone().unwrap_or_default();
        let host = match hosts.hosts.get(host_idx) {
            Some(h) => h.clone(),
            None => return,
        };
        let options = self.migration.options.clone();

        // SECURITY: CWE-404 — Store both progress handle and thread JoinHandle
        // for orderly cleanup. Previously the JoinHandle was silently dropped.
        let handle = vmm_core::migration::migrate_vm(&vm_name, &host, &options);
        self.migration.progress = Some(handle.progress);
        self.migration.migration_thread = handle.join_handle;
        self.set_status(format!(
            "Migration of '{}' to '{}' started",
            vm_name, host.name
        ));
        vmm_core::notifications::notify_task_complete(
            &format!("Migration of '{}' started", vm_name),
            true,
            &self.notification_settings,
        );
    }

    /// Reconnect to the local hypervisor.
    pub fn action_connect_local(&mut self) {
        match HypervisorConnection::connect_best() {
            Ok(conn) => {
                self.conn = Some(conn);
                self.connection_error = None;
                self.remote_host_name = None;
                self.set_status("Reconnected to local hypervisor");
                self.selected_vm = None;
                self.refresh_vms();
            },
            Err(e) => {
                self.conn = None;
                self.connection_error = Some(e.to_string());
                self.remote_host_name = None;
            },
        }
    }

    // ===== UI Scale =====

    pub fn ui_scale(&self) -> f32 {
        self.ui_scale
    }

    pub fn set_ui_scale(&mut self, scale: f32) {
        self.ui_scale = scale;
        save_ui_scale(scale);
    }

    // ===== Boxes System =====

    /// Get the active box type (drives UI accent color).
    pub fn active_box_type(&self) -> &BoxType {
        &self.active_box_type
    }

    /// Set the active box type and update the UI accent.
    pub fn set_active_box_type(&mut self, bt: BoxType) {
        self.active_box_type = bt;
    }

    /// Get the wizard's current box type.
    pub fn wizard_box_type(&self) -> &BoxType {
        &self.wizard.box_type
    }

    /// Set the box type for the wizard (called from BoxSelector).
    pub fn set_wizard_box_type(&mut self, bt: BoxType) {
        self.wizard.box_type = bt.clone();
        self.active_box_type = bt;
    }

    // Architecture wizard accessors (Box 2: Hardware Lab)

    pub fn arch_wizard_arch(&self) -> Option<&QemuArch> {
        self.arch_wizard.arch.as_ref()
    }
    pub fn set_arch_wizard_arch(&mut self, arch: QemuArch) {
        // When architecture changes, reset machine and CPU to defaults
        let default_machine = arch.default_machine().to_string();
        let default_cpu = arch.default_cpu().to_string();
        let use_kvm = arch.can_use_kvm_on_x86();
        self.arch_wizard.machine = default_machine;
        self.arch_wizard.cpu = default_cpu;
        self.arch_wizard.use_kvm = use_kvm;
        self.arch_wizard.arch = Some(arch);
    }

    pub fn arch_wizard_machine(&self) -> &str {
        &self.arch_wizard.machine
    }
    pub fn set_arch_wizard_machine(&mut self, m: String) {
        self.arch_wizard.machine = m;
    }

    pub fn arch_wizard_cpu(&self) -> &str {
        &self.arch_wizard.cpu
    }
    pub fn set_arch_wizard_cpu(&mut self, c: String) {
        self.arch_wizard.cpu = c;
    }

    pub fn arch_wizard_use_kvm(&self) -> bool {
        self.arch_wizard.use_kvm
    }
    pub fn set_arch_wizard_use_kvm(&mut self, v: bool) {
        self.arch_wizard.use_kvm = v;
    }

    pub fn arch_wizard_show_all(&self) -> bool {
        self.arch_wizard.show_all
    }
    pub fn set_arch_wizard_show_all(&mut self, v: bool) {
        self.arch_wizard.show_all = v;
    }

    pub fn arch_cpu_topology(&self) -> Option<&vmm_core::config::CpuTopology> {
        self.arch_wizard.cpu_topology.as_ref()
    }
    pub fn set_arch_cpu_topology(&mut self, t: Option<vmm_core::config::CpuTopology>) {
        self.arch_wizard.cpu_topology = t;
    }

    pub fn arch_cpu_features(&self) -> &[String] {
        &self.arch_wizard.cpu_features
    }
    pub fn arch_cpu_features_mut(&mut self) -> &mut Vec<String> {
        &mut self.arch_wizard.cpu_features
    }

    pub fn toggle_arch_cpu_feature(&mut self, feature: &str) {
        if let Some(pos) = self
            .arch_wizard
            .cpu_features
            .iter()
            .position(|f| f == feature)
        {
            self.arch_wizard.cpu_features.remove(pos);
        } else {
            self.arch_wizard.cpu_features.push(feature.to_string());
        }
    }

    pub fn has_arch_cpu_feature(&self, feature: &str) -> bool {
        self.arch_wizard.cpu_features.iter().any(|f| f == feature)
    }

    /// Apply architecture-specific defaults to the wizard fields.
    pub fn apply_arch_defaults(&mut self) {
        if let Some(ref arch) = self.arch_wizard.arch {
            let defaults = arch.recommended_defaults();
            self.wizard.cpus = defaults.cpus;
            self.wizard.memory_mib = defaults.memory_mib;
            self.wizard.disk_gib = defaults.disk_gib;
            self.wizard.uefi = defaults.uefi;
            // Set a default name if empty
            if self.wizard.name.is_empty() {
                self.wizard.name = format!(
                    "{} VM",
                    arch.display_name().split(' ').next().unwrap_or("New")
                );
            }
        }
    }

    /// Create a VM from the architecture wizard state (Box 2: Hardware Lab).
    pub fn action_create_arch_vm(&mut self) {
        let arch = match &self.arch_wizard.arch {
            Some(a) => a.clone(),
            None => return,
        };

        // Validate VM name before creation
        if let Some(err) = vmm_core::config::validate_vm_name(&self.wizard.name) {
            self.set_error(format!("Invalid VM name: {}", err));
            return;
        }

        let mut config = VmConfig::from_arch(
            &self.wizard.name,
            &arch,
            &self.arch_wizard.machine,
            &self.arch_wizard.cpu,
        );

        // Apply user customizations from wizard
        config.vcpus = self.wizard.cpus;
        config.memory_mib = self.wizard.memory_mib;
        config.disk_size_gib = self.wizard.disk_gib;
        config.network = self.wizard.network.clone();
        config.uefi = self.wizard.uefi;
        config.description = self.wizard.description.clone();
        config.use_kvm = self.arch_wizard.use_kvm;
        config.cpu_topology = self.arch_wizard.cpu_topology.clone();
        config.cpu_features = self.arch_wizard.cpu_features.clone();

        if !self.wizard.iso.is_empty() {
            config.iso_path = Some(self.wizard.iso.clone());
        }

        if let Some(ref conn) = self.conn {
            match conn.create_vm(&config) {
                Ok(()) => {
                    self.set_status(format!(
                        "VM '{}' created ({} / {})",
                        config.name,
                        arch.display_name(),
                        self.arch_wizard.machine,
                    ));
                    self.screen = Screen::Home;
                    self.refresh_vms();
                    self.reset_wizard();
                    self.reset_arch_wizard();
                },
                Err(e) => self.set_error(format!("Failed to create VM: {}", e)),
            }
        }
    }

    /// Reset architecture wizard state.
    fn reset_arch_wizard(&mut self) {
        self.arch_wizard = ArchWizardState::default();
    }

    // ===== Box 3: Power User wizard =====

    pub fn power_cpu_topology(&self) -> Option<&vmm_core::config::CpuTopology> {
        self.power_wizard.cpu_topology.as_ref()
    }
    pub fn set_power_cpu_topology(&mut self, t: Option<vmm_core::config::CpuTopology>) {
        self.power_wizard.cpu_topology = t;
    }
    pub fn power_hugepages(&self) -> bool {
        self.power_wizard.hugepages
    }
    pub fn set_power_hugepages(&mut self, v: bool) {
        self.power_wizard.hugepages = v;
    }
    pub fn power_disk_cache(&self) -> &str {
        &self.power_wizard.disk_cache
    }
    pub fn set_power_disk_cache(&mut self, v: String) {
        self.power_wizard.disk_cache = v;
    }
    pub fn power_disk_io_mode(&self) -> &str {
        &self.power_wizard.disk_io_mode
    }
    pub fn set_power_disk_io_mode(&mut self, v: String) {
        self.power_wizard.disk_io_mode = v;
    }
    pub fn power_io_threads(&self) -> u32 {
        self.power_wizard.io_threads
    }
    pub fn set_power_io_threads(&mut self, v: u32) {
        self.power_wizard.io_threads = v;
    }
    pub fn power_vfio_devices(&self) -> &[vmm_core::config::VfioDeviceConfig] {
        &self.power_wizard.vfio_devices
    }
    pub fn power_vfio_input(&self) -> &str {
        &self.power_wizard.vfio_input
    }
    pub fn power_vfio_input_mut(&mut self) -> &mut String {
        &mut self.power_wizard.vfio_input
    }
    pub fn power_add_vfio_device(&mut self, dev: vmm_core::config::VfioDeviceConfig) {
        self.power_wizard.vfio_devices.push(dev);
        self.power_wizard.vfio_input.clear();
    }
    pub fn power_remove_vfio_device(&mut self, idx: usize) {
        if idx < self.power_wizard.vfio_devices.len() {
            self.power_wizard.vfio_devices.remove(idx);
        }
    }
    pub fn power_custom_args(&self) -> &[String] {
        &self.power_wizard.custom_args
    }
    pub fn power_custom_arg_input(&self) -> &str {
        &self.power_wizard.custom_arg_input
    }
    pub fn power_custom_arg_input_mut(&mut self) -> &mut String {
        &mut self.power_wizard.custom_arg_input
    }
    pub fn power_add_custom_arg(&mut self, arg: String) {
        self.power_wizard.custom_args.push(arg);
        self.power_wizard.custom_arg_input.clear();
    }
    pub fn power_remove_custom_arg(&mut self, idx: usize) {
        if idx < self.power_wizard.custom_args.len() {
            self.power_wizard.custom_args.remove(idx);
        }
    }
    pub fn power_nic_model(&self) -> &str {
        &self.power_wizard.nic_model
    }
    pub fn set_power_nic_model(&mut self, v: String) {
        self.power_wizard.nic_model = v;
    }
    pub fn power_tpm_enabled(&self) -> bool {
        self.power_wizard.tpm_enabled
    }
    pub fn set_power_tpm_enabled(&mut self, v: bool) {
        self.power_wizard.tpm_enabled = v;
    }
    pub fn power_gpu_accel(&self) -> bool {
        self.power_wizard.gpu_accel
    }
    pub fn set_power_gpu_accel(&mut self, v: bool) {
        self.power_wizard.gpu_accel = v;
    }
    pub fn power_display_protocol(&self) -> vmm_core::config::DisplayProtocol {
        self.power_wizard.display_protocol
    }
    pub fn set_power_display_protocol(&mut self, v: vmm_core::config::DisplayProtocol) {
        self.power_wizard.display_protocol = v;
    }

    /// Create a VM from the Power User wizard state (Box 3).
    pub fn action_create_power_vm(&mut self) {
        let templates = builtin_templates();
        let template = match templates.get(self.wizard.template_idx) {
            Some(t) => t,
            None => {
                self.set_error(format!(
                    "Invalid template index: {}",
                    self.wizard.template_idx
                ));
                return;
            },
        };

        // Validate VM name before creation
        if let Some(err) = vmm_core::config::validate_vm_name(&self.wizard.name) {
            self.set_error(format!("Invalid VM name: {}", err));
            return;
        }

        let mut config = VmConfig::from_template(&self.wizard.name, template, None);

        // Override with power user settings
        config.box_type = vmm_core::qemu_archs::BoxType::PowerUser;
        config.vcpus = self.wizard.cpus;
        config.memory_mib = self.wizard.memory_mib;
        config.disk_size_gib = self.wizard.disk_gib;
        config.network = self.wizard.network.clone();
        config.uefi = self.wizard.uefi;
        config.description = self.wizard.description.clone();

        // Power user specific
        config.cpu_topology = self.power_wizard.cpu_topology.clone();
        config.hugepages = self.power_wizard.hugepages;
        config.disk_cache = self.power_wizard.disk_cache.clone();
        config.disk_io_mode = self.power_wizard.disk_io_mode.clone();
        config.io_threads = self.power_wizard.io_threads;
        config.vfio_devices = self.power_wizard.vfio_devices.clone();
        config.custom_qemu_args = self.power_wizard.custom_args.clone();
        config.tpm_enabled = self.power_wizard.tpm_enabled;
        config.gpu_accel = self.power_wizard.gpu_accel;
        config.display_protocol = self.power_wizard.display_protocol;

        // NIC model
        if self.wizard.network != NetworkMode::None {
            config.network_interfaces = vec![vmm_core::config::NicConfig {
                mode: self.wizard.network.clone(),
                model: self.power_wizard.nic_model.clone(),
                mac: String::new(),
            }];
        }

        if !self.wizard.iso.is_empty() {
            config.iso_path = Some(self.wizard.iso.clone());
        }

        if let Some(ref conn) = self.conn {
            match conn.create_vm(&config) {
                Ok(()) => {
                    self.set_status(format!("Power VM '{}' created successfully!", config.name,));
                    self.screen = Screen::Home;
                    self.refresh_vms();
                    self.reset_wizard();
                    self.reset_power_wizard();
                },
                Err(e) => self.set_error(format!("Failed to create VM: {}", e)),
            }
        }
    }

    /// Reset power wizard state.
    fn reset_power_wizard(&mut self) {
        self.power_wizard = PowerWizardState::default();
    }

    // ===== Parallels-inspired: Picture-in-Picture =====

    pub fn pip_state(&self) -> &PipState {
        &self.pip
    }
    pub fn pip_state_mut(&mut self) -> &mut PipState {
        &mut self.pip
    }

    pub fn action_toggle_pip(&mut self) {
        self.pip.open = !self.pip.open;
    }

    // ===== Parallels-inspired: Rollback Mode =====

    pub fn rollback_state(&self) -> &RollbackState {
        &self.rollback
    }
    pub fn rollback_state_mut(&mut self) -> &mut RollbackState {
        &mut self.rollback
    }

    pub fn action_open_rollback(&mut self) {
        self.rollback.open = true;
        if let Some(name) = self.selected_vm.clone() {
            self.action_refresh_rollback_points(&name);
        }
    }

    pub fn action_refresh_rollback_points(&mut self, vm_name: &str) {
        if let Some(ref conn) = self.conn {
            match vmm_core::rollback::list_rollback_points(conn.raw_conn(), vm_name) {
                Ok(points) => {
                    self.rollback.rollback_points = points
                        .into_iter()
                        .map(|s| crate::views::rollback::RollbackPoint {
                            name: s.name.clone(),
                            timestamp: s.creation_time.to_string(),
                            description: s.description,
                        })
                        .collect();
                },
                Err(e) => {
                    self.rollback.rollback_points.clear();
                    self.push_event(true, format!("Failed to list rollback points: {}", e));
                },
            }
        }
    }

    pub fn action_create_rollback_point(&mut self, vm_name: &str) {
        if let Some(ref conn) = self.conn {
            match vmm_core::rollback::create_rollback_point(conn.raw_conn(), vm_name) {
                Ok(name) => {
                    self.set_status(format!("Rollback point '{}' created", name));
                    self.action_refresh_rollback_points(vm_name);
                },
                Err(e) => self.set_error(format!("Failed to create rollback point: {}", e)),
            }
        }
    }

    pub fn action_revert_rollback(&mut self, vm_name: &str, snap_name: &str) {
        if let Some(ref conn) = self.conn {
            match vmm_core::snapshot::revert_snapshot(conn.raw_conn(), vm_name, snap_name) {
                Ok(()) => {
                    self.set_status(format!("Reverted to '{}'", snap_name));
                    self.refresh_vms();
                    self.action_refresh_rollback_points(vm_name);
                },
                Err(e) => self.set_error(format!("Failed to revert: {}", e)),
            }
        }
    }

    // ===== Parallels-inspired: Disk Management =====

    pub fn disk_manage_state(&self) -> &DiskManageState {
        &self.disk_manage
    }
    pub fn disk_manage_state_mut(&mut self) -> &mut DiskManageState {
        &mut self.disk_manage
    }

    pub fn show_disk_resize_dialog(&mut self) {
        self.action_open_disk_manage();
    }

    pub fn action_open_disk_manage(&mut self) {
        self.disk_manage.open = true;
        // Auto-analyze on open
        if let Some(ref config) = self.selected_vm_config {
            let path = config.disk_path.clone();
            self.action_analyze_disk(&path);
        }
    }

    pub fn action_analyze_disk(&mut self, disk_path: &str) {
        match vmm_core::disk_manage::analyze_disk(disk_path) {
            Ok(info) => {
                self.disk_manage.virtual_size = info.virtual_size_bytes;
                self.disk_manage.actual_size = info.actual_size_bytes;
                self.disk_manage.wasted = info.wasted_bytes;
                self.disk_manage.format = info.format;
            },
            Err(e) => self.set_error(format!("Disk analysis failed: {}", e)),
        }
    }

    pub fn action_compact_disk(&mut self, disk_path: &str) {
        self.disk_manage.compacting = true;
        self.disk_manage.compact_result = None;
        let path = disk_path.to_string();
        match vmm_core::disk_manage::compact_disk(&path) {
            Ok(saved) => {
                self.disk_manage.compacting = false;
                self.disk_manage.compact_result = Some(Ok(saved));
                self.set_status(format!("Disk compacted, saved {} bytes", saved));
                self.action_analyze_disk(&path);
            },
            Err(e) => {
                self.disk_manage.compacting = false;
                self.disk_manage.compact_result = Some(Err(e.to_string()));
            },
        }
    }

    pub fn action_check_disk(&mut self, disk_path: &str) {
        self.disk_manage.checking = true;
        self.disk_manage.check_result = None;
        match vmm_core::disk_manage::check_and_repair(disk_path) {
            Ok(msg) => {
                self.disk_manage.checking = false;
                self.disk_manage.check_result = Some(Ok(msg));
            },
            Err(e) => {
                self.disk_manage.checking = false;
                self.disk_manage.check_result = Some(Err(e.to_string()));
            },
        }
    }

    // ===== Parallels-inspired: Network Conditioner =====

    pub fn net_cond_state(&self) -> &NetCondState {
        &self.net_cond
    }
    pub fn net_cond_state_mut(&mut self) -> &mut NetCondState {
        &mut self.net_cond
    }

    pub fn action_open_net_cond(&mut self) {
        self.net_cond.open = true;
    }

    pub fn action_apply_network_preset(
        &mut self,
        vm_name: &str,
        preset_name: &str,
        delay_ms: u32,
        jitter_ms: u32,
        loss_pct: f32,
        bandwidth_kbps: Option<u32>,
    ) {
        let condition = vmm_core::network_conditioner::NetworkCondition {
            name: preset_name.to_string(),
            delay_ms,
            jitter_ms,
            loss_percent: loss_pct,
            bandwidth_kbps,
        };

        if let Some(ref conn) = self.conn {
            match vmm_core::network_conditioner::get_vm_tap_interface(conn.raw_conn(), vm_name) {
                Ok(Some(_iface)) => {
                    match vmm_core::network_conditioner::apply_condition(vm_name, &condition) {
                        Ok(()) => {
                            self.net_cond.active_preset = Some(preset_name.to_string());
                            self.net_cond.status_msg = Some(format!("Applied: {}", preset_name));
                            self.net_cond.is_error = false;
                            self.set_status(format!(
                                "Network condition '{}' applied to '{}'",
                                preset_name, vm_name
                            ));
                        },
                        Err(e) => {
                            self.net_cond.status_msg = Some(format!("Failed: {}", e));
                            self.net_cond.is_error = true;
                        },
                    }
                },
                Ok(None) => {
                    self.net_cond.status_msg =
                        Some("No tap interface found for this VM".to_string());
                    self.net_cond.is_error = true;
                },
                Err(e) => {
                    self.net_cond.status_msg = Some(format!("Failed to find interface: {}", e));
                    self.net_cond.is_error = true;
                },
            }
        }
    }

    pub fn action_clear_network_condition(&mut self, vm_name: &str) {
        match vmm_core::network_conditioner::clear_condition(vm_name) {
            Ok(()) => {
                self.net_cond.active_preset = Some("No Limit".to_string());
                self.net_cond.status_msg = Some("Network conditions cleared".to_string());
                self.net_cond.is_error = false;
                self.set_status(format!("Network conditions cleared for '{}'", vm_name));
            },
            Err(e) => {
                self.net_cond.status_msg = Some(format!("Failed to clear: {}", e));
                self.net_cond.is_error = true;
            },
        }
    }

    // ===== Wave 1: Guest Tools =====

    pub fn guest_tools_state(&self) -> &GuestToolsState {
        &self.guest_tools
    }
    pub fn guest_tools_state_mut(&mut self) -> &mut GuestToolsState {
        &mut self.guest_tools
    }

    pub fn action_open_guest_tools(&mut self) {
        self.guest_tools.open();
    }

    // ===== Wave 1: Media Dialog (CD/DVD) =====

    pub fn media_dialog_state(&self) -> &MediaDialogState {
        &self.media_dialog
    }
    pub fn media_dialog_state_mut(&mut self) -> &mut MediaDialogState {
        &mut self.media_dialog
    }

    pub fn action_open_media_dialog(&mut self) {
        self.media_dialog.open();
        // Detect current media
        if let Some(name) = self.selected_vm.clone() {
            self.media_dialog.current_media =
                crate::views::media_dialog::detect_current_media(&name);
        }
    }

    // ===== Wave 1: Boot to Firmware =====

    pub fn action_boot_to_firmware(&mut self) {
        if let Some(ref conn) = self.conn {
            if let Some(name) = self.selected_vm.clone() {
                match conn.boot_to_firmware(&name) {
                    Ok(()) => self.set_status(format!(
                        "Boot menu enabled for '{}'. Start the VM to enter firmware setup.",
                        name
                    )),
                    Err(e) => self.set_error(format!("Failed to set boot-to-firmware: {}", e)),
                }
            }
        }
    }

    // ===== Wave 1: Display Auto-Resize =====

    pub fn display_auto_resize(&self) -> bool {
        self.display_auto_resize
    }
    pub fn set_display_auto_resize(&mut self, enabled: bool) {
        self.display_auto_resize = enabled;
    }

    /// Called by console.rs each frame with available panel size.
    /// Debounces and sends SetDesktopSize to VNC if size changed.
    pub fn maybe_request_console_resize(&mut self, available_w: u16, available_h: u16) {
        if !self.display_auto_resize {
            return;
        }
        // Round to even dimensions (required by many video encoders/drivers)
        let w = available_w & !1;
        let h = available_h & !1;
        if w < 320 || h < 200 {
            return;
        }
        if (w, h) == self.last_requested_console_size {
            return;
        }
        let now = std::time::Instant::now();
        if now
            .duration_since(self.last_resize_request_time)
            .as_millis()
            < 300
        {
            return; // debounce: wait at least 300ms between requests
        }
        self.last_requested_console_size = (w, h);
        self.last_resize_request_time = now;
        if let Some(ref fb) = self.console_spice_fb {
            fb.request_resolution(w, h);
        } else if let Some(ref fb) = self.console_fb {
            fb.request_resolution(w, h);
        }
    }

    // ===== Wave 2: Drag-and-Drop File Transfer =====

    pub fn drop_transfer_message(&self) -> Option<String> {
        // Auto-clear after 5 seconds
        if let Some(t) = self.drop_transfer_time {
            if t.elapsed().as_secs() > 5 {
                return None;
            }
        }
        self.drop_transfer_msg.clone()
    }

    pub fn drop_transfer_is_error(&self) -> bool {
        self.drop_transfer_err
    }

    pub fn action_drop_file_to_guest(&mut self, vm_name: &str, host_path: &str) {
        let path = std::path::Path::new(host_path);
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");

        // Detect guest OS to pick target path
        let guest_path = format!("/tmp/{}", filename);

        self.drop_transfer_msg = Some(format!("Transferring {}...", filename));
        self.drop_transfer_err = false;
        self.drop_transfer_time = Some(std::time::Instant::now());

        match vmm_core::host_guest::transfer_file_to_guest(vm_name, path, &guest_path) {
            Ok(result) => {
                self.drop_transfer_msg = Some(format!(
                    "Transferred {} ({} bytes) to guest:{}",
                    filename, result.bytes_transferred, result.guest_path
                ));
                self.drop_transfer_err = false;
                self.drop_transfer_time = Some(std::time::Instant::now());
            },
            Err(e) => {
                self.drop_transfer_msg = Some(format!("Transfer failed: {}", e));
                self.drop_transfer_err = true;
                self.drop_transfer_time = Some(std::time::Instant::now());
            },
        }
    }

    // ===== Wave 2: Shared Folder Auto-Mount =====

    pub fn action_auto_mount_shared_folder(&mut self, vm_name: &str) {
        match vmm_core::host_guest::auto_mount_shared_folder(vm_name) {
            Ok(()) => self.set_status(format!("Shared folder auto-mounted for '{}'", vm_name)),
            Err(e) => self.set_error(format!("Auto-mount failed: {}", e)),
        }
    }

    // ===== Wave 2: Preferences =====

    pub fn preferences(&self) -> &vmm_core::Preferences {
        &self.preferences
    }
    pub fn preferences_mut(&mut self) -> &mut vmm_core::Preferences {
        &mut self.preferences
    }

    pub fn action_save_preferences(&mut self) {
        if let Err(e) = self.preferences.save() {
            self.set_error(format!("Failed to save preferences: {}", e));
        } else {
            self.set_status("Preferences saved");
        }
    }

    // ===== Wave 13.6: First-Run Setup Wizard =====

    pub fn first_run_state(&self) -> &FirstRunState {
        &self.first_run
    }
    pub fn first_run_state_mut(&mut self) -> &mut FirstRunState {
        &mut self.first_run
    }

    /// Manually open the first-run wizard (e.g. from Help menu).
    pub fn action_open_first_run(&mut self) {
        self.first_run = FirstRunState::default();
        self.first_run.open = true;
    }

    /// Run the system capability check and advance the wizard.
    pub fn action_first_run_run_check(&mut self) {
        let sc = vmm_core::system_check::run_system_check();
        let st = self.first_run_state_mut();
        st.system_check = sc;
        st.system_check_done = true;
        st.step = FirstRunStep::SystemCheck;
    }

    /// Kick off the discovery step (transitions to spinner state).
    pub fn action_first_run_start_discovery(&mut self) {
        let st = self.first_run_state_mut();
        st.step = FirstRunStep::Discover;
        st.discovery_done = false;
        st.discovered_vms.clear();
        st.selected_for_import.clear();
        st.discovery_in_progress = true;
    }

    /// Synchronously run the discovery scan.
    pub fn action_first_run_run_discovery(&mut self) {
        let vms = vmm_core::import::discover_importable_vms();
        let st = self.first_run_state_mut();
        st.discovered_vms = vms;
        st.discovery_in_progress = false;
        st.discovery_done = true;
    }

    /// Execute every selected import sequentially. Disk action = Symlink (safe default).
    pub fn action_first_run_run_imports(&mut self) {
        let snapshot: Vec<(usize, vmm_core::import::ImportedVm)> = {
            let st = self.first_run_state();
            let mut sel: Vec<usize> = st.selected_for_import.iter().copied().collect();
            sel.sort_unstable();
            sel.into_iter()
                .filter_map(|i| st.discovered_vms.get(i).cloned().map(|vm| (i, vm)))
                .collect()
        };

        for (_idx, vm) in snapshot {
            let name = vm.name.clone();
            // Use Symlink — no copies, fastest, safest if the user wants to back out.
            let result = match vmm_core::import::execute_import(
                &vm,
                vmm_core::import::DiskAction::Symlink,
                &name,
            ) {
                Ok(config) => {
                    // Register with libvirt so it shows up in the sidebar.
                    if let Some(ref conn) = self.conn {
                        match conn.create_vm_from_existing(&config) {
                            Ok(_) => Ok(()),
                            Err(e) => Err(format!("libvirt: {}", e)),
                        }
                    } else {
                        // No connection — still consider import succeeded (config saved on disk).
                        Ok(())
                    }
                },
                Err(e) => Err(e),
            };
            self.first_run_state_mut()
                .import_results
                .push((name, result));
        }
        self.refresh_vms();
        self.first_run_state_mut().step = FirstRunStep::Done;
    }

    /// Mark the first run as complete, persist the flag, and close the wizard.
    pub fn action_first_run_finish(&mut self) {
        self.preferences.first_run_completed = true;
        let _ = self.preferences.save();
        self.first_run.reset();
    }

    /// User clicked Skip / closed the window — same as finish but without
    /// importing anything.
    pub fn action_dismiss_first_run(&mut self) {
        self.preferences.first_run_completed = true;
        let _ = self.preferences.save();
        self.first_run.reset();
    }

    // ===== Wave 13.7-13.9: VMware / VirtualBox library scanners =====

    /// Open a folder picker, scan it for VMware `.vmx` files, and queue them
    /// into the first-run wizard import step.
    pub fn action_import_vmware_library(&mut self) {
        let initial = vmm_core::import::vmware::detect_vmware_library();
        let mut dialog = rfd::FileDialog::new();
        if let Some(p) = initial {
            dialog = dialog.set_directory(p);
        }
        if let Some(root) = dialog.pick_folder() {
            let vms = vmm_core::import::vmware::scan_vmware_library(&root);
            self.open_library_import_results(vms, "VMware");
        }
    }

    /// Open a folder picker, scan it for VirtualBox `.vbox` files, and queue them
    /// into the first-run wizard import step.
    pub fn action_import_vbox_library(&mut self) {
        let initial = vmm_core::import::virtualbox::detect_vbox_library();
        let mut dialog = rfd::FileDialog::new();
        if let Some(p) = initial {
            dialog = dialog.set_directory(p);
        }
        if let Some(root) = dialog.pick_folder() {
            let vms = vmm_core::import::virtualbox::scan_vbox_library(&root);
            self.open_library_import_results(vms, "VirtualBox");
        }
    }

    /// Reuse the first-run wizard's SelectImports step to handle a batch of
    /// discovered VMs from a library folder.
    fn open_library_import_results(
        &mut self,
        vms: Vec<vmm_core::import::ImportedVm>,
        source: &str,
    ) {
        if vms.is_empty() {
            self.set_status(format!("No {} VMs found in selected folder", source));
            return;
        }
        let count = vms.len();
        self.first_run = FirstRunState {
            open: true,
            step: FirstRunStep::SelectImports,
            discovered_vms: vms,
            discovery_done: true,
            ..Default::default()
        };
        self.set_status(format!("Found {} {} VM(s)", count, source));
    }

    // ===== Wave 3: Network Editor =====

    pub fn network_editor_state(&self) -> &views::network_editor::NetworkEditorState {
        &self.network_editor
    }
    pub fn network_editor_state_mut(&mut self) -> &mut views::network_editor::NetworkEditorState {
        &mut self.network_editor
    }

    pub fn action_open_network_editor(&mut self) {
        self.network_editor.open();
        self.action_refresh_networks();
    }

    pub fn action_refresh_networks(&mut self) {
        if let Some(ref conn) = self.conn {
            match vmm_core::network::list_networks(conn.raw_conn()) {
                Ok(nets) => self.network_editor.networks = nets,
                Err(e) => self.network_editor.error = Some(e.to_string()),
            }
        }
    }

    pub fn action_create_network(&mut self) {
        let config = self.network_editor.config.clone();
        if let Some(ref conn) = self.conn {
            match vmm_core::network_editor::create_network(conn.raw_conn(), &config) {
                Ok(()) => {
                    self.network_editor.success =
                        Some(format!("Network '{}' created", config.name));
                    self.network_editor.adding = false;
                    self.action_refresh_networks();
                },
                Err(e) => {
                    self.network_editor.error = Some(e.to_string());
                },
            }
        }
    }

    pub fn action_network_start(&mut self, name: &str) {
        if let Some(ref conn) = self.conn {
            match vmm_core::network_editor::start_network(conn.raw_conn(), name) {
                Ok(()) => self.action_refresh_networks(),
                Err(e) => self.network_editor.error = Some(e.to_string()),
            }
        }
    }

    pub fn action_network_stop(&mut self, name: &str) {
        if let Some(ref conn) = self.conn {
            match vmm_core::network_editor::stop_network(conn.raw_conn(), name) {
                Ok(()) => self.action_refresh_networks(),
                Err(e) => self.network_editor.error = Some(e.to_string()),
            }
        }
    }

    pub fn action_network_delete(&mut self, name: &str) {
        if let Some(ref conn) = self.conn {
            match vmm_core::network_editor::delete_network(conn.raw_conn(), name) {
                Ok(()) => {
                    self.network_editor.success = Some(format!("Network '{}' deleted", name));
                    self.action_refresh_networks();
                },
                Err(e) => self.network_editor.error = Some(e.to_string()),
            }
        }
    }

    // ===== Wave 3: VM Groups/Teams =====

    pub fn sidebar_collapsed_groups(&self) -> &std::collections::HashSet<String> {
        &self.sidebar_collapsed_groups
    }
    /// Cached VM configs (loaded during refresh, not per-frame).
    pub fn vm_configs(&self) -> &[VmConfig] {
        &self.vm_configs_cache
    }

    pub fn toggle_sidebar_group(&mut self, group: &str) {
        if !self.sidebar_collapsed_groups.remove(group) {
            self.sidebar_collapsed_groups.insert(group.to_string());
        }
    }

    pub fn action_batch_start(&mut self, group: &str) {
        let names: Vec<String> = self
            .vms
            .iter()
            .filter(|vm| {
                let folder = VmConfig::list_all()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|c| c.name == vm.name)
                    .and_then(|c| c.folder);
                folder.as_deref() == Some(group)
                    && matches!(vm.state, VmState::Off | VmState::Crashed)
            })
            .map(|vm| vm.name.clone())
            .collect();
        for name in &names {
            self.start_vm(name);
        }
        self.set_status(&format!(
            "Starting {} VMs in group '{}'",
            names.len(),
            group
        ));
    }

    pub fn action_batch_stop(&mut self, group: &str) {
        let names: Vec<String> = self
            .vms
            .iter()
            .filter(|vm| {
                let folder = VmConfig::list_all()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|c| c.name == vm.name)
                    .and_then(|c| c.folder);
                folder.as_deref() == Some(group)
                    && matches!(vm.state, VmState::Running | VmState::Paused)
            })
            .map(|vm| vm.name.clone())
            .collect();
        for name in &names {
            self.shutdown_vm(name);
        }
        self.set_status(&format!(
            "Shutting down {} VMs in group '{}'",
            names.len(),
            group
        ));
    }

    // ===== Parallels-inspired: Auto-Pause =====

    pub fn auto_pause_enabled(&self) -> bool {
        self.auto_pause_enabled
    }
    pub fn set_auto_pause(&mut self, enabled: bool) {
        self.auto_pause_enabled = enabled;
        self.auto_pause_was_running = false;
        if enabled {
            self.set_status("Auto-Pause enabled — VM pauses when window loses focus");
        } else {
            self.set_status("Auto-Pause disabled");
        }
    }

    // ===== Wave 4: Encryption Dialog =====

    pub fn encryption_dialog_state(&self) -> &EncryptionDialogState {
        &self.encryption_dialog
    }
    pub fn encryption_dialog_state_mut(&mut self) -> &mut EncryptionDialogState {
        &mut self.encryption_dialog
    }

    pub fn action_open_encryption_dialog(&mut self, vm_name: &str) {
        self.encryption_dialog.open_create(vm_name);
    }

    pub fn action_open_change_passphrase(&mut self, vm_name: &str) {
        self.encryption_dialog.open_change(vm_name);
    }

    pub fn action_apply_encryption(&mut self) {
        let passphrase = self.encryption_dialog.passphrase.clone();
        let vm_name = self.encryption_dialog.vm_name.clone();

        if let Some(ref mut config) = self.editing_config.clone() {
            match vmm_core::encryption::create_encrypted_qcow2(
                &config.disk_path,
                config.disk_size_gib,
                &passphrase,
            ) {
                Ok(secret_uuid) => {
                    if let Some(ref mut ec) = self.editing_config {
                        ec.disk_encrypted = true;
                        ec.encryption_secret_uuid = Some(secret_uuid);
                    }
                    self.encryption_dialog.success =
                        Some("Disk encrypted successfully".to_string());
                    self.set_status(&format!("Disk encrypted for VM '{}'", vm_name));
                },
                Err(e) => {
                    self.encryption_dialog.error = Some(format!("Encryption failed: {}", e));
                },
            }
        }
        self.clear_encryption_passphrase();
    }

    // ===== Wave 4: noVNC Browser Console =====

    pub fn novnc_panel_state(&self) -> &NoVncPanelState {
        &self.novnc_panel
    }
    pub fn novnc_panel_state_mut(&mut self) -> &mut NoVncPanelState {
        &mut self.novnc_panel
    }

    pub fn action_open_novnc_panel(&mut self) {
        if let Some(name) = self.selected_vm.clone() {
            self.novnc_panel.open_for(&name);
        }
    }

    pub fn action_start_novnc(&mut self) {
        let port: u16 = self.novnc_panel.listen_port.parse().unwrap_or(6080);
        // VNC port is typically 5900 + display number. Default to 5900.
        let vnc_port: u16 = 5900;
        let vm_name = self.novnc_panel.vm_name.clone();

        match vmm_core::novnc::start_novnc(&vm_name, vnc_port, port) {
            Ok(server) => {
                let url = server.url();
                let auto_open = self.novnc_panel.auto_open_browser;
                self.novnc_panel.server = Some(server);
                self.novnc_panel.error = None;
                self.set_status(&format!("noVNC started on port {}", port));
                if auto_open {
                    let _ = vmm_core::novnc::open_in_browser(&url);
                }
            },
            Err(e) => {
                self.novnc_panel.error = Some(e.to_string());
            },
        }
    }

    pub fn action_stop_novnc(&mut self) {
        if let Some(ref mut server) = self.novnc_panel.server {
            let _ = vmm_core::novnc::stop_novnc(server);
        }
        self.novnc_panel.server = None;
        self.set_status("noVNC server stopped");
    }

    // ===== Wave 4: Unattended Install =====

    pub fn unattended_wizard_state(&self) -> &UnattendedWizardState {
        &self.unattended_wizard
    }
    pub fn unattended_wizard_state_mut(&mut self) -> &mut UnattendedWizardState {
        &mut self.unattended_wizard
    }

    pub fn action_open_unattended_wizard(&mut self) {
        if let Some(name) = self.selected_vm.clone() {
            self.unattended_wizard.open_for(&name);
        }
    }

    pub fn action_generate_unattended_iso(&mut self, vm_name: &str) {
        let data_dir = format!(
            "{}/.local/share/libre-vmm/unattended",
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
        );
        let _ = std::fs::create_dir_all(&data_dir);

        let target = self.unattended_wizard.target.clone();
        let result = match target {
            vmm_core::unattended::UnattendedTarget::Windows => {
                let config = self.unattended_wizard.win_config.clone();
                let iso_path = format!("{}/{}-autounattend.iso", data_dir, vm_name);
                vmm_core::unattended::create_autounattend_iso(&config, &iso_path).map(|_| iso_path)
            },
            vmm_core::unattended::UnattendedTarget::LinuxCloudInit => {
                // Parse SSH keys and packages from input fields
                let ssh_keys: Vec<String> = self
                    .unattended_wizard
                    .ssh_key_input
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                let packages: Vec<String> = self
                    .unattended_wizard
                    .package_input
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();

                let mut config = self.unattended_wizard.cloud_config.clone();
                config.ssh_authorized_keys = ssh_keys;
                config.packages = packages;

                let iso_path = format!("{}/{}-cloud-init.iso", data_dir, vm_name);
                vmm_core::unattended::create_cloud_init_iso(&config, &iso_path).map(|_| iso_path)
            },
        };

        match result {
            Ok(path) => {
                self.unattended_wizard.iso_path = Some(path);
                self.unattended_wizard.error = None;
                self.unattended_wizard.step = crate::views::unattended_wizard::UnattendedStep::Done;
                self.set_status(&format!("Unattended ISO generated for '{}'", vm_name));
            },
            Err(e) => {
                self.unattended_wizard.error = Some(e.to_string());
            },
        }
    }

    // ===== Wave 4: Screen Recording =====

    pub fn screen_recording_state(&self) -> &ScreenRecordingState {
        &self.screen_recording
    }
    pub fn screen_recording_state_mut(&mut self) -> &mut ScreenRecordingState {
        &mut self.screen_recording
    }

    pub fn action_start_recording(&mut self) {
        if let Some(ref vm_name) = self.selected_vm.clone() {
            let config = self.screen_recording.config.clone();
            let ext = config.format.extension();
            let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let output_path = format!(
                "{}/{}-{}.{}",
                self.screen_recording.output_dir, vm_name, timestamp, ext
            );

            match vmm_core::screen_recording::start_recording(vm_name, &config, &output_path) {
                Ok(recording) => {
                    self.screen_recording.recording = Some(recording);
                    self.set_status(&format!("Recording started for '{}'", vm_name));
                },
                Err(e) => {
                    self.set_status(&format!("Recording failed: {}", e));
                },
            }
        }
    }

    pub fn action_stop_recording(&mut self) {
        if let Some(ref mut recording) = self.screen_recording.recording {
            match vmm_core::screen_recording::stop_recording(recording) {
                Ok(path) => {
                    self.set_status(&format!("Recording saved: {}", path));
                },
                Err(e) => {
                    self.set_status(&format!("Recording save failed: {}", e));
                },
            }
        }
        self.screen_recording.recording = None;
    }

    pub fn action_take_screenshot(&mut self) {
        if let Some(ref vm_name) = self.selected_vm.clone() {
            let dir = &self.screen_recording.output_dir;
            let _ = std::fs::create_dir_all(dir);
            let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let path = format!("{}/{}-{}.ppm", dir, vm_name, timestamp);
            match vmm_core::screen_recording::take_screenshot(vm_name, &path) {
                Ok(()) => self.set_status(&format!("Screenshot saved: {}", path)),
                Err(e) => self.set_status(&format!("Screenshot failed: {}", e)),
            }
        }
    }

    pub fn action_toggle_recording_settings(&mut self) {
        self.screen_recording.show_settings = !self.screen_recording.show_settings;
    }

    // ===== Wave 5: Memory Ballooning =====

    pub fn balloon_stats(&self) -> &Option<vmm_core::balloon::BalloonStats> {
        &self.balloon_stats
    }
    pub fn balloon_target_mib(&self) -> u64 {
        self.balloon_target_mib
    }
    pub fn set_balloon_target_mib(&mut self, v: u64) {
        self.balloon_target_mib = v;
    }

    pub fn action_refresh_balloon(&mut self, vm_name: &str) {
        match vmm_core::balloon::query_balloon_stats(vm_name) {
            Ok(stats) => {
                self.balloon_target_mib = stats.current_mib;
                self.balloon_stats = Some(stats);
            },
            Err(e) => {
                self.set_status(&format!("Balloon query failed: {}", e));
            },
        }
    }

    pub fn action_set_balloon_memory(&mut self) {
        if let Some(ref vm_name) = self.selected_vm.clone() {
            let max = self
                .balloon_stats
                .as_ref()
                .map(|s| s.maximum_mib)
                .unwrap_or(4096);
            match vmm_core::balloon::set_balloon_memory(vm_name, self.balloon_target_mib, max) {
                Ok(()) => {
                    self.set_status(&format!(
                        "Memory set to {} MiB for '{}'",
                        self.balloon_target_mib, vm_name
                    ));
                    self.action_refresh_balloon(vm_name);
                },
                Err(e) => {
                    self.set_status(&format!("Balloon set failed: {}", e));
                },
            }
        }
    }

    // ===== Wave 5: Guest File Manager =====

    pub fn guest_file_manager_state(&self) -> &GuestFileManagerState {
        &self.guest_file_manager
    }
    pub fn guest_file_manager_state_mut(&mut self) -> &mut GuestFileManagerState {
        &mut self.guest_file_manager
    }

    pub fn action_open_guest_file_manager(&mut self) {
        if let Some(name) = self.selected_vm.clone() {
            self.guest_file_manager.open_for(&name);
            self.action_guest_fm_navigate("/");
        }
    }

    pub fn action_guest_fm_navigate(&mut self, path: &str) {
        let vm_name = self.guest_file_manager.vm_name.clone();
        self.guest_file_manager.loading = true;
        self.guest_file_manager.error = None;

        match vmm_core::guest_file_manager::list_directory(&vm_name, path) {
            Ok(entries) => {
                self.guest_file_manager.entries = entries;
                self.guest_file_manager.current_path = path.to_string();
                self.guest_file_manager.path_input = path.to_string();
                self.guest_file_manager.selected_entry = None;
                self.guest_file_manager.preview_content = None;
                // Add to history
                self.guest_file_manager
                    .history
                    .truncate(self.guest_file_manager.history_idx + 1);
                self.guest_file_manager.history.push(path.to_string());
                self.guest_file_manager.history_idx = self.guest_file_manager.history.len() - 1;
            },
            Err(e) => {
                self.guest_file_manager.error = Some(e.to_string());
            },
        }
        self.guest_file_manager.loading = false;
    }

    pub fn action_guest_fm_back(&mut self) {
        if self.guest_file_manager.history_idx > 0 {
            self.guest_file_manager.history_idx -= 1;
            let path = self.guest_file_manager.history[self.guest_file_manager.history_idx].clone();
            let vm_name = self.guest_file_manager.vm_name.clone();
            match vmm_core::guest_file_manager::list_directory(&vm_name, &path) {
                Ok(entries) => {
                    self.guest_file_manager.entries = entries;
                    self.guest_file_manager.current_path = path.clone();
                    self.guest_file_manager.path_input = path;
                },
                Err(e) => self.guest_file_manager.error = Some(e.to_string()),
            }
        }
    }

    pub fn action_guest_fm_up(&mut self) {
        let current = self.guest_file_manager.current_path.clone();
        let parent = std::path::Path::new(&current)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        self.action_guest_fm_navigate(&parent);
    }

    pub fn action_guest_fm_delete(&mut self, path: &str) {
        let vm_name = self.guest_file_manager.vm_name.clone();
        match vmm_core::guest_file_manager::delete_file(&vm_name, path) {
            Ok(()) => {
                let current = self.guest_file_manager.current_path.clone();
                self.action_guest_fm_navigate(&current);
            },
            Err(e) => self.guest_file_manager.error = Some(e.to_string()),
        }
    }

    pub fn action_guest_fm_mkdir(&mut self) {
        let vm_name = self.guest_file_manager.vm_name.clone();
        let name = self.guest_file_manager.new_dir_name.clone();
        if name.is_empty() {
            return;
        }
        let path = format!(
            "{}/{}",
            self.guest_file_manager.current_path.trim_end_matches('/'),
            name
        );
        match vmm_core::guest_file_manager::create_directory(&vm_name, &path) {
            Ok(()) => {
                self.guest_file_manager.show_new_dir = false;
                self.guest_file_manager.new_dir_name.clear();
                let current = self.guest_file_manager.current_path.clone();
                self.action_guest_fm_navigate(&current);
            },
            Err(e) => self.guest_file_manager.error = Some(e.to_string()),
        }
    }

    pub fn action_guest_fm_preview(&mut self, path: &str) {
        let vm_name = self.guest_file_manager.vm_name.clone();
        match vmm_core::guest_file_manager::read_file(&vm_name, path, 1024 * 1024) {
            Ok(content) => {
                // Truncate for display
                let preview = if content.len() > 4096 {
                    format!("{}...\n[truncated]", &content[..4096])
                } else {
                    content
                };
                self.guest_file_manager.preview_content = Some(preview);
            },
            Err(e) => self.guest_file_manager.error = Some(e.to_string()),
        }
    }

    // ===== Wave 5: Multi-Display =====

    pub fn multi_display_state(&self) -> &MultiDisplayState {
        &self.multi_display
    }
    pub fn multi_display_state_mut(&mut self) -> &mut MultiDisplayState {
        &mut self.multi_display
    }
}

/// Load saved UI scale from disk.
fn load_ui_scale() -> Option<f32> {
    let path = ui_scale_path();
    let data = std::fs::read_to_string(path).ok()?;
    data.trim().parse().ok()
}

/// Save UI scale to disk.
fn save_ui_scale(scale: f32) {
    let path = ui_scale_path();
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, format!("{:.2}", scale));
}

/// Path to the UI scale settings file.
fn ui_scale_path() -> String {
    let home = dirs::home_dir().unwrap_or_default();
    format!("{}/.local/share/libre-vmm/ui_scale.conf", home.display())
}
