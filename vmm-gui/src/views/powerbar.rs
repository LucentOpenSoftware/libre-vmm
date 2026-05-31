//! Power bar — VMware-style quick power controls.
//! Now renders inside the main content panel (not a top-level panel).

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::AppColors;
use eframe::egui;
use rust_i18n::t;
use vmm_core::domain::VmState;

pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let Some(vm_name) = app.selected_vm().map(|s| s.to_string()) else {
        return;
    };
    let Some(state) = app.selected_vm_state() else {
        return;
    };

    egui::Frame::none()
        .fill(AppColors::BG_PANEL)
        .rounding(theme::ThemeRounding::BUTTON)
        .inner_margin(theme::Spacing::SM)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                // VM name + state
                let state_color = match state {
                    VmState::Running => AppColors::RUNNING,
                    VmState::Paused => AppColors::PAUSED,
                    VmState::Crashed => AppColors::CRASHED,
                    _ => AppColors::OFF,
                };

                let (dot_rect, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(dot_rect.center(), 5.0, state_color);

                ui.label(
                    egui::RichText::new(&vm_name)
                        .size(14.0)
                        .strong()
                        .color(AppColors::TEXT),
                );
                let state_label = if state == VmState::Off && app.has_managed_save(&vm_name) {
                    t!("state.off-saved").to_string()
                } else {
                    format!("({})", state)
                };
                ui.label(
                    egui::RichText::new(&state_label)
                        .size(theme::FontSize::BODY)
                        .color(state_color),
                );

                ui.separator();

                // ===== POWER BUTTONS =====
                let has_save = app.has_managed_save(&vm_name);

                // Pre-format button labels once per frame instead of per-button
                // (avoids ~7 format!() allocations on every frame for the Running state)
                match state {
                    VmState::Off | VmState::Crashed => {
                        if has_save {
                            if power_btn_icon(
                                ui,
                                "\u{23F5}",
                                &t!("power.resume-saved").to_string(),
                                AppColors::SUCCESS,
                                &t!("power.tooltip.resume-saved").to_string(),
                            ) {
                                app.action_start(&vm_name);
                            }
                            if power_btn_icon(
                                ui,
                                "\u{2716}",
                                &t!("power.discard-save").to_string(),
                                AppColors::BG_HOVER,
                                &t!("power.tooltip.discard-save").to_string(),
                            ) {
                                app.action_discard_save(&vm_name);
                            }
                        } else {
                            if power_btn_icon(
                                ui,
                                "\u{23F5}",
                                &t!("power.start").to_string(),
                                AppColors::SUCCESS,
                                &t!("power.tooltip.start").to_string(),
                            ) {
                                app.action_start(&vm_name);
                            }
                        }
                    },
                    VmState::Running => {
                        if power_btn_icon(
                            ui,
                            "\u{23F9}",
                            &t!("power.power-off").to_string(),
                            AppColors::DANGER,
                            &t!("power.tooltip.power-off").to_string(),
                        ) {
                            app.request_confirm_force_stop(vm_name.clone());
                        }
                        if power_btn_icon(
                            ui,
                            "\u{23CF}",
                            &t!("power.shutdown").to_string(),
                            AppColors::BG_HOVER,
                            &t!("power.tooltip.shutdown").to_string(),
                        ) {
                            app.action_shutdown(&vm_name);
                        }
                        if power_btn_icon(
                            ui,
                            "\u{23EF}",
                            &t!("power.pause").to_string(),
                            AppColors::BG_HOVER,
                            &t!("power.tooltip.pause").to_string(),
                        ) {
                            app.action_pause(&vm_name);
                        }
                        if power_btn_icon(
                            ui,
                            "\u{1F4BE}",
                            &t!("power.suspend").to_string(),
                            AppColors::BG_HOVER,
                            &t!("power.tooltip.suspend").to_string(),
                        ) {
                            app.action_suspend_to_disk(&vm_name);
                        }
                        if power_btn_icon(
                            ui,
                            "\u{21BB}",
                            &t!("power.reboot").to_string(),
                            AppColors::BG_HOVER,
                            &t!("power.tooltip.reboot").to_string(),
                        ) {
                            app.action_reboot(&vm_name);
                        }

                        ui.separator();

                        if power_btn_icon(
                            ui,
                            "\u{1F5B5}",
                            &t!("power.console").to_string(),
                            AppColors::PRIMARY,
                            &t!("power.tooltip.console").to_string(),
                        ) {
                            app.action_console(&vm_name);
                        }
                    },
                    VmState::Paused | VmState::Suspended => {
                        if power_btn_icon(
                            ui,
                            "\u{23F5}",
                            &t!("power.resume").to_string(),
                            AppColors::SUCCESS,
                            &t!("power.tooltip.resume").to_string(),
                        ) {
                            app.action_resume(&vm_name);
                        }
                        if power_btn_icon(
                            ui,
                            "\u{23F9}",
                            &t!("power.power-off").to_string(),
                            AppColors::DANGER,
                            &t!("power.tooltip.power-off").to_string(),
                        ) {
                            app.request_confirm_force_stop(vm_name.clone());
                        }
                    },
                    _ => {},
                }

                // Right side — bulk operations
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(t!("power.power-off-all").to_string())
                                    .size(theme::FontSize::SMALL)
                                    .color(AppColors::DANGER),
                            )
                            .rounding(theme::ThemeRounding::BUTTON_SMALL),
                        )
                        .on_hover_text(t!("power.tooltip.power-off-all").to_string())
                        .clicked()
                    {
                        app.action_power_off_all();
                    }

                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(t!("power.shutdown-all").to_string())
                                    .size(theme::FontSize::SMALL),
                            )
                            .rounding(theme::ThemeRounding::BUTTON_SMALL),
                        )
                        .on_hover_text(t!("power.tooltip.shutdown-all").to_string())
                        .clicked()
                    {
                        app.action_shutdown_all();
                    }
                });
            });
        });
}

/// Power button using icon + label as a single pre-formatted string.
/// Uses a small reusable buffer to avoid per-call format!() heap allocation.
fn power_btn_icon(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    color: egui::Color32,
    tooltip: &str,
) -> bool {
    // Thread-local buffer avoids heap allocation for the concatenated label
    thread_local! {
        static BUF: std::cell::RefCell<String> = std::cell::RefCell::new(String::with_capacity(32));
    }
    let combined = BUF.with(|buf| {
        let mut buf = buf.borrow_mut();
        buf.clear();
        buf.push_str(icon);
        buf.push(' ');
        buf.push_str(label);
        buf.clone()
    });

    let btn = egui::Button::new(
        egui::RichText::new(combined)
            .size(theme::FontSize::BODY)
            .color(egui::Color32::WHITE),
    )
    .fill(color)
    .min_size(egui::vec2(0.0, 28.0))
    .rounding(5.0);

    ui.add(btn).on_hover_text(tooltip).clicked()
}
