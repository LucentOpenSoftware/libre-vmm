//! Settings / About view — two-column card dashboard layout.

use crate::app::{LibreVmmApp, Screen};
use crate::i18n::{apply_language, load_language, save_language, Language};
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;
// Wave 16.A1: VmConfig itself is a pure type from vmm-types; the I/O methods
// (default_vm_dir, config_dir, ...) ride on the vmm-core VmConfigIo trait.
use vmm_core::config::VmConfigIo;
use vmm_types::VmConfig;

/// Inner margin for settings cards.
const CARD_MARGIN: f32 = 10.0;
/// Gap between card rows.
const ROW_GAP: f32 = 8.0;

pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.button(t!("appsettings.back")).clicked() {
            app.set_screen(Screen::Home);
        }
        ui.heading(t!("appsettings.title"));
    });

    ui.add_space(Spacing::SM);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Two-column layout using columns()
            // Left column gets: System Info, Display, Default HW
            // Right column gets: Language, Notifications, About
            ui.columns(2, |cols| {
                // ── Left column ──
                card_system_info(app, &mut cols[0]);
                cols[0].add_space(ROW_GAP);
                card_display(app, &mut cols[0]);
                cols[0].add_space(ROW_GAP);
                card_default_hw(app, &mut cols[0]);

                // ── Right column ──
                card_language(&mut cols[1]);
                cols[1].add_space(ROW_GAP);
                card_notifications(app, &mut cols[1]);
                cols[1].add_space(ROW_GAP);
                card_about(&mut cols[1]);
            });

            ui.add_space(Spacing::SM);
        });
}

// ─── Card: System Info ───────────────────────────────────────────────

fn card_system_info(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    settings_card(ui, &t!("appsettings.sysinfo"), |ui| {
        egui::Grid::new("settings_info")
            .num_columns(2)
            .spacing([Spacing::SM, 3.0])
            .show(ui, |ui| {
                let conn_val = if app.is_connected() {
                    t!("status.connected")
                } else {
                    t!("status.disconnected")
                };
                setting_row(ui, &t!("appsettings.connection"), &conn_val);

                let config_dir = VmConfig::config_dir();
                setting_row(ui, &t!("appsettings.config-dir"), &config_dir);

                let disk_dir = VmConfig::default_vm_dir();
                setting_row(ui, &t!("appsettings.disk-dir"), &disk_dir);

                let kvm_val = if std::path::Path::new("/dev/kvm").exists() {
                    t!("appsettings.kvm-yes")
                } else {
                    t!("appsettings.kvm-no")
                };
                setting_row(ui, &t!("appsettings.kvm"), &kvm_val);
            });
    });
}

// ─── Card: Language ──────────────────────────────────────────────────

fn card_language(ui: &mut egui::Ui) {
    settings_card(ui, &t!("appsettings.language"), |ui| {
        let current = load_language();
        ui.horizontal_wrapped(|ui| {
            for lang in Language::all() {
                let label = match lang {
                    Language::Auto => t!("appsettings.lang-auto"),
                    Language::English => t!("appsettings.lang-en"),
                    Language::Spanish => t!("appsettings.lang-es"),
                };
                let selected = *lang == current;
                let btn =
                    egui::Button::new(egui::RichText::new(&*label).size(FontSize::LABEL).color(
                        if selected {
                            egui::Color32::WHITE
                        } else {
                            AppColors::TEXT
                        },
                    ))
                    .fill(if selected {
                        AppColors::PRIMARY
                    } else {
                        AppColors::BG_HOVER
                    })
                    .rounding(ThemeRounding::BUTTON_SMALL);
                if ui.add(btn).clicked() && !selected {
                    save_language(*lang);
                    apply_language(*lang);
                }
            }
        });
    });
}

// ─── Card: Display / UI Scale ────────────────────────────────────────

fn card_display(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    settings_card(ui, &t!("appsettings.display"), |ui| {
        let current_scale = app.ui_scale();
        let current_ppp = ui.ctx().pixels_per_point();
        let native_ppp = current_ppp / ui.ctx().zoom_factor();

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t!("appsettings.ui-scale")).size(FontSize::LABEL));
            let label = if current_scale <= 0.0 {
                format!(
                    "{} ({:.0}%)",
                    t!("common.auto"),
                    current_ppp * 100.0 / native_ppp.max(1.0)
                )
            } else {
                format!("{:.0}%", current_scale * 100.0)
            };
            ui.label(
                egui::RichText::new(&label)
                    .color(AppColors::TEXT)
                    .size(FontSize::LABEL)
                    .strong(),
            );
        });

        ui.horizontal_wrapped(|ui| {
            if ui.small_button(t!("common.auto")).clicked() {
                app.set_ui_scale(0.0);
                ui.ctx().set_zoom_factor(1.0);
            }
            for (label_text, factor) in [
                ("75%", 0.75),
                ("100%", 1.0),
                ("125%", 1.25),
                ("150%", 1.5),
                ("175%", 1.75),
                ("200%", 2.0),
            ] {
                if ui.small_button(label_text).clicked() {
                    app.set_ui_scale(factor);
                    ui.ctx().set_zoom_factor(factor);
                }
            }
        });

        let mut slider_val = if current_scale <= 0.0 {
            ui.ctx().zoom_factor()
        } else {
            current_scale
        };
        let old_val = slider_val;
        ui.add(
            egui::Slider::new(&mut slider_val, 0.5..=3.0)
                .text(t!("appsettings.zoom"))
                .step_by(0.05)
                .show_value(true),
        );
        if (slider_val - old_val).abs() > 0.001 {
            app.set_ui_scale(slider_val);
            ui.ctx().set_zoom_factor(slider_val);
        }

        ui.label(
            egui::RichText::new(t!(
                "appsettings.dpi-info",
                native = format!("{:.2}", native_ppp),
                effective = format!("{:.0}", ui.ctx().zoom_factor() * 100.0),
                ppp = format!("{:.1}", current_ppp),
            ))
            .color(AppColors::TEXT_DIM)
            .size(FontSize::CAPTION),
        );
    });
}

// ─── Card: Notifications ─────────────────────────────────────────────

fn card_notifications(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    settings_card(ui, &t!("appsettings.notifications"), |ui| {
        let available = vmm_core::notifications::notifications_available();
        if !available {
            ui.label(
                egui::RichText::new(t!("appsettings.notify-missing"))
                    .color(AppColors::WARNING)
                    .size(FontSize::SMALL),
            );
        }

        let settings = app.notification_settings_mut();
        ui.checkbox(
            &mut settings.vm_power_events,
            t!("appsettings.notify-power"),
        );
        ui.checkbox(
            &mut settings.snapshot_events,
            t!("appsettings.notify-snapshot"),
        );
        ui.checkbox(&mut settings.task_events, t!("appsettings.notify-task"));
        ui.checkbox(&mut settings.error_events, t!("appsettings.notify-error"));
    });
}

// ─── Card: Default Hardware ──────────────────────────────────────────

fn card_default_hw(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    settings_card(ui, &t!("appsettings.default-hw"), |ui| {
        ui.label(
            egui::RichText::new(t!("appsettings.default-hw-desc"))
                .size(FontSize::CAPTION)
                .color(AppColors::TEXT_DIM),
        );

        egui::Grid::new("prefs_grid")
            .num_columns(2)
            .spacing([Spacing::SM, 3.0])
            .show(ui, |ui| {
                ui.label(t!("appsettings.default-cpus"));
                let mut cpus = app.preferences().default_cpus as i32;
                if ui
                    .add(egui::DragValue::new(&mut cpus).range(1..=64).speed(1))
                    .changed()
                {
                    app.preferences_mut().default_cpus = cpus.max(1) as u32;
                }
                ui.end_row();

                ui.label(t!("appsettings.default-ram"));
                let mut ram = app.preferences().default_memory_mib as i64;
                if ui
                    .add(
                        egui::DragValue::new(&mut ram)
                            .range(256..=131072)
                            .speed(256),
                    )
                    .changed()
                {
                    app.preferences_mut().default_memory_mib = ram.max(256) as u64;
                }
                ui.end_row();

                ui.label(t!("appsettings.default-disk"));
                let mut disk = app.preferences().default_disk_gib as i64;
                if ui
                    .add(egui::DragValue::new(&mut disk).range(1..=2048).speed(5))
                    .changed()
                {
                    app.preferences_mut().default_disk_gib = disk.max(1) as u64;
                }
                ui.end_row();

                ui.label(t!("appsettings.default-uefi"));
                let mut uefi = app.preferences().default_uefi;
                if ui.checkbox(&mut uefi, "").changed() {
                    app.preferences_mut().default_uefi = uefi;
                }
                ui.end_row();
            });

        ui.add_space(2.0);

        let mut auto_suspend = app.preferences().auto_suspend_on_shutdown;
        if ui
            .checkbox(&mut auto_suspend, t!("appsettings.auto-suspend"))
            .changed()
        {
            app.preferences_mut().auto_suspend_on_shutdown = auto_suspend;
        }

        let mut auto_mount = app.preferences().shared_folder_auto_mount;
        if ui
            .checkbox(&mut auto_mount, t!("appsettings.auto-mount"))
            .changed()
        {
            app.preferences_mut().shared_folder_auto_mount = auto_mount;
        }

        ui.add_space(2.0);
        if ui.small_button(t!("appsettings.save-prefs")).clicked() {
            app.action_save_preferences();
        }
    });
}

// ─── Card: About ─────────────────────────────────────────────────────

fn card_about(ui: &mut egui::Ui) {
    settings_card(ui, &t!("appsettings.about"), |ui| {
        ui.label(egui::RichText::new(t!("appsettings.about-desc")).size(FontSize::LABEL));
        ui.label(
            egui::RichText::new(t!("appsettings.about-tech"))
                .color(AppColors::TEXT_DIM)
                .size(FontSize::SMALL),
        );
        ui.label(
            egui::RichText::new(t!(
                "appsettings.version",
                version = env!("CARGO_PKG_VERSION")
            ))
            .color(AppColors::TEXT_DIM)
            .size(FontSize::SMALL),
        );
    });
}

// ─── Helpers ─────────────────────────────────────────────────────────

/// Compact settings card with title and body.
fn settings_card(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::FRAME)
        .inner_margin(CARD_MARGIN)
        .outer_margin(egui::Margin {
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 4.0,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .size(FontSize::SUBHEADING)
                    .strong()
                    .color(AppColors::TEXT),
            );
            ui.add_space(2.0);
            body(ui);
        });
}

fn setting_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(
        egui::RichText::new(label)
            .color(AppColors::TEXT_DIM)
            .size(FontSize::LABEL),
    );
    ui.label(
        egui::RichText::new(value)
            .color(AppColors::TEXT)
            .size(FontSize::LABEL),
    );
    ui.end_row();
}
