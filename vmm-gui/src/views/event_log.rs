//! Event log panel — toggle-able bottom panel showing timestamped events.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::AppColors;
use eframe::egui;
use rust_i18n::t;

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("event_log")
        .resizable(true)
        .default_height(140.0)
        .min_height(80.0)
        .max_height(300.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(t!("log.title").to_string())
                        .size(12.0)
                        .strong()
                        .color(AppColors::TEXT_DIM),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(t!("log.clear").to_string())
                        .on_hover_text(t!("tooltip.clear-log").to_string())
                        .clicked()
                    {
                        app.clear_event_log();
                    }
                    if ui
                        .small_button(t!("log.close").to_string())
                        .on_hover_text(t!("tooltip.close-log").to_string())
                        .clicked()
                    {
                        app.toggle_event_log();
                    }
                });
            });

            ui.separator();

            let events: Vec<_> = app.event_log().iter().cloned().collect();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for entry in events
                        .iter()
                        .rev()
                        .take(100)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                    {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&entry.time_str)
                                    .size(theme::FontSize::SMALL)
                                    .color(AppColors::MUTED)
                                    .monospace(),
                            );

                            let (icon, color) = if entry.is_error {
                                ("\u{2717}", AppColors::DANGER)
                            } else {
                                ("\u{2713}", AppColors::SUCCESS)
                            };
                            ui.label(
                                egui::RichText::new(icon)
                                    .size(theme::FontSize::SMALL)
                                    .color(color),
                            );
                            ui.label(
                                egui::RichText::new(&entry.text)
                                    .size(theme::FontSize::SMALL)
                                    .color(if entry.is_error {
                                        AppColors::DANGER
                                    } else {
                                        AppColors::TEXT
                                    }),
                            );
                        });
                    }
                });
        });
}
