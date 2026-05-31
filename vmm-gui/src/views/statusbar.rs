//! Bottom status bar — shows connection status and latest event.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::AppColors;
use eframe::egui;
use rust_i18n::t;

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("statusbar")
        .exact_height(28.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                // Connection indicator
                let (dot_color, label) = if app.is_connected() {
                    (AppColors::RUNNING, t!("status.connected").to_string())
                } else {
                    (AppColors::DANGER, t!("status.disconnected").to_string())
                };

                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, dot_color);
                ui.label(
                    egui::RichText::new(label)
                        .size(theme::FontSize::SMALL)
                        .color(AppColors::TEXT_DIM),
                );

                ui.separator();

                // VM count
                let running = app
                    .vms()
                    .iter()
                    .filter(|v| v.state == vmm_core::domain::VmState::Running)
                    .count();
                let total = app.vms().len();
                ui.label(
                    egui::RichText::new(
                        t!("status.vm-count", total = total, running = running).to_string(),
                    )
                    .size(theme::FontSize::SMALL)
                    .color(AppColors::TEXT_DIM),
                );

                // Background tasks indicator
                ui.separator();
                crate::views::task_panel::render_status_indicator(app, ui);

                // Event log toggle button
                ui.separator();
                let log_btn_label = if app.show_event_log() {
                    t!("status.hide-log").to_string()
                } else {
                    t!("status.log").to_string()
                };
                if ui.small_button(log_btn_label).clicked() {
                    app.toggle_event_log();
                }

                // Latest event (right-aligned)
                if let Some(event) = app.latest_event() {
                    if event.timestamp.elapsed() < std::time::Duration::from_secs(10) {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let color = if event.is_error {
                                AppColors::DANGER
                            } else {
                                AppColors::SUCCESS
                            };
                            ui.label(
                                egui::RichText::new(&event.text)
                                    .size(theme::FontSize::SMALL)
                                    .color(color),
                            );
                        });
                    }
                }
            });
        });
}
