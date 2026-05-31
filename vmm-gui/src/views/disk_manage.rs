//! Disk Space Management GUI — visual disk usage analysis, compact, and repair.
//!
//! TODO(Wave 11.9): Add an "Add Disk" button that opens a file picker (rfd),
//! prompts for target device suffix (auto-suggest next free vdb/vdc/...) and
//! bus type (default Virtio), then calls `vmm_core::disk_manage::hotplug_disk`
//! and shows the result in the event log. Also add a per-disk "Remove" action
//! that calls `vmm_core::disk_manage::hotunplug_disk`. Core function landed
//! in this wave; UI deferred to keep the batch focused.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding, GRID_SPACING};
use eframe::egui;
use rust_i18n::t;
use vmm_core::domain::VmState;

/// Disk management UI state.
pub struct DiskManageState {
    pub open: bool,
    pub virtual_size: u64,
    pub actual_size: u64,
    pub wasted: u64,
    pub format: String,
    pub compacting: bool,
    pub compact_result: Option<Result<u64, String>>,
    pub checking: bool,
    pub check_result: Option<Result<String, String>>,
    /// New disk size for resize (in GiB).
    pub resize_new_gib: u64,
    /// Result of last resize attempt.
    pub resize_result: Option<Result<(), String>>,
}

impl Default for DiskManageState {
    fn default() -> Self {
        Self {
            open: false,
            virtual_size: 0,
            actual_size: 0,
            wasted: 0,
            format: String::new(),
            compacting: false,
            compact_result: None,
            checking: false,
            check_result: None,
            resize_new_gib: 0,
            resize_result: None,
        }
    }
}

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    if !app.disk_manage_state().open {
        return;
    }

    // Check Esc to close the dialog (CWE-1216 — UX accessibility)
    if theme::escape_pressed(ctx) {
        app.disk_manage_state_mut().open = false;
        return;
    }

    let Some(vm_name) = app.selected_vm().map(|s| s.to_string()) else {
        app.disk_manage_state_mut().open = false;
        return;
    };

    let vm_state = app.selected_vm_state().unwrap_or(VmState::Off);
    let is_running = matches!(vm_state, VmState::Running | VmState::Paused);
    let disk_path = app
        .selected_vm_config()
        .map(|c| c.disk_path.clone())
        .unwrap_or_default();

    let mut open = true;
    egui::Window::new(t!("diskmgmt.title"))
        .id(egui::Id::new("disk_manage_dialog"))
        .default_size([500.0, 400.0])
        .resizable(true)
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.heading(
                        egui::RichText::new(t!("diskmgmt.disk-label", name = &vm_name).to_string())
                            .size(FontSize::HEADING)
                            .color(AppColors::TEXT),
                    );
                    ui.add_space(Spacing::XS);

                    // Disk path
                    ui.label(
                        egui::RichText::new(&disk_path)
                            .size(FontSize::SMALL)
                            .color(AppColors::TEXT_DIM),
                    );
                    ui.add_space(Spacing::SM);

                    // Usage visualization
                    let state = app.disk_manage_state();
                    if state.virtual_size > 0 {
                        render_usage_bar(ui, state.actual_size, state.virtual_size);
                        ui.add_space(Spacing::SM);

                        egui::Grid::new("disk_info")
                            .num_columns(2)
                            .spacing(GRID_SPACING)
                            .show(ui, |ui| {
                                info_row(ui, &t!("diskmgmt.format"), &state.format);
                                info_row(
                                    ui,
                                    &t!("diskmgmt.virtual-size"),
                                    &format_bytes(state.virtual_size),
                                );
                                info_row(
                                    ui,
                                    &t!("diskmgmt.actual-size"),
                                    &format_bytes(state.actual_size),
                                );
                                info_row(
                                    ui,
                                    &t!("diskmgmt.reclaimable"),
                                    &format_bytes(state.wasted),
                                );
                                let usage_pct = if state.virtual_size > 0 {
                                    (state.actual_size as f64 / state.virtual_size as f64 * 100.0)
                                        as u32
                                } else {
                                    0
                                };
                                info_row(ui, &t!("diskmgmt.usage"), &format!("{}%", usage_pct));
                            });
                    } else {
                        ui.label(
                            egui::RichText::new(t!("diskmgmt.analyze-hint").to_string())
                                .size(FontSize::LABEL)
                                .color(AppColors::TEXT_DIM),
                        );
                    }

                    ui.add_space(Spacing::MD);
                    ui.separator();
                    ui.add_space(Spacing::SM);

                    // Actions
                    ui.horizontal(|ui| {
                        // Analyze
                        if ui.button(t!("diskmgmt.analyze")).clicked() {
                            app.action_analyze_disk(&disk_path);
                        }

                        // Compact
                        let compact_enabled = !is_running && !app.disk_manage_state().compacting;
                        if ui
                            .add_enabled(
                                compact_enabled,
                                egui::Button::new(t!("diskmgmt.compact").to_string()),
                            )
                            .on_hover_text(if is_running {
                                t!("diskmgmt.compact-hover-running").to_string()
                            } else {
                                t!("diskmgmt.compact-hover-off").to_string()
                            })
                            .clicked()
                        {
                            app.action_compact_disk(&disk_path);
                        }

                        // Check & Repair
                        let check_enabled = !is_running && !app.disk_manage_state().checking;
                        if ui
                            .add_enabled(
                                check_enabled,
                                egui::Button::new(t!("diskmgmt.check").to_string()),
                            )
                            .on_hover_text(if is_running {
                                t!("diskmgmt.check-hover-running").to_string()
                            } else {
                                t!("diskmgmt.check-hover-off").to_string()
                            })
                            .clicked()
                        {
                            app.action_check_disk(&disk_path);
                        }
                    });

                    // ── Resize section ──
                    ui.add_space(Spacing::MD);
                    ui.separator();
                    ui.add_space(Spacing::SM);
                    ui.label(
                        egui::RichText::new(t!("diskmgmt.resize-title").to_string())
                            .size(FontSize::SUBHEADING)
                            .strong()
                            .color(AppColors::TEXT),
                    );
                    ui.add_space(Spacing::XS);

                    let current_gib = app
                        .selected_vm_config()
                        .map(|c| c.disk_size_gib)
                        .unwrap_or(0);

                    // Initialize resize field on first open
                    if app.disk_manage_state().resize_new_gib == 0 {
                        app.disk_manage_state_mut().resize_new_gib = current_gib;
                    }

                    ui.horizontal(|ui| {
                        ui.label(t!("diskmgmt.current-size").to_string());
                        ui.label(
                            egui::RichText::new(format!("{} GiB", current_gib))
                                .color(AppColors::TEXT_DIM),
                        );
                    });

                    ui.horizontal(|ui| {
                        ui.label(t!("diskmgmt.new-size").to_string());
                        let mut new_gib =
                            app.disk_manage_state().resize_new_gib.max(current_gib) as i32;
                        let max_gib = (current_gib * 4).max(2048).min(65536) as i32;
                        if ui
                            .add(
                                egui::Slider::new(&mut new_gib, current_gib as i32..=max_gib)
                                    .text("GiB"),
                            )
                            .changed()
                        {
                            app.disk_manage_state_mut().resize_new_gib = new_gib as u64;
                        }
                    });

                    let resize_enabled =
                        !is_running && app.disk_manage_state().resize_new_gib > current_gib;

                    ui.horizontal(|ui| {
                        let btn = egui::Button::new(
                            egui::RichText::new(t!("diskmgmt.resize-btn").to_string())
                                .color(egui::Color32::WHITE),
                        )
                        .fill(if resize_enabled {
                            AppColors::PRIMARY
                        } else {
                            AppColors::MUTED
                        })
                        .rounding(ThemeRounding::BUTTON);

                        if ui
                            .add_enabled(resize_enabled, btn)
                            .on_hover_text(if is_running {
                                t!("diskmgmt.resize-hover-running").to_string()
                            } else if app.disk_manage_state().resize_new_gib <= current_gib {
                                t!("diskmgmt.resize-hover-same").to_string()
                            } else {
                                t!("diskmgmt.resize-hover-off").to_string()
                            })
                            .clicked()
                        {
                            let new_gib = app.disk_manage_state().resize_new_gib;
                            let dp = disk_path.clone();
                            match vmm_core::disk::resize_disk(&dp, new_gib) {
                                Ok(()) => {
                                    app.disk_manage_state_mut().resize_result = Some(Ok(()));
                                    // Update the config's disk_size_gib
                                    if let Some(ref mut config) = app.editing_config_mut() {
                                        config.disk_size_gib = new_gib;
                                    }
                                    // Status update handled via resize_result
                                    app.action_analyze_disk(&dp);
                                },
                                Err(e) => {
                                    app.disk_manage_state_mut().resize_result =
                                        Some(Err(e.to_string()));
                                },
                            }
                        }

                        if let Some(ref result) = app.disk_manage_state().resize_result {
                            match result {
                                Ok(()) => {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "\u{2714} {}",
                                            t!("diskmgmt.resize-ok")
                                        ))
                                        .color(AppColors::SUCCESS),
                                    );
                                },
                                Err(e) => {
                                    ui.label(
                                        egui::RichText::new(format!("\u{2718} {}", e))
                                            .color(AppColors::DANGER),
                                    );
                                },
                            }
                        }
                    });

                    ui.add_space(Spacing::SM);
                    ui.label(
                        egui::RichText::new(t!("diskmgmt.resize-hint").to_string())
                            .size(FontSize::CAPTION)
                            .color(AppColors::TEXT_DIM),
                    );

                    ui.add_space(Spacing::SM);
                    ui.separator();
                    ui.add_space(Spacing::SM);

                    // Progress/results
                    let dms = app.disk_manage_state();
                    if dms.compacting {
                        ui.add_space(Spacing::SM);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(
                                egui::RichText::new(t!("diskmgmt.compacting").to_string())
                                    .size(FontSize::LABEL)
                                    .color(AppColors::WARNING),
                            );
                        });
                    }

                    if let Some(ref result) = dms.compact_result {
                        ui.add_space(Spacing::SM);
                        match result {
                            Ok(saved) => {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "\u{2714} {}",
                                        t!("diskmgmt.compact-ok", saved = format_bytes(*saved))
                                    ))
                                    .size(FontSize::BODY)
                                    .color(AppColors::SUCCESS),
                                );
                            },
                            Err(e) => {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "\u{2718} {}",
                                        t!("diskmgmt.compact-fail", err = e.as_str())
                                    ))
                                    .size(FontSize::BODY)
                                    .color(AppColors::DANGER),
                                );
                            },
                        }
                    }

                    if dms.checking {
                        ui.add_space(Spacing::SM);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(t!("diskmgmt.checking"));
                        });
                    }

                    if let Some(ref result) = dms.check_result {
                        ui.add_space(Spacing::SM);
                        match result {
                            Ok(msg) => {
                                ui.label(
                                    egui::RichText::new(format!("\u{2714} {}", msg))
                                        .size(FontSize::LABEL)
                                        .color(AppColors::SUCCESS),
                                );
                            },
                            Err(e) => {
                                ui.label(
                                    egui::RichText::new(format!("\u{2718} {}", e))
                                        .size(FontSize::LABEL)
                                        .color(AppColors::DANGER),
                                );
                            },
                        }
                    }
                }); // ScrollArea
        });

    if !open {
        app.disk_manage_state_mut().open = false;
    }
}

fn render_usage_bar(ui: &mut egui::Ui, actual: u64, virtual_size: u64) {
    let fraction = if virtual_size > 0 {
        (actual as f32 / virtual_size as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let bar_color = if fraction > 0.9 {
        AppColors::DANGER
    } else if fraction > 0.7 {
        AppColors::WARNING
    } else {
        AppColors::PRIMARY
    };

    let desired_width = ui.available_width().min(400.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(desired_width, 24.0), egui::Sense::hover());

    // Background
    ui.painter()
        .rect_filled(rect, ThemeRounding::BUTTON_SMALL, AppColors::BG_DARK);

    // Filled portion
    let filled_rect =
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * fraction, rect.height()));
    ui.painter()
        .rect_filled(filled_rect, ThemeRounding::BUTTON_SMALL, bar_color);

    // Label
    let label = format!(
        "{} / {} ({:.0}%)",
        format_bytes(actual),
        format_bytes(virtual_size),
        fraction * 100.0,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(FontSize::SMALL),
        egui::Color32::WHITE,
    );
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(
        egui::RichText::new(label)
            .size(FontSize::LABEL)
            .color(AppColors::TEXT_DIM),
    );
    ui.label(
        egui::RichText::new(value)
            .size(FontSize::LABEL)
            .color(AppColors::TEXT),
    );
    ui.end_row();
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GiB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.0} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}
