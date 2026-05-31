//! Backup & Restore dialog — create, browse, and restore VM backups.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;
use vmm_core::backup::{self, BackupCompression, BackupMeta, BackupOptions};
use vmm_core::config::VmConfigIo;

/// State for the backup/restore dialog.
pub struct BackupRestoreState {
    pub open: bool,
    /// Current tab: 0 = Create, 1 = History/Restore
    pub tab: usize,
    /// Backup list (cached).
    pub backups: Vec<BackupMeta>,
    /// Options for new backup.
    pub compression: BackupCompression,
    pub note: String,
    pub compute_checksum: bool,
    /// Retention policy.
    pub retention_count: u32,
    /// Status message after operation.
    pub status: Option<(String, bool)>, // (message, is_error)
    /// Whether a backup is in progress.
    pub in_progress: bool,
}

impl Default for BackupRestoreState {
    fn default() -> Self {
        Self {
            open: false,
            tab: 0,
            backups: Vec::new(),
            compression: BackupCompression::Qcow2Compressed,
            note: String::new(),
            compute_checksum: true,
            retention_count: 5,
            status: None,
            in_progress: false,
        }
    }
}

impl BackupRestoreState {
    pub fn open(&mut self) {
        self.open = true;
        self.status = None;
    }

    pub fn refresh_backups(&mut self, vm_name: &str) {
        self.backups = backup::list_backups(vm_name, None);
    }
}

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let open = app.backup_restore_state().map(|s| s.open).unwrap_or(false);
    if !open {
        return;
    }

    let vm_name = match app.selected_vm() {
        Some(n) => n.to_string(),
        None => {
            app.backup_restore_state_mut().open = false;
            return;
        },
    };

    // Check Esc to close the dialog (CWE-1216 — UX accessibility)
    if theme::escape_pressed(ctx) {
        app.backup_restore_state_mut().open = false;
        return;
    }

    let mut should_close = false;

    egui::Window::new(t!("backup.title"))
        .default_width(550.0)
        .default_height(450.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            // Tab bar
            ui.horizontal(|ui| {
                let tab = app.backup_restore_state().map(|s| s.tab).unwrap_or(0);
                if ui
                    .selectable_label(tab == 0, t!("backup.tab-create"))
                    .clicked()
                {
                    app.backup_restore_state_mut().tab = 0;
                }
                if ui
                    .selectable_label(tab == 1, t!("backup.tab-history"))
                    .clicked()
                {
                    app.backup_restore_state_mut().tab = 1;
                    app.backup_restore_state_mut().refresh_backups(&vm_name);
                }
            });
            ui.separator();
            ui.add_space(Spacing::SM);

            let tab = app.backup_restore_state().map(|s| s.tab).unwrap_or(0);

            match tab {
                0 => render_create_tab(app, ui, &vm_name),
                1 => render_history_tab(app, ui, &vm_name),
                _ => {},
            }

            // Status message
            if let Some((msg, is_err)) = app.backup_restore_state().and_then(|s| s.status.clone()) {
                ui.add_space(Spacing::SM);
                ui.separator();
                ui.add_space(theme::Spacing::XS);
                let color = if is_err {
                    AppColors::DANGER
                } else {
                    AppColors::SUCCESS
                };
                ui.label(egui::RichText::new(&msg).size(12.0).color(color));
            }

            // Close button
            ui.add_space(Spacing::SM);
            ui.separator();
            ui.add_space(theme::Spacing::XS);
            ui.horizontal(|ui| {
                if ui.button(t!("backup.close")).clicked() {
                    should_close = true;
                }
            });
        });

    if should_close {
        app.backup_restore_state_mut().open = false;
    }
}

fn render_create_tab(app: &mut LibreVmmApp, ui: &mut egui::Ui, vm_name: &str) {
    ui.label(
        egui::RichText::new(t!("backup.create-heading", name = vm_name))
            .size(FontSize::SUBHEADING)
            .strong()
            .color(AppColors::TEXT),
    );
    ui.add_space(Spacing::SM);

    egui::Grid::new("backup_create_grid")
        .num_columns(2)
        .spacing([Spacing::MD, 10.0])
        .show(ui, |ui| {
            // Compression
            ui.label(t!("backup.compression"));
            let current = app
                .backup_restore_state()
                .map(|s| s.compression)
                .unwrap_or(BackupCompression::Qcow2Compressed);
            egui::ComboBox::from_id_salt("backup_compression")
                .selected_text(current.to_string())
                .show_ui(ui, |ui| {
                    for &comp in BackupCompression::ALL {
                        if ui
                            .selectable_label(current == comp, comp.to_string())
                            .clicked()
                        {
                            app.backup_restore_state_mut().compression = comp;
                        }
                    }
                });
            ui.end_row();

            // Note
            ui.label(t!("backup.note"));
            let note = &mut app.backup_restore_state_mut().note;
            ui.add(
                egui::TextEdit::singleline(note)
                    .hint_text(t!("backup.note-hint"))
                    .desired_width(300.0),
            );
            ui.end_row();

            // Checksum
            ui.label(t!("backup.checksum"));
            let chk = &mut app.backup_restore_state_mut().compute_checksum;
            ui.checkbox(chk, t!("backup.checksum-desc"));
            ui.end_row();

            // Retention
            ui.label(t!("backup.retention"));
            let ret = &mut app.backup_restore_state_mut().retention_count;
            let mut r = *ret as i32;
            ui.add(egui::Slider::new(&mut r, 1..=20).text(t!("backup.backups-to-keep")));
            *ret = r.max(1) as u32;
            ui.end_row();
        });

    ui.add_space(Spacing::MD);

    // Existing backups count
    let existing = backup::list_backups(vm_name, None);
    let total_size = backup::total_backup_size(vm_name, None);
    if !existing.is_empty() {
        ui.label(
            egui::RichText::new(format!(
                "{} backup(s) | {}",
                existing.len(),
                backup::format_size(total_size),
            ))
            .size(theme::FontSize::SMALL)
            .color(AppColors::TEXT_DIM),
        );
        ui.add_space(theme::Spacing::XS);
    }

    // Create button
    let in_progress = app
        .backup_restore_state()
        .map(|s| s.in_progress)
        .unwrap_or(false);
    ui.add_enabled_ui(!in_progress, |ui| {
        let btn = egui::Button::new(
            egui::RichText::new(t!("backup.create-now")).color(egui::Color32::WHITE),
        )
        .fill(AppColors::SUCCESS)
        .min_size(egui::vec2(180.0, 32.0));

        if ui.add(btn).clicked() {
            let state = app.backup_restore_state_mut();
            state.in_progress = true;
            let opts = BackupOptions {
                compression: state.compression,
                note: state.note.clone(),
                compute_checksum: state.compute_checksum,
                ..Default::default()
            };
            let retention = state.retention_count as usize;

            // Run backup
            if let Some(config) = app.editing_config().cloned().or_else(|| {
                // Try loading from disk if no editing config
                let config_path = format!(
                    "{}/{}.json",
                    vmm_core::config::VmConfig::config_dir(),
                    vm_name
                );
                std::fs::read_to_string(&config_path)
                    .ok()
                    .and_then(|s| serde_json::from_str::<vmm_core::config::VmConfig>(&s).ok())
            }) {
                match backup::create_backup(&config, &opts) {
                    Ok(meta) => {
                        // Apply retention
                        let _ = backup::apply_retention(vm_name, retention, None);
                        app.backup_restore_state_mut().status = Some((
                            format!(
                                "Backup created: {} ({})",
                                meta.backup_id,
                                backup::format_size(meta.disk_size_bytes)
                            ),
                            false,
                        ));
                        app.backup_restore_state_mut().note.clear();
                    },
                    Err(e) => {
                        app.backup_restore_state_mut().status =
                            Some((format!("Backup failed: {}", e), true));
                    },
                }
            } else {
                app.backup_restore_state_mut().status =
                    Some(("No VM config found".to_string(), true));
            }
            app.backup_restore_state_mut().in_progress = false;
        }
    });
}

fn render_history_tab(app: &mut LibreVmmApp, ui: &mut egui::Ui, vm_name: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t!("backup.history-heading"))
                .size(FontSize::SUBHEADING)
                .strong()
                .color(AppColors::TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(t!("backup.refresh")).clicked() {
                app.backup_restore_state_mut().refresh_backups(vm_name);
            }
        });
    });
    ui.add_space(Spacing::SM);

    let backups = app
        .backup_restore_state()
        .map(|s| s.backups.clone())
        .unwrap_or_default();

    if backups.is_empty() {
        ui.label(
            egui::RichText::new(t!("backup.no-backups"))
                .size(12.0)
                .color(AppColors::TEXT_DIM),
        );
        return;
    }

    egui::ScrollArea::vertical()
        .max_height(300.0)
        .show(ui, |ui| {
            for backup in &backups {
                egui::Frame::none()
                    .fill(AppColors::BG_CARD)
                    .rounding(ThemeRounding::CARD)
                    .stroke(egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE))
                    .inner_margin(10.0)
                    .outer_margin(egui::Margin {
                        left: 0.0,
                        right: 0.0,
                        top: 0.0,
                        bottom: 6.0,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new(&backup.backup_id)
                                        .size(theme::FontSize::BODY)
                                        .strong()
                                        .color(AppColors::TEXT),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} | {}",
                                        &backup.created_at,
                                        backup::format_size(backup.disk_size_bytes),
                                    ))
                                    .size(theme::FontSize::SMALL)
                                    .color(AppColors::TEXT_DIM),
                                );
                                if !backup.note.is_empty() {
                                    ui.label(
                                        egui::RichText::new(format!("\u{1F4DD} {}", &backup.note))
                                            .size(theme::FontSize::SMALL)
                                            .color(AppColors::MUTED),
                                    );
                                }
                                if let Some(ref chk) = backup.disk_checksum {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "SHA256: {}...",
                                            &chk[..16.min(chk.len())]
                                        ))
                                        .size(10.0)
                                        .color(AppColors::MUTED),
                                    );
                                }
                            });

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Delete button
                                    if ui
                                        .small_button("\u{1F5D1}")
                                        .on_hover_text(t!("backup.delete"))
                                        .clicked()
                                    {
                                        let _ =
                                            backup::delete_backup(vm_name, &backup.backup_id, None);
                                        app.backup_restore_state_mut().refresh_backups(vm_name);
                                        app.backup_restore_state_mut().status =
                                            Some((format!("Deleted: {}", backup.backup_id), false));
                                    }

                                    // Restore button
                                    let restore_btn = egui::Button::new(
                                        egui::RichText::new(t!("backup.restore"))
                                            .size(theme::FontSize::SMALL),
                                    )
                                    .rounding(theme::ThemeRounding::BUTTON_SMALL);
                                    if ui.add(restore_btn).clicked() {
                                        match backup::restore_backup(
                                            vm_name,
                                            &backup.backup_id,
                                            None,
                                        ) {
                                            Ok(_config) => {
                                                app.backup_restore_state_mut().status = Some((
                                                    format!("Restored from: {}", backup.backup_id),
                                                    false,
                                                ));
                                                app.action_refresh();
                                            },
                                            Err(e) => {
                                                app.backup_restore_state_mut().status =
                                                    Some((format!("Restore failed: {}", e), true));
                                            },
                                        }
                                    }
                                },
                            );
                        });
                    });
            }
        });
}
