//! Management view — tabbed admin panel (Summary / Snapshots / Performance)
//! with a "◀ Console" button to return to the console.
//! Replaces tab_bar.rs in the Console-First UX architecture.

use crate::app::{LibreVmmApp, ManageTab, Screen, ViewMode};
use crate::theme;
use crate::theme::AppColors;
use crate::views;
use eframe::egui;
use rust_i18n::t;
use vmm_core::domain::VmState;

pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let active = match app.view_mode() {
        ViewMode::Manage(tab) => tab.clone(),
        _ => ManageTab::Summary,
    };
    let is_running = matches!(app.selected_vm_state(), Some(VmState::Running));

    // Tab bar row
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;

        // ◀ Console button (only if VM is running or console is connected)
        if is_running || app.console_framebuffer().is_some() {
            let console_btn = egui::Button::new(
                egui::RichText::new("\u{25C0} Console")
                    .size(theme::FontSize::BODY)
                    .color(AppColors::PRIMARY),
            )
            .fill(egui::Color32::TRANSPARENT)
            .rounding(egui::Rounding {
                nw: 6.0,
                ne: 6.0,
                sw: 0.0,
                se: 0.0,
            })
            .min_size(egui::vec2(100.0, 28.0));

            if ui
                .add(console_btn)
                .on_hover_text("Ctrl+Enter \u{2014} Return to console")
                .clicked()
            {
                if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                    app.action_console(&name);
                }
            }

            // Visual separator between console button and tabs
            ui.add_space(theme::Spacing::XS);
            ui.separator();
            ui.add_space(theme::Spacing::XS);
        }

        // Management tabs
        ui.spacing_mut().item_spacing.x = 0.0;

        if tab_button(ui, "Summary", active == ManageTab::Summary) {
            app.set_view_mode(ViewMode::Manage(ManageTab::Summary));
        }
        if tab_button(ui, "Snapshots", active == ManageTab::Snapshots) {
            app.set_view_mode(ViewMode::Manage(ManageTab::Snapshots));
        }
        if tab_button(ui, "Performance", active == ManageTab::Performance) {
            app.set_view_mode(ViewMode::Manage(ManageTab::Performance));
        }

        // Right-aligned contextual actions
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match active {
                ManageTab::Summary => {
                    let is_off = matches!(
                        app.selected_vm_state(),
                        Some(VmState::Off) | Some(VmState::Crashed)
                    );
                    if is_off {
                        if ui
                            .small_button("Edit Settings")
                            .on_hover_text(t!("tooltip.edit-vm-settings").to_string())
                            .clicked()
                        {
                            if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                                if let Some(config) = app.selected_vm_config().cloned() {
                                    app.set_editing_config(Some(config));
                                    app.set_screen(Screen::VmSettings(name));
                                }
                            }
                        }
                    }
                    // Guest agent info button (only when running)
                    if is_running {
                        if ui
                            .small_button("Guest Info")
                            .on_hover_text(t!("tooltip.refresh-guest-info").to_string())
                            .clicked()
                        {
                            app.refresh_guest_info();
                        }
                    }
                },
                ManageTab::Snapshots => {
                    if ui
                        .small_button("Refresh")
                        .on_hover_text(t!("tooltip.refresh-snapshots").to_string())
                        .clicked()
                    {
                        app.refresh_snapshots();
                    }
                },
                ManageTab::Performance => {
                    if !is_running {
                        ui.label(
                            egui::RichText::new("VM must be running for live stats")
                                .size(10.0)
                                .color(AppColors::MUTED),
                        );
                    }
                },
            }
        });
    });

    ui.separator();

    // Tab content — scrollable so long content doesn't clip
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match active {
            ManageTab::Summary => {
                ui.add_space(theme::Spacing::XS);
                if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                    views::summary::render(app, ui, &name);
                }
            },
            ManageTab::Snapshots => {
                ui.add_space(theme::Spacing::XS);
                views::snapshots::render(app, ui);
            },
            ManageTab::Performance => {
                ui.add_space(theme::Spacing::XS);
                views::monitor::render(app, ui);
            },
        });
}

fn tab_button(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    let fill = if active {
        AppColors::BG_CARD
    } else {
        egui::Color32::TRANSPARENT
    };
    let text_color = if active {
        AppColors::TEXT
    } else {
        AppColors::TEXT_DIM
    };

    let btn = egui::Button::new(
        egui::RichText::new(label)
            .size(theme::FontSize::BODY)
            .color(text_color),
    )
    .fill(fill)
    .rounding(egui::Rounding {
        nw: 6.0,
        ne: 6.0,
        sw: 0.0,
        se: 0.0,
    })
    .min_size(egui::vec2(90.0, 28.0))
    .stroke(if active {
        egui::Stroke::new(1.0, AppColors::PRIMARY)
    } else {
        egui::Stroke::NONE
    });

    ui.add(btn).clicked()
}
