//! Rollback Mode panel — auto-snapshot before sessions with easy revert.
//! Renders as a section in the Manage view or as a floating dialog.

use crate::app::LibreVmmApp;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;
use vmm_core::domain::VmState;

/// Rollback UI state.
pub struct RollbackState {
    pub open: bool,
    pub rollback_points: Vec<RollbackPoint>,
    pub confirm_revert: Option<String>,
    #[allow(dead_code)]
    pub last_refresh: std::time::Instant,
}

#[derive(Clone, Debug)]
pub struct RollbackPoint {
    pub name: String,
    pub timestamp: String,
    pub description: String,
}

impl Default for RollbackState {
    fn default() -> Self {
        Self {
            open: false,
            rollback_points: Vec::new(),
            confirm_revert: None,
            last_refresh: std::time::Instant::now(),
        }
    }
}

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    if !app.rollback_state().open {
        return;
    }

    let Some(vm_name) = app.selected_vm().map(|s| s.to_string()) else {
        app.rollback_state_mut().open = false;
        return;
    };

    let vm_state = app.selected_vm_state().unwrap_or(VmState::Off);
    let is_running = matches!(vm_state, VmState::Running | VmState::Paused);

    let mut open = true;
    egui::Window::new(t!("rollback.title"))
        .id(egui::Id::new("rollback_dialog"))
        .default_size([450.0, 400.0])
        .resizable(true)
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            // Header
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new(format!("Rollback: {}", vm_name))
                        .size(FontSize::HEADING)
                        .color(AppColors::TEXT),
                );
            });

            ui.add_space(Spacing::XS);

            // Rollback config
            egui::Frame::none()
                .fill(AppColors::BG_CARD)
                .rounding(ThemeRounding::BUTTON)
                .inner_margin(Spacing::MD)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(t!("rollback.config").to_string())
                            .size(FontSize::BODY)
                            .strong()
                            .color(AppColors::TEXT),
                    );
                    ui.add_space(Spacing::XS);

                    if let Some(ref mut config) = app.editing_config_mut() {
                        ui.checkbox(
                            &mut config.rollback_enabled,
                            t!("rollback.enable").to_string(),
                        );
                        ui.add_space(Spacing::XS);

                        // SECURITY: CWE-681 — Clamp usize to slider range before i32 cast.
                        let mut max = (config.rollback_max_points.min(20)) as i32;
                        ui.horizontal(|ui| {
                            ui.label(t!("rollback.max-points"));
                            ui.add(
                                egui::Slider::new(&mut max, 1..=20)
                                    .text(t!("rollback.snapshots").to_string()),
                            );
                        });
                        config.rollback_max_points = max.clamp(1, 20) as usize;
                    } else if let Some(ref config) = app.selected_vm_config() {
                        let enabled = config.rollback_enabled;
                        let label_text = if enabled {
                            format!("\u{2714} {}", t!("rollback.enabled"))
                        } else {
                            format!("\u{2718} {}", t!("rollback.disabled"))
                        };
                        ui.label(egui::RichText::new(label_text).size(FontSize::LABEL).color(
                            if enabled {
                                AppColors::SUCCESS
                            } else {
                                AppColors::TEXT_DIM
                            },
                        ));
                        ui.label(
                            egui::RichText::new(
                                t!("rollback.max-label", count = config.rollback_max_points)
                                    .to_string(),
                            )
                            .size(FontSize::SMALL)
                            .color(AppColors::TEXT_DIM),
                        );
                    }
                });

            ui.add_space(Spacing::SM);

            // Actions
            ui.horizontal(|ui| {
                // Manual rollback point
                if ui
                    .add_enabled(
                        !is_running,
                        egui::Button::new(t!("rollback.create-point").to_string()),
                    )
                    .on_hover_text(if is_running {
                        t!("rollback.vm-must-stop").to_string()
                    } else {
                        t!("rollback.take-now").to_string()
                    })
                    .clicked()
                {
                    app.action_create_rollback_point(&vm_name);
                }

                // Refresh list
                if ui.button(t!("rollback.refresh")).clicked() {
                    app.action_refresh_rollback_points(&vm_name);
                }
            });

            ui.add_space(Spacing::SM);

            // Rollback points list
            ui.label(
                egui::RichText::new(t!("rollback.points-title").to_string())
                    .size(FontSize::SUBHEADING)
                    .strong()
                    .color(AppColors::TEXT),
            );
            ui.add_space(Spacing::XS);

            let points = app.rollback_state().rollback_points.clone();
            if points.is_empty() {
                ui.label(
                    egui::RichText::new(t!("rollback.no-points").to_string())
                        .size(FontSize::LABEL)
                        .color(AppColors::TEXT_DIM),
                );
            } else {
                egui::ScrollArea::vertical()
                    .max_height(250.0)
                    .show(ui, |ui| {
                        for (i, point) in points.iter().enumerate() {
                            egui::Frame::none()
                                .fill(if i == 0 {
                                    AppColors::BG_HOVER
                                } else {
                                    AppColors::BG_CARD
                                })
                                .rounding(ThemeRounding::BUTTON_SMALL)
                                .inner_margin(Spacing::SM)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        if i == 0 {
                                            ui.label(
                                                egui::RichText::new(
                                                    t!("rollback.latest").to_string(),
                                                )
                                                .size(FontSize::TINY)
                                                .strong()
                                                .color(AppColors::SUCCESS),
                                            );
                                        }
                                        ui.label(
                                            egui::RichText::new(&point.name)
                                                .size(FontSize::LABEL)
                                                .strong()
                                                .color(AppColors::TEXT),
                                        );
                                        ui.label(
                                            egui::RichText::new(&point.timestamp)
                                                .size(FontSize::SMALL)
                                                .color(AppColors::TEXT_DIM),
                                        );

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                let confirm =
                                                    app.rollback_state().confirm_revert.as_deref()
                                                        == Some(&point.name);

                                                if confirm {
                                                    if ui
                                                        .button(t!("rollback.yes-revert"))
                                                        .on_hover_text(
                                                            t!("rollback.discard-warning")
                                                                .to_string(),
                                                        )
                                                        .clicked()
                                                    {
                                                        let snap_name = point.name.clone();
                                                        app.rollback_state_mut().confirm_revert =
                                                            None;
                                                        app.action_revert_rollback(
                                                            &vm_name, &snap_name,
                                                        );
                                                    }
                                                    if ui.button(t!("common.no")).clicked() {
                                                        app.rollback_state_mut().confirm_revert =
                                                            None;
                                                    }
                                                } else {
                                                    if ui
                                                        .add_enabled(
                                                            !is_running,
                                                            egui::Button::new(
                                                                t!("rollback.revert").to_string(),
                                                            ),
                                                        )
                                                        .clicked()
                                                    {
                                                        app.rollback_state_mut().confirm_revert =
                                                            Some(point.name.clone());
                                                    }
                                                }
                                            },
                                        );
                                    });
                                    if !point.description.is_empty() {
                                        ui.label(
                                            egui::RichText::new(&point.description)
                                                .size(FontSize::SMALL)
                                                .color(AppColors::TEXT_DIM),
                                        );
                                    }
                                });
                            ui.add_space(Spacing::XS / 2.0);
                        }
                    });
            }
        });

    if !open {
        app.rollback_state_mut().open = false;
    }
}
