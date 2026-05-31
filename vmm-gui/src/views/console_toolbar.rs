//! Console toolbar — minimal controls shown above the console in Console-First mode.
//! Shows state dot + VM name + compact power controls + Ctrl+Alt+Del + Disconnect + "Manage" button.

use crate::app::{LibreVmmApp, ManageTab, ViewMode};
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;
use vmm_core::domain::VmState;

pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let Some(vm_name) = app.selected_vm().map(|s| s.to_string()) else {
        return;
    };
    let state = app.selected_vm_state().unwrap_or(VmState::Off);
    let has_console = app.console_framebuffer().is_some();

    egui::Frame::none()
        .fill(AppColors::BG_PANEL)
        .inner_margin(egui::Margin::symmetric(Spacing::SM, Spacing::XS))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = Spacing::SM;

                // State dot
                let state_color = match state {
                    VmState::Running => AppColors::RUNNING,
                    VmState::Paused => AppColors::PAUSED,
                    VmState::Crashed => AppColors::CRASHED,
                    _ => AppColors::OFF,
                };
                let (dot_rect, _) =
                    ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(dot_rect.center(), 4.0, state_color);

                // VM name
                ui.label(
                    egui::RichText::new(&vm_name)
                        .size(FontSize::BODY)
                        .strong()
                        .color(AppColors::TEXT),
                );

                ui.separator();

                // Compact power controls (only when running)
                if state == VmState::Running {
                    if small_action_btn(ui, &t!("console.shutdown").to_string())
                        .on_hover_text(t!("tooltip.shutdown-vm").to_string())
                        .clicked()
                    {
                        app.action_shutdown(&vm_name);
                    }
                    if small_action_btn(ui, &t!("console.pause").to_string())
                        .on_hover_text(t!("tooltip.pause-vm").to_string())
                        .clicked()
                    {
                        app.action_pause(&vm_name);
                    }
                    if small_action_btn(ui, &t!("console.suspend").to_string())
                        .on_hover_text(t!("console.suspend-tooltip").to_string())
                        .clicked()
                    {
                        app.action_suspend_to_disk(&vm_name);
                    }
                }

                // Console controls
                if has_console {
                    ui.separator();
                    if small_action_btn(ui, &t!("console.ctrl-alt-del").to_string())
                        .on_hover_text(t!("tooltip.send-cad").to_string())
                        .clicked()
                    {
                        app.send_ctrl_alt_del();
                    }
                    if small_action_btn(ui, &t!("console.cd-dvd").to_string())
                        .on_hover_text(t!("console.cd-dvd-tooltip").to_string())
                        .clicked()
                    {
                        app.action_open_media_dialog();
                    }
                    // Looking Glass launch button — only when LG is enabled in config
                    // and the VM is running.
                    if app.selected_vm_config_has_looking_glass() && app.is_selected_vm_running() {
                        if small_action_btn(ui, &t!("console.looking-glass").to_string())
                            .on_hover_text(t!("console.looking-glass-tooltip").to_string())
                            .clicked()
                        {
                            app.action_launch_looking_glass();
                        }
                    }
                    // Screen recording controls
                    ui.separator();
                    crate::views::screen_recording::render_toolbar_controls(app, ui);

                    if small_action_btn(ui, &t!("console.disconnect").to_string())
                        .on_hover_text(t!("tooltip.disconnect-console").to_string())
                        .clicked()
                    {
                        app.disconnect_console();
                        app.set_view_mode(ViewMode::Manage(ManageTab::Summary));
                    }
                }

                // Right-aligned: Manage button + hint
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let manage_btn = egui::Button::new(
                        egui::RichText::new(t!("console.manage-arrow").to_string())
                            .size(FontSize::LABEL)
                            .color(AppColors::TEXT),
                    )
                    .fill(AppColors::BG_HOVER)
                    .rounding(ThemeRounding::BUTTON_SMALL);
                    if ui
                        .add(manage_btn)
                        .on_hover_text(t!("console.manage-tooltip").to_string())
                        .clicked()
                    {
                        app.set_view_mode(ViewMode::Manage(ManageTab::Summary));
                    }

                    // Input hint
                    ui.label(
                        egui::RichText::new(t!("console.click-capture").to_string())
                            .size(FontSize::TINY)
                            .color(AppColors::MUTED),
                    );
                });
            });
        });
}

fn small_action_btn(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .size(FontSize::SMALL)
                .color(AppColors::TEXT),
        )
        .fill(AppColors::BG_HOVER)
        .rounding(ThemeRounding::BUTTON_SMALL),
    )
}
