//! VM Settings editor — two-column card layout (mirrors app settings style).
//! Metadata fields (description, tags, favorite, etc.) are always editable.
//! Hardware fields (CPU, RAM, boot, network, etc.) require the VM to be off.

use crate::app::{LibreVmmApp, Screen};
use crate::theme;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;
use vmm_core::config::{
    BootDevice, GpuModel, NetworkMode, NicConfig, OsType, ParallelPortConfig, SerialBackend,
    SerialPortConfig, UsbControllerVersion,
};
use vmm_core::domain::VmState;

/// Inner margin for settings cards.
const CARD_MARGIN: f32 = 16.0;
/// Gap between card rows (unused now — outer_margin handles it).
const ROW_GAP: f32 = 6.0;
/// Vertical spacing between rows inside a settings grid.
const GRID_ROW_GAP: f32 = 10.0;
/// Max width for text input fields inside grid cells.
/// (Grid cells report available_width as infinity, so we must cap it.)
const TEXT_FIELD_WIDTH: f32 = 320.0;
const TEXT_FIELD_NARROW: f32 = 240.0;

pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui, name: &str) {
    let name = name.to_string();

    // Determine if VM is running (hardware changes blocked)
    let vm_state = app.selected_vm_state().unwrap_or(VmState::Off);
    let is_running = matches!(vm_state, VmState::Running | VmState::Paused);

    // ── Top bar (title + buttons) — outside scroll area ──
    ui.horizontal(|ui| {
        ui.heading(
            egui::RichText::new(t!("vmsettings.title", name = name).to_string())
                .color(AppColors::TEXT)
                .strong(),
        );

        // State badge
        if is_running {
            let badge_color = if vm_state == VmState::Running {
                AppColors::RUNNING
            } else {
                AppColors::PAUSED
            };
            ui.label(
                egui::RichText::new(format!("  \u{25cf} {}", vm_state))
                    .size(12.0)
                    .color(badge_color),
            );
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Cancel button
            if ui.button(t!("vmsettings.cancel").to_string()).clicked() {
                app.set_editing_config(None);
                app.set_screen(Screen::Home);
            }

            if is_running {
                // Metadata-only save (always available)
                let save_meta_btn = egui::Button::new(
                    egui::RichText::new(t!("vmsettings.save-metadata").to_string())
                        .color(egui::Color32::WHITE),
                )
                .fill(AppColors::PRIMARY)
                .rounding(theme::ThemeRounding::BUTTON);

                if ui
                    .add(save_meta_btn)
                    .on_hover_text(t!("vmsettings.save-metadata-tooltip").to_string())
                    .clicked()
                {
                    if let Some(config) = app.editing_config().cloned() {
                        app.action_save_metadata(&config);
                    }
                }
            } else {
                // Full save (hardware + metadata)
                let save_btn = egui::Button::new(
                    egui::RichText::new(t!("vmsettings.save-all").to_string())
                        .color(egui::Color32::WHITE),
                )
                .fill(AppColors::SUCCESS)
                .rounding(theme::ThemeRounding::BUTTON);

                if ui
                    .add(save_btn)
                    .on_hover_text(t!("vmsettings.save-all-tooltip").to_string())
                    .clicked()
                {
                    if let Some(config) = app.editing_config().cloned() {
                        app.action_update_vm(&config);
                    }
                }
            }
        });
    });

    ui.add_space(theme::Spacing::XS);

    // ── Running VM banner — outside columns ──
    if is_running {
        egui::Frame::none()
            .fill(AppColors::BANNER_BG)
            .rounding(theme::ThemeRounding::BUTTON)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("\u{26a0}")
                            .size(theme::FontSize::HEADING)
                            .color(AppColors::WARNING),
                    );
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(t!("vmsettings.running-warning").to_string())
                                .size(theme::FontSize::BODY)
                                .color(AppColors::WARNING)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new(t!("vmsettings.running-detail").to_string())
                                .size(theme::FontSize::SMALL)
                                .color(AppColors::TEXT_DIM),
                        );
                    });
                });
            });
    }

    ui.add_space(Spacing::SM);
    ui.separator();
    ui.add_space(Spacing::MD);

    // Check we have a config to edit
    let has_config = app.editing_config().is_some();
    if !has_config {
        ui.label(
            egui::RichText::new(t!("vmsettings.no-config").to_string()).color(AppColors::DANGER),
        );
        if ui.button(t!("vmsettings.back").to_string()).clicked() {
            app.set_screen(Screen::Home);
        }
        return;
    }

    // ── Main content: two manually-positioned columns ──
    let col_gap = 10.0;
    let col_w = (ui.available_width() - col_gap) / 2.0;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let origin = ui.cursor().min;

            // LEFT COLUMN — positioned at exact coordinates
            let left_rect = egui::Rect::from_min_size(origin, egui::vec2(col_w, 20000.0));
            let mut left_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(left_rect)
                    .layout(egui::Layout::top_down(egui::Align::LEFT)),
            );
            left_ui.set_clip_rect(left_ui.clip_rect().intersect(left_rect));
            card_metadata(app, &mut left_ui, is_running);
            card_hardware(app, &mut left_ui, is_running);
            card_boot(app, &mut left_ui, is_running);
            card_network(app, &mut left_ui, is_running);
            let left_h = left_ui.min_size().y;

            // RIGHT COLUMN — starts at origin.x + col_w + gap, never pushed by left
            let right_origin = egui::pos2(origin.x + col_w + col_gap, origin.y);
            let right_rect = egui::Rect::from_min_size(right_origin, egui::vec2(col_w, 20000.0));
            let mut right_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(right_rect)
                    .layout(egui::Layout::top_down(egui::Align::LEFT)),
            );
            right_ui.set_clip_rect(right_ui.clip_rect().intersect(right_rect));
            card_display_gpu(app, &mut right_ui, is_running);
            card_autoprotect(app, &mut right_ui);
            card_organization(app, &mut right_ui);
            card_devices(app, &mut right_ui, is_running);
            card_serial_ports(app, &mut right_ui, is_running);
            card_parallel_ports(app, &mut right_ui, is_running);
            card_passthrough(app, &mut right_ui, is_running);
            card_resource_limits(app, &mut right_ui, is_running);
            let right_h = right_ui.min_size().y;

            // Tell parent ScrollArea how tall the content is
            ui.allocate_space(egui::vec2(ui.available_width(), left_h.max(right_h)));
        });
}

// ─── Card helper ─────────────────────────────────────────────────────

fn settings_card(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::FRAME)
        .stroke(egui::Stroke::new(1.0, AppColors::STROKE_SUBTLE))
        .inner_margin(CARD_MARGIN)
        .outer_margin(egui::Margin {
            left: 4.0,
            right: 4.0,
            top: 0.0,
            bottom: 10.0,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .size(FontSize::SUBHEADING)
                    .strong()
                    .color(AppColors::TEXT),
            );
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            body(ui);
        });
}

/// Card variant for hardware sections that dim when VM is running.
fn settings_card_hw(
    ui: &mut egui::Ui,
    title: &str,
    is_running: bool,
    body: impl FnOnce(&mut egui::Ui),
) {
    let fill = if is_running {
        AppColors::BG_CARD.linear_multiply(0.6)
    } else {
        AppColors::BG_CARD
    };
    egui::Frame::none()
        .fill(fill)
        .rounding(ThemeRounding::FRAME)
        .stroke(egui::Stroke::new(1.0, AppColors::STROKE_SUBTLE))
        .inner_margin(CARD_MARGIN)
        .outer_margin(egui::Margin {
            left: 4.0,
            right: 4.0,
            top: 0.0,
            bottom: 10.0,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(title)
                        .size(FontSize::SUBHEADING)
                        .strong()
                        .color(if is_running {
                            AppColors::TEXT_DIM
                        } else {
                            AppColors::TEXT
                        }),
                );
                if is_running {
                    ui.label(
                        egui::RichText::new("  \u{1F512}")
                            .size(10.0)
                            .color(AppColors::MUTED),
                    );
                }
            });
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            ui.add_enabled_ui(!is_running, |ui| {
                body(ui);
            });
        });
}

// ─── Card: Metadata ──────────────────────────────────────────────────

fn card_metadata(app: &mut LibreVmmApp, ui: &mut egui::Ui, is_running: bool) {
    settings_card(ui, &t!("vmsettings.section-metadata"), |ui| {
        egui::Grid::new("vm_settings_metadata")
            .num_columns(2)
            .spacing([Spacing::SM, GRID_ROW_GAP])
            .show(ui, |ui| {
                // Name (read-only when running)
                ui.label(t!("vmsettings.name").to_string());
                if is_running {
                    if let Some(ref config) = app.editing_config() {
                        ui.label(
                            egui::RichText::new(&config.name)
                                .size(theme::FontSize::BODY)
                                .color(AppColors::TEXT_DIM),
                        );
                    }
                } else if let Some(ref mut config) = app.editing_config_mut() {
                    ui.add(
                        egui::TextEdit::singleline(&mut config.name)
                            .desired_width(TEXT_FIELD_WIDTH),
                    );
                }
                ui.end_row();

                // Description (always editable)
                ui.label(t!("vmsettings.description").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    ui.add(
                        egui::TextEdit::multiline(&mut config.description)
                            .desired_width(ui.available_width())
                            .desired_rows(2)
                            .hint_text(t!("vmsettings.optional-notes").to_string()),
                    );
                }
                ui.end_row();

                // OS Type
                ui.label(t!("vmsettings.os-type").to_string());
                if is_running {
                    if let Some(ref config) = app.editing_config() {
                        ui.label(
                            egui::RichText::new(os_type_label(&config.os_type))
                                .size(theme::FontSize::BODY)
                                .color(AppColors::TEXT_DIM),
                        );
                    }
                } else if let Some(config) = app.editing_config().cloned() {
                    let current = config.os_type.clone();
                    egui::ComboBox::from_id_salt("os_type_edit")
                        .selected_text(os_type_label(&current))
                        .show_ui(ui, |ui| {
                            for ot in &[
                                OsType::Linux,
                                OsType::Windows,
                                OsType::FreeBSD,
                                OsType::MacOS,
                                OsType::Other,
                            ] {
                                if ui
                                    .selectable_label(*ot == current, os_type_label(ot))
                                    .clicked()
                                {
                                    if let Some(ref mut c) = app.editing_config_mut() {
                                        c.os_type = ot.clone();
                                    }
                                }
                            }
                        });
                }
                ui.end_row();

                // Tags
                ui.label(t!("vmsettings.tags").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    let tags_text = config.tags.join(", ");
                    let mut tags_edit = tags_text;
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut tags_edit)
                            .desired_width(ui.available_width())
                            .hint_text(t!("vmsettings.tags-hint").to_string()),
                    );
                    if resp.changed() {
                        config.tags = tags_edit
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
                ui.end_row();

                // Autostart
                ui.label(t!("vmsettings.autostart").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    ui.checkbox(
                        &mut config.autostart,
                        t!("vmsettings.autostart-desc").to_string(),
                    );
                }
                ui.end_row();

                // Favorite
                ui.label(t!("vmsettings.favorite").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    ui.checkbox(
                        &mut config.favorite,
                        t!("vmsettings.favorite-desc").to_string(),
                    );
                }
                ui.end_row();
            });
    });
}

// ─── Card: Hardware ──────────────────────────────────────────────────

fn card_hardware(app: &mut LibreVmmApp, ui: &mut egui::Ui, is_running: bool) {
    settings_card_hw(ui, &t!("vmsettings.section-hardware"), is_running, |ui| {
        egui::Grid::new("vm_settings_hardware")
            .num_columns(2)
            .spacing([Spacing::SM, GRID_ROW_GAP])
            .show(ui, |ui| {
                // CPUs
                ui.label(t!("vmsettings.processors").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    let mut cpus = (config.vcpus.min(16)) as i32;
                    ui.add(egui::Slider::new(&mut cpus, 1..=16).text("vCPUs"));
                    config.vcpus = cpus.max(1) as u32;
                }
                ui.end_row();

                // Memory
                ui.label(t!("vmsettings.memory").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    let mut mem = (config.memory_mib.min(65536)) as i32;
                    ui.add(
                        egui::Slider::new(&mut mem, 512..=65536)
                            .text("MiB")
                            .step_by(512.0),
                    );
                    config.memory_mib = mem.max(512) as u64;
                }
                ui.end_row();

                // Disk size + resize button
                ui.label(t!("vmsettings.disk-size").to_string());
                if let Some(config) = app.editing_config().cloned() {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(
                                t!("vmsettings.disk-size-info", size = config.disk_size_gib)
                                    .to_string(),
                            )
                            .size(theme::FontSize::BODY)
                            .color(AppColors::TEXT_DIM),
                        );
                        if !is_running {
                            if ui
                                .small_button(t!("vmsettings.resize-disk").to_string())
                                .clicked()
                            {
                                app.show_disk_resize_dialog();
                            }
                        }
                    });
                }
                ui.end_row();
            });
    });
}

// ─── Card: Boot ──────────────────────────────────────────────────────

fn card_boot(app: &mut LibreVmmApp, ui: &mut egui::Ui, is_running: bool) {
    settings_card_hw(ui, &t!("vmsettings.section-boot"), is_running, |ui| {
        egui::Grid::new("vm_settings_boot")
            .num_columns(2)
            .spacing([Spacing::SM, GRID_ROW_GAP])
            .show(ui, |ui| {
                // UEFI
                ui.label(t!("vmsettings.uefi-boot").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    ui.checkbox(&mut config.uefi, t!("vmsettings.uefi-desc").to_string());
                }
                ui.end_row();

                // Secure Boot (only visible when UEFI is enabled)
                let show_secboot = app.editing_config().map(|c| c.uefi).unwrap_or(false);
                if show_secboot {
                    ui.label(t!("vmsettings.secure-boot").to_string());
                    if let Some(ref mut config) = app.editing_config_mut() {
                        ui.checkbox(
                            &mut config.secure_boot,
                            t!("vmsettings.secure-boot-desc").to_string(),
                        );
                    }
                    ui.end_row();
                }

                // Firmware chooser (only visible when UEFI is enabled).
                //
                // The LibreUEFI paths below are the canonical install locations
                // a future `libreuefi` distribution package would target. They
                // mirror Debian's OVMF layout (`/usr/share/OVMF/OVMF_CODE.fd`).
                // On systems where LibreUEFI isn't installed at these paths,
                // selecting "LibreUEFI" in the firmware picker will produce a
                // config that points at non-existent files — the user can then
                // switch to Custom mode and browse to their own build, or
                // install a libreuefi package once one is published.
                if show_secboot {
                    let libreuefi_code = "/usr/share/libreuefi/OVMF_CODE.fd";
                    let libreuefi_vars = "/usr/share/libreuefi/OVMF_VARS.fd";

                    // Determine current firmware mode: 0=System, 1=LibreUEFI, 2=Custom
                    let fw_mode = app
                        .editing_config()
                        .map(
                            |c| match (&c.custom_firmware_code, &c.custom_firmware_vars) {
                                (None, None) => 0u8,
                                (Some(c), Some(v))
                                    if c == libreuefi_code && v == libreuefi_vars =>
                                {
                                    1
                                },
                                (Some(_), _) => 2,
                                _ => 0,
                            },
                        )
                        .unwrap_or(0);

                    ui.label(t!("vmsettings.firmware").to_string());
                    if let Some(ref mut config) = app.editing_config_mut() {
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(fw_mode == 0, t!("vmsettings.firmware-system"))
                                .clicked()
                            {
                                config.custom_firmware_code = None;
                                config.custom_firmware_vars = None;
                            }
                            if ui
                                .selectable_label(fw_mode == 1, t!("vmsettings.firmware-libreuefi"))
                                .clicked()
                            {
                                config.custom_firmware_code = Some(libreuefi_code.to_string());
                                config.custom_firmware_vars = Some(libreuefi_vars.to_string());
                            }
                            if ui
                                .selectable_label(fw_mode == 2, t!("vmsettings.firmware-custom"))
                                .clicked()
                                && fw_mode != 2
                            {
                                config.custom_firmware_code = Some(String::new());
                                config.custom_firmware_vars = Some(String::new());
                            }
                        });
                    }
                    ui.end_row();

                    // Show custom path fields when in Custom mode
                    if fw_mode == 2 {
                        ui.label(t!("vmsettings.firmware-code-path").to_string());
                        if let Some(ref mut config) = app.editing_config_mut() {
                            ui.horizontal(|ui| {
                                let mut code =
                                    config.custom_firmware_code.clone().unwrap_or_default();
                                if ui
                                    .add(
                                        egui::TextEdit::singleline(&mut code)
                                            .desired_width(TEXT_FIELD_NARROW),
                                    )
                                    .changed()
                                {
                                    config.custom_firmware_code = Some(code);
                                }
                                if ui.button(t!("vmsettings.browse")).clicked() {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("Firmware", &["fd", "bin", "rom"])
                                        .pick_file()
                                    {
                                        config.custom_firmware_code =
                                            Some(path.display().to_string());
                                    }
                                }
                            });
                        }
                        ui.end_row();

                        ui.label(t!("vmsettings.firmware-vars-path").to_string());
                        if let Some(ref mut config) = app.editing_config_mut() {
                            ui.horizontal(|ui| {
                                let mut vars =
                                    config.custom_firmware_vars.clone().unwrap_or_default();
                                if ui
                                    .add(
                                        egui::TextEdit::singleline(&mut vars)
                                            .desired_width(TEXT_FIELD_NARROW),
                                    )
                                    .changed()
                                {
                                    config.custom_firmware_vars = Some(vars);
                                }
                                if ui.button(t!("vmsettings.browse")).clicked() {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("Firmware", &["fd", "bin", "rom"])
                                        .pick_file()
                                    {
                                        config.custom_firmware_vars =
                                            Some(path.display().to_string());
                                    }
                                }
                            });
                        }
                        ui.end_row();
                    }

                    // Hint text for LibreUEFI
                    if fw_mode == 1 {
                        ui.label("");
                        ui.label(
                            egui::RichText::new(t!("vmsettings.firmware-hint"))
                                .size(FontSize::CAPTION)
                                .color(AppColors::TEXT_DIM),
                        );
                        ui.end_row();
                    }
                }

                // Boot ISO
                ui.label(t!("vmsettings.boot-iso").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    ui.horizontal(|ui| {
                        let mut iso = config.iso_path.clone().unwrap_or_default();
                        let changed = ui
                            .add(
                                egui::TextEdit::singleline(&mut iso)
                                    .hint_text(t!("vmsettings.empty").to_string())
                                    .desired_width(TEXT_FIELD_NARROW),
                            )
                            .changed();
                        if changed {
                            config.iso_path = if iso.is_empty() { None } else { Some(iso) };
                        }
                        if ui.button(t!("vmsettings.browse").to_string()).clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("ISO Images", &["iso", "img"])
                                .pick_file()
                            {
                                config.iso_path = Some(path.display().to_string());
                            }
                        }
                        if config.iso_path.is_some() {
                            if ui.button(t!("vmsettings.clear").to_string()).clicked() {
                                config.iso_path = None;
                            }
                        }
                    });
                }
                ui.end_row();
            });

        ui.add_space(Spacing::SM);

        // Boot Order editor
        ui.label(
            egui::RichText::new(t!("vmsettings.boot-order").to_string())
                .size(theme::FontSize::BODY)
                .color(AppColors::TEXT),
        );
        ui.add_space(theme::Spacing::XS);
        ui.label(
            egui::RichText::new(t!("vmsettings.boot-order-hint").to_string())
                .size(theme::FontSize::SMALL)
                .color(AppColors::TEXT_DIM),
        );
        ui.add_space(theme::Spacing::XS);

        render_boot_order_editor(app, ui);
    });
}

// ─── Card: Network ───────────────────────────────────────────────────

fn card_network(app: &mut LibreVmmApp, ui: &mut egui::Ui, is_running: bool) {
    settings_card_hw(ui, &t!("vmsettings.section-network"), is_running, |ui| {
        render_nic_editor(app, ui);
    });
}

// ─── Card: Display & GPU ─────────────────────────────────────────────

fn card_display_gpu(app: &mut LibreVmmApp, ui: &mut egui::Ui, is_running: bool) {
    settings_card_hw(ui, &t!("vmsettings.section-display"), is_running, |ui| {
        egui::Grid::new("vm_settings_display_gpu")
            .num_columns(2)
            .spacing([Spacing::MD, GRID_ROW_GAP])
            .show(ui, |ui| {
                // GPU Model
                ui.label(t!("vmsettings.gpu-model").to_string());
                if let Some(config) = app.editing_config().cloned() {
                    let current = config.gpu_model;
                    egui::ComboBox::from_id_salt("vm_gpu_model")
                        .selected_text(current.to_string())
                        .show_ui(ui, |ui| {
                            for &model in GpuModel::ALL {
                                if ui
                                    .selectable_label(current == model, model.to_string())
                                    .clicked()
                                {
                                    if let Some(ref mut c) = app.editing_config_mut() {
                                        c.gpu_model = model;
                                        if model == GpuModel::VirtioGpuGl {
                                            c.gpu_accel = true;
                                        }
                                    }
                                }
                            }
                        });
                }
                ui.end_row();

                // Video RAM
                ui.label(t!("vmsettings.video-ram").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    let mut vram = config.video_ram_mb.clamp(16, 256) as i32;
                    ui.add(
                        egui::Slider::new(&mut vram, 16..=256)
                            .text("MiB")
                            .step_by(16.0),
                    );
                    config.video_ram_mb = vram as u32;
                }
                ui.end_row();

                // 3D Acceleration
                ui.label(t!("vmsettings.gpu-accel").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    let can_3d = config.gpu_model.supports_3d();
                    ui.add_enabled(
                        can_3d,
                        egui::Checkbox::new(
                            &mut config.gpu_accel,
                            t!("vmsettings.gpu-accel-desc").to_string(),
                        ),
                    );
                    if !can_3d && config.gpu_accel {
                        config.gpu_accel = false;
                    }
                }
                ui.end_row();

                // Display Protocol
                ui.label(t!("vmsettings.display-protocol").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    let current = config.display_protocol;
                    egui::ComboBox::from_id_salt("vm_display_protocol")
                        .selected_text(current.to_string())
                        .show_ui(ui, |ui| {
                            for &proto in vmm_core::config::DisplayProtocol::ALL {
                                let label = format!("{} — {}", proto, proto.description());
                                if ui.selectable_label(current == proto, label).clicked() {
                                    config.display_protocol = proto;
                                }
                            }
                        });
                }
                ui.end_row();

                // Display Heads
                ui.label(t!("vmsettings.display-heads").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    let mut heads = (config.display_count.min(4)) as i32;
                    ui.add(
                        egui::Slider::new(&mut heads, 1..=4)
                            .text(t!("vmsettings.monitors").to_string()),
                    );
                    config.display_count = heads.clamp(1, 4) as u8;
                }
                ui.end_row();
            });

        // GPU capabilities info (always visible, inside the card)
        ui.add_space(Spacing::SM);
        app.detect_gpu();
        if let Some(ref caps) = app.gpu_capabilities() {
            egui::Frame::none()
                .fill(AppColors::BG_CARD.linear_multiply(0.7))
                .rounding(theme::ThemeRounding::BUTTON_SMALL)
                .inner_margin(theme::Spacing::SM)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(t!("vmsettings.host-gpu-info").to_string())
                            .size(theme::FontSize::SMALL)
                            .strong()
                            .color(AppColors::TEXT_DIM),
                    );
                    ui.add_space(2.0);
                    if caps.virgl_supported {
                        ui.label(
                            egui::RichText::new(format!(
                                "\u{2714} {}",
                                t!("vmsettings.virgl-supported")
                            ))
                            .size(theme::FontSize::SMALL)
                            .color(AppColors::SUCCESS),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new(format!(
                                "\u{2718} {}",
                                t!("vmsettings.virgl-not-detected")
                            ))
                            .size(theme::FontSize::SMALL)
                            .color(AppColors::TEXT_DIM),
                        );
                    }
                    if let Some(ref renderer) = caps.gl_renderer {
                        ui.label(
                            egui::RichText::new(format!("Renderer: {}", renderer))
                                .size(theme::FontSize::SMALL)
                                .color(AppColors::TEXT_DIM),
                        );
                    }
                    if !caps.vfio_devices.is_empty() {
                        for dev in &caps.vfio_devices {
                            let status = if dev.vfio_bound { "VFIO" } else { "host" };
                            ui.label(
                                egui::RichText::new(format!(
                                    "GPU: {} [{}]",
                                    dev.description, status
                                ))
                                .size(theme::FontSize::SMALL)
                                .color(AppColors::TEXT_DIM),
                            );
                        }
                    }
                });
        }
    });
}

// ─── Card: AutoProtect ───────────────────────────────────────────────

fn card_autoprotect(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    settings_card(ui, &t!("vmsettings.section-autoprotect"), |ui| {
        egui::Grid::new("vm_settings_autoprotect")
            .num_columns(2)
            .spacing([Spacing::SM, GRID_ROW_GAP])
            .show(ui, |ui| {
                ui.label(t!("vmsettings.enabled").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    ui.checkbox(
                        &mut config.auto_snapshot.enabled,
                        t!("vmsettings.autoprotect-desc").to_string(),
                    );
                }
                ui.end_row();

                ui.label(t!("vmsettings.interval").to_string());
                if let Some(config) = app.editing_config().cloned() {
                    let current = config.auto_snapshot.interval_hours;
                    static INTERVAL_HOURS: &[u32] = &[1, 4, 8, 12, 24, 168];
                    let interval_label = |h: u32| -> String {
                        match h {
                            1 => t!("vmsettings.every-hour").to_string(),
                            4 => t!("vmsettings.every-4-hours").to_string(),
                            8 => t!("vmsettings.every-8-hours").to_string(),
                            12 => t!("vmsettings.every-12-hours").to_string(),
                            24 => t!("vmsettings.daily").to_string(),
                            168 => t!("vmsettings.weekly").to_string(),
                            _ => t!("vmsettings.custom").to_string(),
                        }
                    };
                    let selected_label = interval_label(current);
                    egui::ComboBox::from_id_salt("autosnap_interval")
                        .selected_text(&selected_label)
                        .show_ui(ui, |ui| {
                            for &val in INTERVAL_HOURS {
                                if ui
                                    .selectable_label(current == val, interval_label(val))
                                    .clicked()
                                {
                                    if let Some(ref mut c) = app.editing_config_mut() {
                                        c.auto_snapshot.interval_hours = val;
                                    }
                                }
                            }
                        });
                }
                ui.end_row();

                ui.label(t!("vmsettings.retention").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    let mut ret = config.auto_snapshot.retention.min(50) as i32;
                    ui.add(
                        egui::Slider::new(&mut ret, 1..=50)
                            .text(t!("vmsettings.snapshots-to-keep").to_string()),
                    );
                    config.auto_snapshot.retention = ret.max(1) as u32;
                }
                ui.end_row();
            });

        ui.add_space(theme::Spacing::XS);
        ui.label(
            egui::RichText::new(t!("vmsettings.autoprotect-hint").to_string())
                .size(theme::FontSize::SMALL)
                .color(AppColors::TEXT_DIM),
        );
    });
}

// ─── Card: Organization ──────────────────────────────────────────────

fn card_organization(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    settings_card(ui, &t!("vmsettings.section-organization"), |ui| {
        egui::Grid::new("vm_settings_organization")
            .num_columns(2)
            .spacing([Spacing::SM, GRID_ROW_GAP])
            .show(ui, |ui| {
                ui.label(t!("vmsettings.group").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    let mut group = config.folder.clone().unwrap_or_default();
                    let changed = ui
                        .add(
                            egui::TextEdit::singleline(&mut group)
                                .desired_width(ui.available_width())
                                .hint_text(t!("vmsettings.group-examples").to_string()),
                        )
                        .changed();
                    if changed {
                        config.folder = if group.is_empty() { None } else { Some(group) };
                    }
                }
                ui.end_row();
            });

        ui.add_space(theme::Spacing::XS);
        ui.label(
            egui::RichText::new(t!("vmsettings.group-hint").to_string())
                .size(theme::FontSize::SMALL)
                .color(AppColors::TEXT_DIM),
        );
    });
}

// ─── Card: Advanced ──────────────────────────────────────────────────

// ─── Card: Devices (USB, Audio, TPM, Battery, Shared Folder) ────────

fn card_devices(app: &mut LibreVmmApp, ui: &mut egui::Ui, is_running: bool) {
    settings_card_hw(ui, &t!("vmsettings.section-devices"), is_running, |ui| {
        egui::Grid::new("vm_settings_devices")
            .num_columns(2)
            .spacing([Spacing::MD, GRID_ROW_GAP])
            .show(ui, |ui| {
                // USB Support toggle
                ui.label(t!("vmsettings.usb-support").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    ui.checkbox(
                        &mut config.usb_support,
                        t!("vmsettings.usb-enabled").to_string(),
                    );
                }
                ui.end_row();

                // USB Controller Version (only when USB is enabled)
                let usb_on = app.editing_config().map(|c| c.usb_support).unwrap_or(false);
                if usb_on {
                    ui.label(t!("vmsettings.usb-version").to_string());
                    if let Some(config) = app.editing_config().cloned() {
                        let current = config.usb_controller;
                        egui::ComboBox::from_id_salt("vm_usb_controller")
                            .selected_text(current.to_string())
                            .show_ui(ui, |ui| {
                                for &ver in UsbControllerVersion::ALL {
                                    if ui
                                        .selectable_label(current == ver, ver.to_string())
                                        .clicked()
                                    {
                                        if let Some(ref mut c) = app.editing_config_mut() {
                                            c.usb_controller = ver;
                                        }
                                    }
                                }
                            });
                    }
                    ui.end_row();
                }

                ui.label(t!("vmsettings.audio").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    ui.checkbox(&mut config.audio, t!("vmsettings.audio-desc").to_string());
                }
                ui.end_row();

                // TPM 2.0
                ui.label(t!("vmsettings.tpm").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    ui.checkbox(
                        &mut config.tpm_enabled,
                        t!("vmsettings.tpm-desc").to_string(),
                    );
                }
                ui.end_row();

                // Battery reporting
                ui.label(t!("vmsettings.report-battery").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    ui.checkbox(
                        &mut config.report_battery,
                        t!("vmsettings.report-battery-desc").to_string(),
                    );
                }
                ui.end_row();

                // Shared folder
                ui.label(t!("vmsettings.shared-folder").to_string());
                if let Some(ref mut config) = app.editing_config_mut() {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let mut folder = config.shared_folder.clone().unwrap_or_default();
                            let changed = ui
                                .add(
                                    egui::TextEdit::singleline(&mut folder)
                                        .hint_text(t!("vmsettings.none").to_string())
                                        .desired_width(TEXT_FIELD_NARROW),
                                )
                                .changed();
                            if changed {
                                config.shared_folder = if folder.is_empty() {
                                    None
                                } else {
                                    Some(folder)
                                };
                            }
                            if ui.button(t!("vmsettings.browse").to_string()).clicked() {
                                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                    config.shared_folder = Some(path.display().to_string());
                                }
                            }
                        });
                    });
                }
                ui.end_row();
            });
    });
}

// ─── Card: Serial Ports ─────────────────────────────────────────────

fn card_serial_ports(app: &mut LibreVmmApp, ui: &mut egui::Ui, is_running: bool) {
    settings_card_hw(ui, &t!("vmsettings.serial-ports"), is_running, |ui| {
        render_port_editor(app, ui, PortKind::Serial);
    });
}

// ─── Card: Parallel Ports ───────────────────────────────────────────

fn card_parallel_ports(app: &mut LibreVmmApp, ui: &mut egui::Ui, is_running: bool) {
    settings_card_hw(ui, &t!("vmsettings.parallel-ports"), is_running, |ui| {
        render_port_editor(app, ui, PortKind::Parallel);
    });
}

// ─── Card: Passthrough & Security ───────────────────────────────────

fn card_passthrough(app: &mut LibreVmmApp, ui: &mut egui::Ui, is_running: bool) {
    settings_card_hw(
        ui,
        &t!("vmsettings.section-passthrough"),
        is_running,
        |ui| {
            egui::Grid::new("vm_settings_passthrough")
                .num_columns(2)
                .spacing([Spacing::MD, GRID_ROW_GAP])
                .show(ui, |ui| {
                    // PCI Passthrough
                    ui.label(t!("pci.title").to_string());
                    ui.horizontal(|ui| {
                        let n_devs = app
                            .editing_config()
                            .map(|c| c.vfio_devices.len())
                            .unwrap_or(0);
                        let label = if n_devs > 0 {
                            format!("{} device(s)", n_devs)
                        } else {
                            "None".to_string()
                        };
                        ui.label(
                            egui::RichText::new(label)
                                .size(12.0)
                                .color(AppColors::TEXT_DIM),
                        );
                        if ui.button(t!("pci.scan").to_string()).clicked() {
                            app.pci_passthrough_state_mut().open = true;
                        }
                    });
                    ui.end_row();

                    // Looking Glass
                    ui.label(t!("pci.looking-glass").to_string());
                    ui.vertical(|ui| {
                        let mut lg_enabled = app
                            .editing_config()
                            .map(|c| c.looking_glass.enabled)
                            .unwrap_or(false);
                        let has_vfio = app
                            .editing_config()
                            .map(|c| !c.vfio_devices.is_empty())
                            .unwrap_or(false);
                        if ui
                            .checkbox(&mut lg_enabled, t!("pci.lg-enable").to_string())
                            .changed()
                        {
                            if let Some(ref mut config) = app.editing_config_mut() {
                                config.looking_glass.enabled = lg_enabled;
                            }
                        }
                        if lg_enabled {
                            ui.add_space(theme::Spacing::XS);
                            ui.horizontal(|ui| {
                                let mut size = app
                                    .editing_config()
                                    .map(|c| c.looking_glass.ivshmem_size_mib)
                                    .unwrap_or(64);
                                ui.label(
                                    egui::RichText::new("IVSHMEM:")
                                        .size(theme::FontSize::SMALL)
                                        .color(AppColors::TEXT_DIM),
                                );
                                let sizes: Vec<u32> = vec![32, 64, 128, 256];
                                egui::ComboBox::from_id_salt("lg_ivshmem_size")
                                    .width(70.0)
                                    .selected_text(format!("{} MiB", size))
                                    .show_ui(ui, |ui| {
                                        for s in &sizes {
                                            if ui
                                                .selectable_value(
                                                    &mut size,
                                                    *s,
                                                    format!("{} MiB", s),
                                                )
                                                .changed()
                                            {
                                                if let Some(ref mut config) =
                                                    app.editing_config_mut()
                                                {
                                                    config.looking_glass.ivshmem_size_mib = size;
                                                }
                                            }
                                        }
                                    });
                            });
                            let mut auto_launch = app
                                .editing_config()
                                .map(|c| c.looking_glass.auto_launch)
                                .unwrap_or(true);
                            if ui
                                .checkbox(&mut auto_launch, t!("pci.lg-auto-launch").to_string())
                                .changed()
                            {
                                if let Some(ref mut config) = app.editing_config_mut() {
                                    config.looking_glass.auto_launch = auto_launch;
                                }
                            }
                            if !has_vfio {
                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new(t!("pci.lg-needs-gpu").to_string())
                                        .size(10.0)
                                        .color(AppColors::WARNING),
                                );
                            }
                        }
                    });
                    ui.end_row();

                    // Disk encryption
                    ui.label(t!("vmsettings.disk-encryption").to_string());
                    if let Some(ref config) = app.editing_config() {
                        if config.disk_encrypted {
                            ui.label(
                                egui::RichText::new("\u{1F512} LUKS Encrypted")
                                    .size(theme::FontSize::BODY)
                                    .color(AppColors::SUCCESS),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new(t!("vmsettings.not-encrypted").to_string())
                                    .size(12.0)
                                    .color(AppColors::TEXT_DIM),
                            );
                        }
                    }
                    ui.end_row();
                });
        },
    );
}

// ─── Card: Resource Limits ───────────────────────────────────────────

fn card_resource_limits(app: &mut LibreVmmApp, ui: &mut egui::Ui, is_running: bool) {
    settings_card_hw(ui, &t!("vmsettings.section-resources"), is_running, |ui| {
        crate::views::resource_limits::render_section(app, ui);
    });
}

// ─── Boot order editor ──────────────────────────────────────────────

fn render_boot_order_editor(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let all_devices = BootDevice::all();
    let Some(boot_order) = app.editing_config().map(|c| c.boot_order.clone()) else {
        return;
    };

    let mut move_up: Option<usize> = None;
    let mut move_down: Option<usize> = None;
    let mut remove_idx: Option<usize> = None;

    for (i, device) in boot_order.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{}.", i + 1))
                    .size(theme::FontSize::BODY)
                    .color(AppColors::PRIMARY)
                    .strong(),
            );

            ui.label(
                egui::RichText::new(device.to_string())
                    .size(theme::FontSize::BODY)
                    .color(AppColors::TEXT),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("\u{2716}")
                    .on_hover_text(t!("vmsettings.remove-boot-device").to_string())
                    .clicked()
                {
                    remove_idx = Some(i);
                }

                let can_down = boot_order.len() > 1 && i < boot_order.len() - 1;
                if ui
                    .add_enabled(can_down, egui::Button::new("\u{25BC}").small())
                    .on_hover_text(t!("vmsettings.move-down").to_string())
                    .clicked()
                {
                    move_down = Some(i);
                }

                let can_up = i > 0;
                if ui
                    .add_enabled(can_up, egui::Button::new("\u{25B2}").small())
                    .on_hover_text(t!("vmsettings.move-up").to_string())
                    .clicked()
                {
                    move_up = Some(i);
                }
            });
        });
    }

    if let Some(i) = move_up {
        if let Some(ref mut config) = app.editing_config_mut() {
            if i > 0 && i < config.boot_order.len() {
                config.boot_order.swap(i, i - 1);
            }
        }
    }
    if let Some(i) = move_down {
        if let Some(ref mut config) = app.editing_config_mut() {
            if i + 1 < config.boot_order.len() {
                config.boot_order.swap(i, i + 1);
            }
        }
    }
    if let Some(i) = remove_idx {
        if let Some(ref mut config) = app.editing_config_mut() {
            if i < config.boot_order.len() {
                config.boot_order.remove(i);
            }
        }
    }

    let available: Vec<&BootDevice> = all_devices
        .iter()
        .filter(|d| !boot_order.contains(d))
        .collect();

    if !available.is_empty() {
        ui.add_space(theme::Spacing::XS);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(t!("vmsettings.add-device").to_string())
                    .size(12.0)
                    .color(AppColors::TEXT_DIM),
            );
            for dev in available {
                if ui.small_button(&dev.to_string()).clicked() {
                    if let Some(ref mut config) = app.editing_config_mut() {
                        config.boot_order.push(dev.clone());
                    }
                }
            }
        });
    }
}

// ─── NIC editor ─────────────────────────────────────────────────────

fn render_nic_editor(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let Some(nics) = app.editing_config().map(|c| c.network_interfaces.clone()) else {
        return;
    };
    let legacy_mode = nics.is_empty();

    if legacy_mode {
        ui.label(
            egui::RichText::new(t!("vmsettings.legacy-nic-hint").to_string())
                .size(theme::FontSize::SMALL)
                .color(AppColors::TEXT_DIM),
        );
        ui.add_space(theme::Spacing::XS);

        egui::Grid::new("vm_settings_network_legacy")
            .num_columns(2)
            .spacing([Spacing::SM, GRID_ROW_GAP])
            .show(ui, |ui| {
                ui.label(t!("vmsettings.mode").to_string());
                if let Some(config) = app.editing_config().cloned() {
                    let current = config.network.clone();
                    egui::ComboBox::from_id_salt("network_edit")
                        .selected_text(network_label(&current))
                        .show_ui(ui, |ui| {
                            for nm in &[
                                NetworkMode::Nat,
                                NetworkMode::Bridged,
                                NetworkMode::HostOnly,
                                NetworkMode::None,
                            ] {
                                if ui
                                    .selectable_label(*nm == current, network_label(nm))
                                    .clicked()
                                {
                                    if let Some(ref mut c) = app.editing_config_mut() {
                                        c.network = nm.clone();
                                    }
                                }
                            }
                        });
                }
                ui.end_row();
            });

        ui.add_space(Spacing::SM);
    }

    let mut remove_idx: Option<usize> = None;

    for (i, nic) in nics.iter().enumerate() {
        egui::Frame::none()
            .fill(AppColors::BG_CARD.linear_multiply(0.7))
            .rounding(theme::ThemeRounding::BUTTON)
            .inner_margin(10.0)
            .stroke(egui::Stroke::new(0.5, egui::Color32::from_rgb(55, 60, 75)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(t!("vmsettings.nic-number", n = i + 1).to_string())
                            .size(theme::FontSize::BODY)
                            .strong()
                            .color(AppColors::PRIMARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(format!("\u{2716} {}", t!("vmsettings.remove-nic")))
                            .clicked()
                        {
                            remove_idx = Some(i);
                        }
                    });
                });

                egui::Grid::new(format!("nic_grid_{}", i))
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        // Mode
                        ui.label(t!("vmsettings.mode").to_string());
                        let current_mode = nic.mode.clone();
                        egui::ComboBox::from_id_salt(format!("nic_mode_{}", i))
                            .selected_text(network_label(&current_mode))
                            .show_ui(ui, |ui| {
                                for nm in &[
                                    NetworkMode::Nat,
                                    NetworkMode::Bridged,
                                    NetworkMode::HostOnly,
                                    NetworkMode::None,
                                ] {
                                    if ui
                                        .selectable_label(*nm == current_mode, network_label(nm))
                                        .clicked()
                                    {
                                        if let Some(ref mut c) = app.editing_config_mut() {
                                            if let Some(ref mut n) = c.network_interfaces.get_mut(i)
                                            {
                                                n.mode = nm.clone();
                                            }
                                        }
                                    }
                                }
                            });
                        ui.end_row();

                        // Model
                        ui.label(t!("vmsettings.model").to_string());
                        let current_model = nic.model.clone();
                        egui::ComboBox::from_id_salt(format!("nic_model_{}", i))
                            .selected_text(&current_model)
                            .show_ui(ui, |ui| {
                                for model in &["virtio", "e1000e", "rtl8139"] {
                                    if ui
                                        .selectable_label(current_model == *model, *model)
                                        .clicked()
                                    {
                                        if let Some(ref mut c) = app.editing_config_mut() {
                                            if let Some(ref mut n) = c.network_interfaces.get_mut(i)
                                            {
                                                n.model = model.to_string();
                                            }
                                        }
                                    }
                                }
                            });
                        ui.end_row();

                        // MAC
                        ui.label(t!("vmsettings.mac").to_string());
                        let mut mac = nic.mac.clone();
                        let changed = ui
                            .add(
                                egui::TextEdit::singleline(&mut mac)
                                    .hint_text(t!("vmsettings.auto-generated").to_string())
                                    .desired_width(180.0),
                            )
                            .changed();
                        if changed {
                            if let Some(ref mut c) = app.editing_config_mut() {
                                if let Some(ref mut n) = c.network_interfaces.get_mut(i) {
                                    n.mac = mac;
                                }
                            }
                        }
                        ui.end_row();
                    });
            });

        ui.add_space(theme::Spacing::XS);
    }

    if let Some(i) = remove_idx {
        if let Some(ref mut config) = app.editing_config_mut() {
            config.network_interfaces.remove(i);
        }
    }

    ui.add_space(theme::Spacing::XS);
    let add_btn =
        egui::Button::new(egui::RichText::new(t!("vmsettings.add-nic").to_string()).size(12.0))
            .rounding(theme::ThemeRounding::BUTTON_SMALL);
    if ui.add(add_btn).clicked() {
        if let Some(ref mut config) = app.editing_config_mut() {
            config.network_interfaces.push(NicConfig::default());
        }
    }
}

// ─── Serial / Parallel port editor ──────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum PortKind {
    Serial,
    Parallel,
}

impl PortKind {
    fn cap(self) -> usize {
        match self {
            PortKind::Serial => 4,
            PortKind::Parallel => 3,
        }
    }

    fn label_prefix(self) -> &'static str {
        match self {
            PortKind::Serial => "Serial",
            PortKind::Parallel => "Parallel",
        }
    }

    fn id_salt(self) -> &'static str {
        match self {
            PortKind::Serial => "serial",
            PortKind::Parallel => "parallel",
        }
    }
}

#[derive(Clone)]
struct PortRow {
    backend: SerialBackend,
    target: String,
}

fn read_port_rows(app: &LibreVmmApp, kind: PortKind) -> Vec<PortRow> {
    let Some(config) = app.editing_config() else {
        return Vec::new();
    };
    match kind {
        PortKind::Serial => config
            .serial_ports
            .iter()
            .map(|p| PortRow {
                backend: p.backend,
                target: p.target.clone(),
            })
            .collect(),
        PortKind::Parallel => config
            .parallel_ports
            .iter()
            .map(|p| PortRow {
                backend: p.backend,
                target: p.target.clone(),
            })
            .collect(),
    }
}

fn write_port_backend(app: &mut LibreVmmApp, kind: PortKind, i: usize, backend: SerialBackend) {
    if let Some(ref mut c) = app.editing_config_mut() {
        match kind {
            PortKind::Serial => {
                if let Some(p) = c.serial_ports.get_mut(i) {
                    p.backend = backend;
                    if matches!(backend, SerialBackend::Pty | SerialBackend::Null) {
                        p.target.clear();
                    }
                }
            },
            PortKind::Parallel => {
                if let Some(p) = c.parallel_ports.get_mut(i) {
                    p.backend = backend;
                    if matches!(backend, SerialBackend::Pty | SerialBackend::Null) {
                        p.target.clear();
                    }
                }
            },
        }
    }
}

fn write_port_target(app: &mut LibreVmmApp, kind: PortKind, i: usize, target: String) {
    if let Some(ref mut c) = app.editing_config_mut() {
        match kind {
            PortKind::Serial => {
                if let Some(p) = c.serial_ports.get_mut(i) {
                    p.target = target;
                }
            },
            PortKind::Parallel => {
                if let Some(p) = c.parallel_ports.get_mut(i) {
                    p.target = target;
                }
            },
        }
    }
}

fn remove_port(app: &mut LibreVmmApp, kind: PortKind, i: usize) {
    if let Some(ref mut c) = app.editing_config_mut() {
        match kind {
            PortKind::Serial => {
                if i < c.serial_ports.len() {
                    c.serial_ports.remove(i);
                }
            },
            PortKind::Parallel => {
                if i < c.parallel_ports.len() {
                    c.parallel_ports.remove(i);
                }
            },
        }
    }
}

fn add_port(app: &mut LibreVmmApp, kind: PortKind) {
    if let Some(ref mut c) = app.editing_config_mut() {
        match kind {
            PortKind::Serial => {
                if c.serial_ports.len() < PortKind::Serial.cap() {
                    c.serial_ports.push(SerialPortConfig::default());
                }
            },
            PortKind::Parallel => {
                if c.parallel_ports.len() < PortKind::Parallel.cap() {
                    c.parallel_ports.push(ParallelPortConfig::default());
                }
            },
        }
    }
}

fn port_target_placeholder(backend: SerialBackend) -> Option<(String, String)> {
    match backend {
        SerialBackend::File => Some((
            format!("{}", t!("vmsettings.port-target-file")),
            t!("vmsettings.port-file-placeholder").to_string(),
        )),
        SerialBackend::UnixSocket => Some((
            format!("{}", t!("vmsettings.port-target-socket")),
            t!("vmsettings.port-socket-placeholder").to_string(),
        )),
        SerialBackend::Tcp => Some((
            format!("{}", t!("vmsettings.port-target-tcp")),
            t!("vmsettings.port-tcp-placeholder").to_string(),
        )),
        SerialBackend::Pty | SerialBackend::Null => None,
    }
}

fn render_port_editor(app: &mut LibreVmmApp, ui: &mut egui::Ui, kind: PortKind) {
    if app.editing_config().is_none() {
        return;
    }

    let rows = read_port_rows(app, kind);
    let cap = kind.cap();

    if rows.is_empty() {
        ui.label(
            egui::RichText::new(t!("vmsettings.port-empty").to_string())
                .size(theme::FontSize::SMALL)
                .color(AppColors::TEXT_DIM),
        );
        ui.add_space(theme::Spacing::XS);
    }

    let mut remove_idx: Option<usize> = None;

    for (i, row) in rows.iter().enumerate() {
        egui::Frame::none()
            .fill(AppColors::BG_CARD.linear_multiply(0.7))
            .rounding(theme::ThemeRounding::BUTTON)
            .inner_margin(10.0)
            .stroke(egui::Stroke::new(0.5, egui::Color32::from_rgb(55, 60, 75)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} {}", kind.label_prefix(), i))
                            .size(theme::FontSize::BODY)
                            .strong()
                            .color(AppColors::PRIMARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(format!("\u{2716} {}", t!("vmsettings.remove-nic")))
                            .clicked()
                        {
                            remove_idx = Some(i);
                        }
                    });
                });

                egui::Grid::new(format!("{}_grid_{}", kind.id_salt(), i))
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        // Backend
                        ui.label(t!("vmsettings.port-backend").to_string());
                        let current = row.backend;
                        egui::ComboBox::from_id_salt(format!("{}_backend_{}", kind.id_salt(), i))
                            .selected_text(current.to_string())
                            .show_ui(ui, |ui| {
                                for &b in SerialBackend::ALL {
                                    if ui.selectable_label(b == current, b.to_string()).clicked() {
                                        write_port_backend(app, kind, i, b);
                                    }
                                }
                            });
                        ui.end_row();

                        // Target (depends on backend)
                        if let Some((label, placeholder)) = port_target_placeholder(row.backend) {
                            ui.label(label);
                            let mut target = row.target.clone();
                            let changed = ui
                                .add(
                                    egui::TextEdit::singleline(&mut target)
                                        .hint_text(placeholder)
                                        .desired_width(TEXT_FIELD_WIDTH),
                                )
                                .changed();
                            if changed {
                                write_port_target(app, kind, i, target);
                            }
                            ui.end_row();
                        }
                    });
            });

        ui.add_space(theme::Spacing::XS);
    }

    if let Some(i) = remove_idx {
        remove_port(app, kind, i);
    }

    ui.add_space(theme::Spacing::XS);
    let at_cap = rows.len() >= cap;
    let add_btn =
        egui::Button::new(egui::RichText::new(t!("vmsettings.add-port").to_string()).size(12.0))
            .rounding(theme::ThemeRounding::BUTTON_SMALL);
    if ui
        .add_enabled(!at_cap, add_btn)
        .on_hover_text(if at_cap {
            t!("vmsettings.port-at-cap", n = cap).to_string()
        } else {
            String::new()
        })
        .clicked()
    {
        add_port(app, kind);
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────

fn os_type_label(ot: &OsType) -> &'static str {
    match ot {
        OsType::Linux => "Linux",
        OsType::Windows => "Windows",
        OsType::MacOS => "macOS",
        OsType::FreeBSD => "FreeBSD",
        OsType::Other => "Other",
    }
}

fn network_label(nm: &NetworkMode) -> String {
    match nm {
        NetworkMode::Nat => t!("vmsettings.net-nat").to_string(),
        NetworkMode::Bridged => t!("vmsettings.net-bridged").to_string(),
        NetworkMode::HostOnly => t!("vmsettings.net-host-only").to_string(),
        NetworkMode::LanSegment(name) => format!("LAN: {}", name),
        NetworkMode::None => t!("vmsettings.net-none").to_string(),
    }
}
