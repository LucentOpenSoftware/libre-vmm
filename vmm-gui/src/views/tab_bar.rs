//! VM tab bar — shows open VM tabs above the main content area.
//! Clicking a tab selects that VM. The × button closes a tab.

use crate::app::{LibreVmmApp, Screen};
use crate::theme;
use crate::theme::{AppColors, FontSize};
use eframe::egui;
use rust_i18n::t;
use vmm_core::domain::VmState;

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let tabs = app.open_vm_tabs().to_vec();
    if tabs.is_empty() {
        return;
    }

    egui::TopBottomPanel::top("vm_tab_bar")
        .exact_height(30.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(theme::Spacing::XS);

                let selected = app.selected_vm().map(|s| s.to_string());
                let mut tab_to_close: Option<String> = None;
                let mut tab_to_select: Option<String> = None;

                for tab_name in &tabs {
                    let is_selected = selected.as_deref() == Some(tab_name.as_str());

                    // Look up VM state for the dot indicator
                    let vm_state = app
                        .vms()
                        .iter()
                        .find(|v| v.name == *tab_name)
                        .map(|v| v.state.clone());

                    let state_dot = match vm_state {
                        Some(VmState::Running) => ("\u{25CF} ", AppColors::RUNNING),
                        Some(VmState::Paused) => ("\u{25CF} ", AppColors::PAUSED),
                        Some(VmState::Suspended) => ("\u{25CF} ", AppColors::WARNING),
                        _ => ("", AppColors::TEXT_DIM),
                    };

                    // Tab frame
                    let fill = if is_selected {
                        AppColors::BG_CARD
                    } else {
                        AppColors::BG_DARK
                    };
                    let stroke_color = if is_selected {
                        AppColors::PRIMARY
                    } else {
                        AppColors::STROKE_SUBTLE
                    };

                    egui::Frame::none()
                        .fill(fill)
                        .rounding(egui::Rounding {
                            nw: 4.0,
                            ne: 4.0,
                            sw: 0.0,
                            se: 0.0,
                        })
                        .stroke(egui::Stroke::new(
                            if is_selected { 1.5 } else { 0.5 },
                            stroke_color,
                        ))
                        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;

                                // State dot
                                if !state_dot.0.is_empty() {
                                    ui.label(
                                        egui::RichText::new(state_dot.0)
                                            .size(9.0)
                                            .color(state_dot.1),
                                    );
                                }

                                // Tab name — clickable
                                let label_color = if is_selected {
                                    AppColors::TEXT
                                } else {
                                    AppColors::TEXT_DIM
                                };
                                let tab_label = ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(tab_name)
                                            .size(FontSize::SMALL)
                                            .color(label_color),
                                    )
                                    .sense(egui::Sense::click()),
                                );
                                if tab_label.clicked() {
                                    tab_to_select = Some(tab_name.clone());
                                }

                                // Close button
                                let close_btn = ui
                                    .add(
                                        egui::Label::new(
                                            egui::RichText::new(" \u{2715}")
                                                .size(FontSize::TINY)
                                                .color(AppColors::TEXT_DIM),
                                        )
                                        .sense(egui::Sense::click()),
                                    )
                                    .on_hover_text(t!("tooltip.close-tab").to_string());
                                if close_btn.clicked() {
                                    tab_to_close = Some(tab_name.clone());
                                }
                                if close_btn.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                            });
                        });

                    ui.add_space(1.0);
                }

                // Process actions after iteration
                if let Some(name) = tab_to_select {
                    app.set_selected_vm(Some(name.clone()));
                    // If we're in VmSettings for a different VM, switch
                    if let Screen::VmSettings(_) = app.screen().clone() {
                        if let Some(config) = app.selected_vm_config().cloned() {
                            app.set_editing_config(Some(config));
                            app.set_screen(Screen::VmSettings(name));
                        }
                    }
                }

                if let Some(name) = tab_to_close {
                    app.close_vm_tab(&name);
                    // If the closed tab was selected and we're in VmSettings, go home
                    if app.selected_vm().map(|s| s == name).unwrap_or(false) {
                        if matches!(app.screen(), Screen::VmSettings(_)) {
                            app.set_editing_config(None);
                            app.set_screen(Screen::Home);
                        }
                    }
                }
            });
        });
}
