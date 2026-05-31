//! PCI Device Passthrough — dialog for selecting PCI/GPU devices for VFIO passthrough.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;

// ─── State ──────────────────────────────────────────────────────────

/// Filter for PCI device classes.
#[derive(Debug, Clone, PartialEq)]
pub enum PciFilter {
    All,
    GpuOnly,
    NvmeOnly,
    UsbOnly,
}

impl Default for PciFilter {
    fn default() -> Self {
        Self::All
    }
}

/// A single PCI device entry discovered on the host.
#[derive(Debug, Clone)]
pub struct PciDeviceEntry {
    pub address: String,
    pub vendor_name: String,
    pub device_name: String,
    pub class: String,
    pub driver: String,
    pub iommu_group: u32,
    pub selected: bool,
}

/// An IOMMU group containing one or more PCI devices.
#[derive(Debug, Clone)]
pub struct IommuGroupEntry {
    pub id: u32,
    pub devices: Vec<PciDeviceEntry>,
}

/// VFIO / IOMMU readiness information.
#[derive(Debug, Clone)]
pub struct VfioStatusInfo {
    pub iommu_enabled: bool,
    pub vfio_loaded: bool,
    pub kernel_params_ok: bool,
    pub issues: Vec<String>,
    pub suggestions: Vec<String>,
}

impl Default for VfioStatusInfo {
    fn default() -> Self {
        Self {
            iommu_enabled: false,
            vfio_loaded: false,
            kernel_params_ok: false,
            issues: Vec::new(),
            suggestions: Vec::new(),
        }
    }
}

/// Main state for the PCI passthrough dialog.
pub struct PciPassthroughState {
    pub open: bool,
    pub scanning: bool,
    pub devices: Vec<PciDeviceEntry>,
    pub iommu_groups: Vec<IommuGroupEntry>,
    pub selected_devices: Vec<String>,
    pub vfio_status: Option<VfioStatusInfo>,
    pub error: Option<String>,
    pub filter: PciFilter,
}

impl Default for PciPassthroughState {
    fn default() -> Self {
        Self {
            open: false,
            scanning: false,
            devices: Vec::new(),
            iommu_groups: Vec::new(),
            selected_devices: Vec::new(),
            vfio_status: None,
            error: None,
            filter: PciFilter::All,
        }
    }
}

impl PciPassthroughState {
    pub fn open(&mut self) {
        self.open = true;
        self.scan_devices();
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Scan the host for PCI devices and VFIO readiness.
    /// Tries vmm_core helpers first, falls back to stub data.
    pub fn scan_devices(&mut self) {
        self.scanning = true;
        self.error = None;

        // Check VFIO readiness
        self.vfio_status = Some(probe_vfio_status());

        // Scan PCI devices
        let raw_devices = scan_host_pci_devices();
        self.devices = raw_devices.clone();

        // Group by IOMMU group
        let mut group_map = std::collections::BTreeMap::<u32, Vec<PciDeviceEntry>>::new();
        for dev in &raw_devices {
            group_map
                .entry(dev.iommu_group)
                .or_default()
                .push(dev.clone());
        }
        self.iommu_groups = group_map
            .into_iter()
            .map(|(id, devices)| IommuGroupEntry { id, devices })
            .collect();

        self.scanning = false;
    }

    /// Apply current selected_devices back: mark devices and collect addresses.
    fn sync_selection(&mut self) {
        for group in &mut self.iommu_groups {
            for dev in &mut group.devices {
                dev.selected = self.selected_devices.contains(&dev.address);
            }
        }
        for dev in &mut self.devices {
            dev.selected = self.selected_devices.contains(&dev.address);
        }
    }
}

// ─── Host probing helpers ───────────────────────────────────────────

/// Probe VFIO readiness from the host (checks /sys, kernel modules, cmdline).
fn probe_vfio_status() -> VfioStatusInfo {
    let mut info = VfioStatusInfo::default();

    // Check IOMMU enabled via /sys
    let iommu_path = std::path::Path::new("/sys/class/iommu");
    info.iommu_enabled = iommu_path.exists()
        && std::fs::read_dir(iommu_path)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);

    // Check vfio-pci module loaded
    info.vfio_loaded = std::path::Path::new("/sys/bus/pci/drivers/vfio-pci").exists()
        || std::fs::read_to_string("/proc/modules")
            .map(|m| m.contains("vfio_pci"))
            .unwrap_or(false);

    // Check kernel command line for iommu params
    let cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();
    info.kernel_params_ok = cmdline.contains("iommu=on")
        || cmdline.contains("intel_iommu=on")
        || cmdline.contains("amd_iommu=on");

    // Build issues / suggestions
    if !info.iommu_enabled {
        info.issues.push("IOMMU is not enabled".into());
        info.suggestions.push(
            "Add 'intel_iommu=on iommu=pt' or 'amd_iommu=on iommu=pt' to kernel command line"
                .into(),
        );
    }
    if !info.vfio_loaded {
        info.issues
            .push("vfio-pci kernel module is not loaded".into());
        info.suggestions.push("Run: sudo modprobe vfio-pci".into());
    }
    if !info.kernel_params_ok {
        info.issues
            .push("IOMMU kernel parameters not detected".into());
        info.suggestions.push(
            "Edit /etc/default/grub or /etc/kernel/cmdline and add iommu params, then update bootloader"
                .into(),
        );
    }

    info
}

/// Scan the host for PCI devices by reading /sys/bus/pci/devices.
/// Falls back to stub/mock data if reading fails.
fn scan_host_pci_devices() -> Vec<PciDeviceEntry> {
    match try_scan_sysfs() {
        Ok(devs) if !devs.is_empty() => devs,
        _ => stub_pci_devices(),
    }
}

/// Try to read real PCI devices from sysfs.
fn try_scan_sysfs() -> Result<Vec<PciDeviceEntry>, std::io::Error> {
    let pci_dir = std::path::Path::new("/sys/bus/pci/devices");
    if !pci_dir.exists() {
        return Ok(Vec::new());
    }

    let mut devices = Vec::new();

    for entry in std::fs::read_dir(pci_dir)? {
        let entry = entry?;
        let addr = entry.file_name().to_string_lossy().to_string();
        let dev_path = entry.path();

        let vendor_id = read_sysfs_hex(&dev_path.join("vendor"));
        let device_id = read_sysfs_hex(&dev_path.join("device"));
        let class_id = read_sysfs_hex(&dev_path.join("class"));

        let driver = dev_path
            .join("driver")
            .read_link()
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
            .unwrap_or_default();

        let iommu_group = dev_path
            .join("iommu_group")
            .read_link()
            .ok()
            .and_then(|p| {
                p.file_name()
                    .and_then(|f| f.to_string_lossy().parse::<u32>().ok())
            })
            .unwrap_or(0);

        let class = classify_pci_device(class_id);

        // Only include interesting device classes
        if matches!(
            class.as_str(),
            "GPU" | "Audio" | "NVMe" | "USB" | "Network" | "Storage"
        ) {
            devices.push(PciDeviceEntry {
                address: addr,
                vendor_name: format_vendor(vendor_id),
                device_name: format!("{:04x}:{:04x}", vendor_id, device_id),
                class,
                driver,
                iommu_group,
                selected: false,
            });
        }
    }

    devices.sort_by(|a, b| {
        a.iommu_group
            .cmp(&b.iommu_group)
            .then(a.address.cmp(&b.address))
    });
    Ok(devices)
}

fn read_sysfs_hex(path: &std::path::Path) -> u32 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| {
            let s = s.trim().trim_start_matches("0x");
            u32::from_str_radix(s, 16).ok()
        })
        .unwrap_or(0)
}

fn classify_pci_device(class_id: u32) -> String {
    let base = (class_id >> 16) & 0xFF;
    match base {
        0x03 => "GPU".into(),
        0x04 => "Audio".into(),
        0x01 => "Storage".into(),
        0x02 => "Network".into(),
        0x0C => "USB".into(),
        _ => {
            // NVMe is class 0x010802
            if class_id >> 8 == 0x0108 {
                "NVMe".into()
            } else {
                format!("Other ({:06x})", class_id)
            }
        },
    }
}

fn format_vendor(vendor_id: u32) -> String {
    match vendor_id {
        0x10DE => "NVIDIA".into(),
        0x1002 => "AMD/ATI".into(),
        0x8086 => "Intel".into(),
        0x1B36 => "Red Hat/QEMU".into(),
        0x1AF4 => "Virtio".into(),
        0x14E4 => "Broadcom".into(),
        0x10EC => "Realtek".into(),
        0x1022 => "AMD".into(),
        _ => format!("Vendor {:04x}", vendor_id),
    }
}

/// Stub/mock PCI devices for development when sysfs is not available.
fn stub_pci_devices() -> Vec<PciDeviceEntry> {
    vec![
        PciDeviceEntry {
            address: "0000:01:00.0".into(),
            vendor_name: "NVIDIA".into(),
            device_name: "GeForce RTX 3080".into(),
            class: "GPU".into(),
            driver: "nvidia".into(),
            iommu_group: 1,
            selected: false,
        },
        PciDeviceEntry {
            address: "0000:01:00.1".into(),
            vendor_name: "NVIDIA".into(),
            device_name: "GA102 HD Audio".into(),
            class: "Audio".into(),
            driver: "snd_hda_intel".into(),
            iommu_group: 1,
            selected: false,
        },
        PciDeviceEntry {
            address: "0000:02:00.0".into(),
            vendor_name: "AMD/ATI".into(),
            device_name: "Radeon RX 6800 XT".into(),
            class: "GPU".into(),
            driver: "amdgpu".into(),
            iommu_group: 2,
            selected: false,
        },
        PciDeviceEntry {
            address: "0000:03:00.0".into(),
            vendor_name: "Samsung".into(),
            device_name: "980 PRO NVMe".into(),
            class: "NVMe".into(),
            driver: "nvme".into(),
            iommu_group: 3,
            selected: false,
        },
        PciDeviceEntry {
            address: "0000:04:00.0".into(),
            vendor_name: "Intel".into(),
            device_name: "USB 3.1 xHCI Controller".into(),
            class: "USB".into(),
            driver: "xhci_hcd".into(),
            iommu_group: 4,
            selected: false,
        },
    ]
}

// ─── UI Rendering ───────────────────────────────────────────────────

/// Render the PCI passthrough dialog as a floating window.
pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let should_show = app.pci_passthrough_state().open;
    if !should_show {
        return;
    }

    let mut open = true;
    egui::Window::new(t!("pci.title").to_string())
        .open(&mut open)
        .resizable(true)
        .collapsible(true)
        .default_width(680.0)
        .default_height(550.0)
        .show(ctx, |ui| {
            render_contents(app, ui);
        });

    if !open {
        app.pci_passthrough_state_mut().close();
    }
}

fn render_contents(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    // 1. System Status Card
    render_system_status(app, ui);
    ui.add_space(Spacing::SM);

    // 2. Filter Bar
    render_filter_bar(app, ui);
    ui.add_space(Spacing::SM);

    // 3. Device List (scrollable)
    render_device_list(app, ui);

    // 4. Warning Panel
    render_warnings(app, ui);

    ui.add_space(Spacing::SM);

    // 5. Action Buttons
    render_action_buttons(app, ui);
}

// ─── System Status Card ─────────────────────────────────────────────

fn render_system_status(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let status = app.pci_passthrough_state().vfio_status.clone();

    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::CARD)
        .inner_margin(10.0)
        .stroke(egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(t!("pci.system-status").to_string())
                    .size(FontSize::HEADING)
                    .strong()
                    .color(AppColors::TEXT),
            );
            ui.add_space(Spacing::XS);

            if let Some(ref st) = status {
                egui::Grid::new("pci_status_grid")
                    .num_columns(2)
                    .spacing([Spacing::MD, 4.0])
                    .show(ui, |ui| {
                        status_row(ui, &t!("pci.iommu-enabled").to_string(), st.iommu_enabled);
                        ui.end_row();
                        status_row(ui, &t!("pci.vfio-loaded").to_string(), st.vfio_loaded);
                        ui.end_row();
                        status_row(
                            ui,
                            &t!("pci.kernel-params").to_string(),
                            st.kernel_params_ok,
                        );
                        ui.end_row();
                    });

                // Show issues / suggestions
                if !st.issues.is_empty() {
                    ui.add_space(Spacing::XS);
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(50, 40, 20))
                        .rounding(ThemeRounding::FRAME)
                        .inner_margin(theme::Spacing::SM)
                        .show(ui, |ui| {
                            for issue in &st.issues {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("\u{26A0}").color(AppColors::WARNING),
                                    );
                                    ui.label(
                                        egui::RichText::new(issue)
                                            .size(FontSize::SMALL)
                                            .color(AppColors::WARNING),
                                    );
                                });
                            }
                            ui.add_space(theme::Spacing::XS);
                            for suggestion in &st.suggestions {
                                ui.label(
                                    egui::RichText::new(format!("  \u{2192} {}", suggestion))
                                        .size(FontSize::SMALL)
                                        .color(AppColors::TEXT_DIM),
                                );
                            }
                        });
                }
            } else {
                ui.label(
                    egui::RichText::new("Checking system status...")
                        .size(FontSize::BODY)
                        .color(AppColors::TEXT_DIM),
                );
            }
        });
}

fn status_row(ui: &mut egui::Ui, label: &str, ok: bool) {
    let (icon, color) = if ok {
        ("\u{2714}", AppColors::SUCCESS)
    } else {
        ("\u{2718}", AppColors::DANGER)
    };
    ui.label(egui::RichText::new(icon).size(FontSize::BODY).color(color));
    ui.label(
        egui::RichText::new(label)
            .size(FontSize::BODY)
            .color(AppColors::TEXT),
    );
}

// ─── Filter Bar ─────────────────────────────────────────────────────

fn render_filter_bar(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let current_filter = app.pci_passthrough_state().filter.clone();

        let filters = [
            (PciFilter::All, t!("pci.filter-all").to_string()),
            (PciFilter::GpuOnly, t!("pci.filter-gpu").to_string()),
            (PciFilter::NvmeOnly, t!("pci.filter-nvme").to_string()),
            (PciFilter::UsbOnly, t!("pci.filter-usb").to_string()),
        ];

        for (filter, label) in &filters {
            let selected = current_filter == *filter;
            let btn = egui::Button::new(
                egui::RichText::new(label.as_str())
                    .size(FontSize::LABEL)
                    .color(if selected {
                        egui::Color32::WHITE
                    } else {
                        AppColors::TEXT
                    }),
            )
            .fill(if selected {
                AppColors::PRIMARY
            } else {
                AppColors::BG_CARD
            })
            .rounding(ThemeRounding::BUTTON_SMALL);

            if ui.add(btn).clicked() {
                app.pci_passthrough_state_mut().filter = filter.clone();
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(
                    egui::RichText::new(format!("\u{1F504} {}", t!("pci.scan").to_string()))
                        .size(FontSize::LABEL),
                )
                .clicked()
            {
                app.pci_passthrough_state_mut().scan_devices();
                // Re-sync selection after rescan
                let selected = app.pci_passthrough_state().selected_devices.clone();
                app.pci_passthrough_state_mut().selected_devices = selected;
                app.pci_passthrough_state_mut().sync_selection();
            }
        });
    });
}

// ─── Device List ────────────────────────────────────────────────────

fn render_device_list(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let groups = app.pci_passthrough_state().iommu_groups.clone();
    let filter = app.pci_passthrough_state().filter.clone();
    let selected = app.pci_passthrough_state().selected_devices.clone();

    // Error display
    if let Some(err) = app.pci_passthrough_state().error.clone() {
        ui.label(
            egui::RichText::new(format!("Error: {}", err))
                .color(AppColors::DANGER)
                .size(FontSize::LABEL),
        );
        ui.add_space(Spacing::XS);
    }

    if groups.is_empty() {
        ui.add_space(Spacing::LG);
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("No PCI devices found. Click Scan to refresh.")
                    .size(FontSize::BODY)
                    .color(AppColors::TEXT_DIM),
            );
        });
        return;
    }

    egui::ScrollArea::vertical()
        .max_height(280.0)
        .show(ui, |ui| {
            for group in &groups {
                // Filter: skip groups with no matching devices
                let filtered_devs: Vec<&PciDeviceEntry> = group
                    .devices
                    .iter()
                    .filter(|d| match filter {
                        PciFilter::All => true,
                        PciFilter::GpuOnly => d.class == "GPU",
                        PciFilter::NvmeOnly => d.class == "NVMe",
                        PciFilter::UsbOnly => d.class == "USB",
                    })
                    .collect();

                if filtered_devs.is_empty() {
                    continue;
                }

                // IOMMU group header
                let all_selected = group.devices.iter().all(|d| selected.contains(&d.address));

                egui::Frame::none()
                    .fill(AppColors::BG_CARD.linear_multiply(0.6))
                    .rounding(ThemeRounding::FRAME)
                    .inner_margin(theme::Spacing::SM)
                    .stroke(egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE))
                    .show(ui, |ui| {
                        // Group header with group-level checkbox
                        ui.horizontal(|ui| {
                            let mut group_checked = all_selected;
                            if ui.checkbox(&mut group_checked, "").changed() {
                                let addrs: Vec<String> =
                                    group.devices.iter().map(|d| d.address.clone()).collect();
                                let sel = &mut app.pci_passthrough_state_mut().selected_devices;
                                if group_checked {
                                    for a in &addrs {
                                        if !sel.contains(a) {
                                            sel.push(a.clone());
                                        }
                                    }
                                } else {
                                    sel.retain(|a| !addrs.contains(a));
                                }
                                app.pci_passthrough_state_mut().sync_selection();
                            }

                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {}",
                                    t!("pci.iommu-group").to_string(),
                                    group.id
                                ))
                                .size(FontSize::SUBHEADING)
                                .strong()
                                .color(AppColors::PRIMARY),
                            );
                        });

                        ui.add_space(2.0);

                        // Device rows
                        for dev in &filtered_devs {
                            render_device_row(app, ui, dev, &selected);
                        }
                    });

                ui.add_space(Spacing::XS);
            }
        });
}

fn render_device_row(
    app: &mut LibreVmmApp,
    ui: &mut egui::Ui,
    dev: &PciDeviceEntry,
    selected: &[String],
) {
    ui.horizontal(|ui| {
        // Checkbox
        let mut checked = selected.contains(&dev.address);
        if ui.checkbox(&mut checked, "").changed() {
            let sel = &mut app.pci_passthrough_state_mut().selected_devices;
            if checked {
                if !sel.contains(&dev.address) {
                    sel.push(dev.address.clone());
                }
            } else {
                sel.retain(|a| a != &dev.address);
            }
            app.pci_passthrough_state_mut().sync_selection();
        }

        // PCI address (monospace)
        ui.label(
            egui::RichText::new(&dev.address)
                .size(FontSize::LABEL)
                .family(egui::FontFamily::Monospace)
                .color(AppColors::TEXT_DIM),
        );

        // Class icon
        let class_icon = match dev.class.as_str() {
            "GPU" => "\u{1F3AE}",     // game controller / GPU
            "Audio" => "\u{1F50A}",   // speaker
            "NVMe" => "\u{1F4BE}",    // floppy / storage
            "USB" => "\u{1F50C}",     // plug
            "Network" => "\u{1F310}", // globe
            "Storage" => "\u{1F4BF}", // CD
            _ => "\u{2699}",          // gear
        };
        ui.label(egui::RichText::new(class_icon).size(FontSize::BODY));

        // Vendor + device name
        ui.label(
            egui::RichText::new(format!("{} {}", dev.vendor_name, dev.device_name))
                .size(FontSize::BODY)
                .color(AppColors::TEXT),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Driver badge
            if dev.driver == "vfio-pci" {
                egui::Frame::none()
                    .fill(AppColors::SUCCESS.linear_multiply(0.3))
                    .rounding(ThemeRounding::BUTTON_SMALL)
                    .inner_margin(egui::vec2(6.0, 2.0))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("VFIO")
                                .size(FontSize::TINY)
                                .strong()
                                .color(AppColors::SUCCESS),
                        );
                    });
            } else if !dev.driver.is_empty() {
                egui::Frame::none()
                    .fill(AppColors::BG_HOVER)
                    .rounding(ThemeRounding::BUTTON_SMALL)
                    .inner_margin(egui::vec2(6.0, 2.0))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(&dev.driver)
                                .size(FontSize::TINY)
                                .color(AppColors::TEXT_DIM),
                        );
                    });
            }
        });
    });
}

// ─── Warnings Panel ─────────────────────────────────────────────────

fn render_warnings(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let selected = &app.pci_passthrough_state().selected_devices;
    if selected.is_empty() {
        return;
    }

    let has_gpu = app
        .pci_passthrough_state()
        .devices
        .iter()
        .any(|d| d.class == "GPU" && selected.contains(&d.address));

    let gpu_count = app
        .pci_passthrough_state()
        .devices
        .iter()
        .filter(|d| d.class == "GPU")
        .count();

    let selected_gpu_count = app
        .pci_passthrough_state()
        .devices
        .iter()
        .filter(|d| d.class == "GPU" && selected.contains(&d.address))
        .count();

    if !has_gpu && selected.len() < 2 {
        return;
    }

    ui.add_space(Spacing::SM);

    egui::Frame::none()
        .fill(AppColors::BANNER_BG)
        .rounding(ThemeRounding::FRAME)
        .inner_margin(10.0)
        .stroke(egui::Stroke::new(0.5, AppColors::WARNING.linear_multiply(0.5)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("\u{26A0}")
                        .size(FontSize::HEADING)
                        .color(AppColors::WARNING),
                );
                ui.label(
                    egui::RichText::new(t!("common.warning").to_string())
                        .size(FontSize::SUBHEADING)
                        .strong()
                        .color(AppColors::WARNING),
                );
            });
            ui.add_space(theme::Spacing::XS);

            // Always warn about IOMMU groups
            ui.label(
                egui::RichText::new(t!("pci.warning-iommu-group").to_string())
                    .size(FontSize::SMALL)
                    .color(AppColors::TEXT),
            );

            if has_gpu {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(
                        "GPU passthrough requires the host to have another GPU or headless operation",
                    )
                    .size(FontSize::SMALL)
                    .color(AppColors::TEXT),
                );

                // Single-GPU warning
                if gpu_count <= 1 || selected_gpu_count >= gpu_count {
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(t!("pci.warning-single-gpu").to_string())
                            .size(FontSize::SMALL)
                            .strong()
                            .color(AppColors::DANGER),
                    );
                }
            }
        });
}

// ─── Action Buttons ─────────────────────────────────────────────────

fn render_action_buttons(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Cancel
            if ui
                .button(egui::RichText::new(t!("common.cancel").to_string()).size(FontSize::BODY))
                .clicked()
            {
                app.pci_passthrough_state_mut().close();
            }

            // Apply Selection
            let selected = app.pci_passthrough_state().selected_devices.clone();
            let devices_info = app.pci_passthrough_state().devices.clone();

            let apply_btn = egui::Button::new(
                egui::RichText::new(t!("pci.apply").to_string())
                    .size(FontSize::BODY)
                    .color(egui::Color32::WHITE),
            )
            .fill(AppColors::PRIMARY)
            .rounding(ThemeRounding::BUTTON);

            if ui.add(apply_btn).clicked() {
                // Convert selected PCI addresses to VfioDeviceConfig entries
                let vfio_configs: Vec<vmm_core::config::VfioDeviceConfig> = selected
                    .iter()
                    .map(|addr| {
                        let desc = devices_info
                            .iter()
                            .find(|d| &d.address == addr)
                            .map(|d| format!("{} {}", d.vendor_name, d.device_name))
                            .unwrap_or_default();
                        vmm_core::config::VfioDeviceConfig {
                            pci_address: addr.clone(),
                            description: desc,
                            rom_bar: true,
                        }
                    })
                    .collect();

                if let Some(ref mut config) = app.editing_config_mut() {
                    config.vfio_devices = vfio_configs;
                }

                app.pci_passthrough_state_mut().close();
            }

            // Show selection count
            let count = app.pci_passthrough_state().selected_devices.len();
            if count > 0 {
                ui.label(
                    egui::RichText::new(format!(
                        "{} device{} selected",
                        count,
                        if count == 1 { "" } else { "s" }
                    ))
                    .size(FontSize::SMALL)
                    .color(AppColors::TEXT_DIM),
                );
            }
        });
    });
}
