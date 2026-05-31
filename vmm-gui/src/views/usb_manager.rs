//! USB Device Manager — list and attach/detach USB devices to running VMs.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::AppColors;
use eframe::egui;
use vmm_core::usb::UsbDevice;

/// State for the USB device manager.
pub struct UsbManagerState {
    pub visible: bool,
    pub devices: Vec<UsbDevice>,
    pub last_scan: std::time::Instant,
    pub error: Option<String>,
}

impl Default for UsbManagerState {
    fn default() -> Self {
        Self {
            visible: false,
            devices: Vec::new(),
            last_scan: std::time::Instant::now() - std::time::Duration::from_secs(60),
            error: None,
        }
    }
}

impl UsbManagerState {
    pub fn open(&mut self) {
        self.visible = true;
        self.refresh();
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    pub fn refresh(&mut self) {
        self.devices = vmm_core::usb::list_host_usb_devices();
        self.last_scan = std::time::Instant::now();
        self.error = None;
    }
}

/// Render the USB manager as a floating window.
pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let should_show = app.usb_manager_state().visible;
    if !should_show {
        return;
    }

    // Check Esc to close the dialog (CWE-1216 — UX accessibility)
    if theme::escape_pressed(ctx) {
        app.usb_manager_state_mut().close();
        return;
    }

    let mut open = true;
    egui::Window::new("USB Device Manager")
        .open(&mut open)
        .resizable(true)
        .collapsible(true)
        .default_width(500.0)
        .default_height(350.0)
        .show(ctx, |ui| {
            let vm_name = app.selected_vm().map(|s| s.to_string());
            let vm_is_running = app
                .selected_vm_state()
                .map(|s| s == vmm_core::domain::VmState::Running)
                .unwrap_or(false);

            // Header
            ui.horizontal(|ui| {
                if let Some(ref name) = vm_name {
                    ui.label(
                        egui::RichText::new(format!("Target VM: {}", name))
                            .size(theme::FontSize::BODY)
                            .color(AppColors::PRIMARY),
                    );
                    if !vm_is_running {
                        ui.label(
                            egui::RichText::new("(VM must be running)")
                                .size(theme::FontSize::SMALL)
                                .color(AppColors::DANGER),
                        );
                    }
                } else {
                    ui.label(egui::RichText::new("No VM selected").color(AppColors::TEXT_DIM));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("\u{1F504} Refresh").clicked() {
                        app.usb_manager_state_mut().refresh();
                    }
                });
            });

            ui.add_space(theme::Spacing::XS);
            ui.separator();
            ui.add_space(theme::Spacing::XS);

            // Error
            if let Some(error) = app.usb_manager_state().error.clone() {
                ui.label(
                    egui::RichText::new(format!("Error: {}", error))
                        .color(AppColors::DANGER)
                        .size(12.0),
                );
                ui.add_space(theme::Spacing::XS);
            }

            // Device list
            let devices = app.usb_manager_state().devices.clone();

            if devices.is_empty() {
                ui.add_space(20.0);
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("No USB devices found on the host.")
                            .size(theme::FontSize::BODY)
                            .color(AppColors::TEXT_DIM),
                    );
                });
            } else {
                ui.label(
                    egui::RichText::new(format!(
                        "{} USB device{} found",
                        devices.len(),
                        if devices.len() == 1 { "" } else { "s" }
                    ))
                    .size(theme::FontSize::SMALL)
                    .color(AppColors::TEXT_DIM),
                );
                ui.add_space(theme::Spacing::XS);

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for device in &devices {
                        egui::Frame::none()
                            .fill(AppColors::BG_CARD)
                            .rounding(theme::ThemeRounding::BUTTON)
                            .inner_margin(10.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("\u{1F50C}")
                                            .size(theme::FontSize::HEADING),
                                    );
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(device.display_label())
                                                .size(12.0)
                                                .strong()
                                                .color(AppColors::TEXT),
                                        );
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "Bus {} Device {} | {}:{}",
                                                device.bus,
                                                device.device,
                                                device.vendor_id,
                                                device.product_id
                                            ))
                                            .size(10.0)
                                            .color(AppColors::TEXT_DIM),
                                        );
                                    });

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let can_interact = vm_is_running && vm_name.is_some();

                                            if device.attached {
                                                let detach_btn = egui::Button::new(
                                                    egui::RichText::new("Detach")
                                                        .size(theme::FontSize::SMALL)
                                                        .color(egui::Color32::WHITE),
                                                )
                                                .fill(AppColors::DANGER)
                                                .rounding(theme::ThemeRounding::BUTTON_SMALL);
                                                if ui
                                                    .add_enabled(can_interact, detach_btn)
                                                    .clicked()
                                                {
                                                    app.action_detach_usb(
                                                        &device.vendor_id,
                                                        &device.product_id,
                                                    );
                                                }
                                            } else {
                                                let attach_btn = egui::Button::new(
                                                    egui::RichText::new("Attach")
                                                        .size(theme::FontSize::SMALL)
                                                        .color(egui::Color32::WHITE),
                                                )
                                                .fill(AppColors::PRIMARY)
                                                .rounding(theme::ThemeRounding::BUTTON_SMALL);
                                                if ui
                                                    .add_enabled(can_interact, attach_btn)
                                                    .clicked()
                                                {
                                                    app.action_attach_usb(
                                                        &device.vendor_id,
                                                        &device.product_id,
                                                    );
                                                }
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(3.0);
                    }
                });
            }
        });

    if !open {
        app.usb_manager_state_mut().close();
    }
}
