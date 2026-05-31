//! noVNC browser console panel — start/stop websockify, open in browser.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::{AppColors, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;

/// State for the noVNC panel dialog.
pub struct NoVncPanelState {
    pub open: bool,
    pub listen_port: String,
    pub auto_open_browser: bool,
    pub server: Option<vmm_core::novnc::NoVncServer>,
    pub error: Option<String>,
    pub vm_name: String,
}

impl Default for NoVncPanelState {
    fn default() -> Self {
        Self {
            open: false,
            listen_port: "6080".to_string(),
            auto_open_browser: true,
            server: None,
            error: None,
            vm_name: String::new(),
        }
    }
}

impl NoVncPanelState {
    pub fn open_for(&mut self, vm_name: &str) {
        self.open = true;
        self.vm_name = vm_name.to_string();
        self.error = None;
    }

    pub fn is_running(&self) -> bool {
        self.server
            .as_ref()
            .map(|s| s.status == vmm_core::novnc::NoVncStatus::Running)
            .unwrap_or(false)
    }
}

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let state = app.novnc_panel_state();
    if !state.open {
        return;
    }

    let mut open = true;
    let mut do_start = false;
    let mut do_stop = false;
    let mut do_open_browser = false;
    let mut do_copy_url = false;

    egui::Window::new(t!("novnc.title"))
        .open(&mut open)
        .resizable(false)
        .default_width(380.0)
        .collapsible(false)
        .show(ctx, |ui| {
            let state = app.novnc_panel_state();
            let vm_name = state.vm_name.clone();
            let is_running = state.is_running();

            ui.label(
                egui::RichText::new(t!("novnc.vm-label", name = vm_name))
                    .strong()
                    .color(AppColors::TEXT),
            );
            ui.add_space(Spacing::SM);

            // Status
            ui.horizontal(|ui| {
                ui.label(t!("novnc.status"));
                if is_running {
                    ui.label(
                        egui::RichText::new(t!("novnc.status-running")).color(AppColors::RUNNING),
                    );
                } else {
                    ui.label(egui::RichText::new(t!("novnc.status-stopped")).color(AppColors::OFF));
                }
            });

            ui.add_space(Spacing::SM);

            // Port configuration (only when not running)
            ui.horizontal(|ui| {
                ui.label(t!("novnc.port"));
                let port = &mut app.novnc_panel_state_mut().listen_port;
                ui.add_enabled(
                    !is_running,
                    egui::TextEdit::singleline(port)
                        .desired_width(80.0)
                        .hint_text("6080"),
                );
            });

            // Auto-open browser toggle
            let mut auto_open = app.novnc_panel_state().auto_open_browser;
            if ui
                .checkbox(&mut auto_open, t!("novnc.open-browser-auto"))
                .changed()
            {
                app.novnc_panel_state_mut().auto_open_browser = auto_open;
            }

            ui.add_space(Spacing::SM);

            // URL display (when running)
            let state = app.novnc_panel_state();
            if let Some(ref server) = state.server {
                if server.status == vmm_core::novnc::NoVncStatus::Running {
                    let url = server.url();
                    egui::Frame::none()
                        .fill(AppColors::BG_CARD)
                        .rounding(ThemeRounding::BUTTON_SMALL)
                        .inner_margin(Spacing::SM)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&url)
                                    .size(12.0)
                                    .color(AppColors::PRIMARY)
                                    .monospace(),
                            );
                        });
                    ui.add_space(Spacing::XS);
                }
            }

            // Error
            if let Some(err) = app.novnc_panel_state().error.clone() {
                ui.label(
                    egui::RichText::new(&err)
                        .color(AppColors::DANGER)
                        .size(theme::FontSize::SMALL),
                );
                ui.add_space(Spacing::XS);
            }

            ui.separator();
            ui.add_space(Spacing::XS);

            // Buttons
            ui.horizontal(|ui| {
                if !is_running {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(t!("novnc.start-server"))
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(AppColors::PRIMARY),
                        )
                        .clicked()
                    {
                        do_start = true;
                    }
                } else {
                    if ui.button(t!("novnc.stop-server")).clicked() {
                        do_stop = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(t!("novnc.open-in-browser"))
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(AppColors::PRIMARY),
                        )
                        .clicked()
                    {
                        do_open_browser = true;
                    }
                    if ui.button(t!("novnc.copy-url")).clicked() {
                        do_copy_url = true;
                    }
                }
            });

            ui.add_space(Spacing::SM);

            // Tool availability check
            if !vmm_core::novnc::websockify_available() {
                ui.label(
                    egui::RichText::new(t!("novnc.websockify-missing"))
                        .size(theme::FontSize::SMALL)
                        .color(AppColors::DANGER),
                );
            }
        });

    if !open {
        app.novnc_panel_state_mut().open = false;
    }

    if do_start {
        app.action_start_novnc();
    }
    if do_stop {
        app.action_stop_novnc();
    }
    if do_open_browser {
        if let Some(ref server) = app.novnc_panel_state().server {
            let url = server.url();
            let _ = vmm_core::novnc::open_in_browser(&url);
        }
    }
    if do_copy_url {
        if let Some(ref server) = app.novnc_panel_state().server {
            let url = server.url();
            ctx.copy_text(url);
        }
    }
}
