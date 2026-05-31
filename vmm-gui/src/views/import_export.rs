//! Import/Export dialogs for OVA/OVF support.

use crate::app::LibreVmmApp;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;

/// State for import/export operations.
/// Export format options.
#[derive(Debug, Clone, PartialEq)]
pub enum ExportFormat {
    Ova,
    Vmdk,
    Vhd,
    Raw,
    Qcow2,
}

impl ExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Ova => "ova",
            Self::Vmdk => "vmdk",
            Self::Vhd => "vhd",
            Self::Raw => "img",
            Self::Qcow2 => "qcow2",
        }
    }

    pub fn qemu_format(&self) -> &'static str {
        match self {
            Self::Ova => "vmdk",
            Self::Vmdk => "vmdk",
            Self::Vhd => "vpc",
            Self::Raw => "raw",
            Self::Qcow2 => "qcow2",
        }
    }
}

impl ExportFormat {
    pub fn display_name(&self) -> String {
        match self {
            Self::Ova => t!("export.fmt-ova").to_string(),
            Self::Vmdk => t!("export.fmt-vmdk").to_string(),
            Self::Vhd => t!("export.fmt-vhd").to_string(),
            Self::Raw => t!("export.fmt-raw").to_string(),
            Self::Qcow2 => t!("export.fmt-qcow2").to_string(),
        }
    }
}

impl std::fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

impl Default for ExportFormat {
    fn default() -> Self {
        Self::Ova
    }
}

pub struct ImportExportState {
    pub show_import: bool,
    pub show_export: bool,
    pub import_path: String,
    pub import_name: String,
    pub export_vm_name: String,
    pub export_path: String,
    pub export_format: ExportFormat,
    pub in_progress: bool,
    pub error: Option<String>,
}

impl Default for ImportExportState {
    fn default() -> Self {
        Self {
            show_import: false,
            show_export: false,
            import_path: String::new(),
            import_name: String::new(),
            export_vm_name: String::new(),
            export_path: String::new(),
            export_format: ExportFormat::default(),
            in_progress: false,
            error: None,
        }
    }
}

impl ImportExportState {
    pub fn open_import(&mut self) {
        self.show_import = true;
        self.show_export = false;
        self.import_path.clear();
        self.import_name.clear();
        self.in_progress = false;
        self.error = None;
    }

    pub fn open_export(&mut self, vm_name: &str) {
        self.show_export = true;
        self.show_import = false;
        self.export_vm_name = vm_name.to_string();
        self.export_path = format!(
            "{}/{}.ova",
            std::env::var("HOME").unwrap_or_default(),
            sanitize_filename(vm_name)
        );
        self.in_progress = false;
        self.error = None;
    }

    pub fn close(&mut self) {
        self.show_import = false;
        self.show_export = false;
    }
}

/// Render import dialog.
pub fn render_import(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let should_show = app.import_export_state().show_import;
    if !should_show {
        return;
    }

    let mut open = true;
    egui::Window::new(t!("import.title"))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .min_width(450.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(t!("import.subtitle").to_string())
                    .size(FontSize::BODY)
                    .color(AppColors::TEXT_DIM),
            );
            ui.add_space(Spacing::SM);

            // File path
            ui.horizontal(|ui| {
                ui.label(t!("import.ova-file"));
                let state = app.import_export_state_mut();
                ui.add(
                    egui::TextEdit::singleline(&mut state.import_path)
                        .desired_width(280.0)
                        .hint_text("/path/to/vm.ova"),
                );
                if ui.button(t!("import.browse")).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("OVA Files", &["ova", "ovf"])
                        .pick_file()
                    {
                        state.import_path = path.display().to_string();
                        // Auto-generate name from filename
                        if state.import_name.is_empty() {
                            if let Some(stem) = path.file_stem() {
                                state.import_name = stem.to_string_lossy().to_string();
                            }
                        }
                    }
                }
            });

            ui.add_space(Spacing::XS);

            // VM name override
            ui.horizontal(|ui| {
                ui.label(t!("import.vm-name"));
                let state = app.import_export_state_mut();
                ui.add(
                    egui::TextEdit::singleline(&mut state.import_name)
                        .desired_width(280.0)
                        .hint_text(t!("import.name-hint").to_string()),
                );
            });

            ui.add_space(Spacing::SM);

            // Error
            if let Some(error) = app.import_export_state().error.clone() {
                ui.label(
                    egui::RichText::new(format!("{}: {}", t!("common.error"), error))
                        .color(AppColors::DANGER)
                        .size(FontSize::LABEL),
                );
                ui.add_space(Spacing::XS);
            }

            // Actions
            let in_progress = app.import_export_state().in_progress;
            let can_import = !app.import_export_state().import_path.is_empty() && !in_progress;

            ui.horizontal(|ui| {
                let import_btn = egui::Button::new(
                    egui::RichText::new(if in_progress {
                        t!("import.importing").to_string()
                    } else {
                        t!("import.import").to_string()
                    })
                    .color(egui::Color32::WHITE),
                )
                .fill(if can_import {
                    AppColors::SUCCESS
                } else {
                    AppColors::MUTED
                })
                .rounding(ThemeRounding::BUTTON)
                .min_size(egui::vec2(100.0, 30.0));

                if ui.add_enabled(can_import, import_btn).clicked() {
                    let import_path = app.import_export_state().import_path.clone();
                    let import_name = app.import_export_state().import_name.clone();
                    // SECURITY (CWE-22, CWE-73): Validate import file path
                    if let Err(e) = validate_file_path(&import_path, false) {
                        app.import_export_state_mut().error =
                            Some(t!("import.invalid-path", err = e).to_string());
                    } else if let Err(e) = validate_import_name(&import_name) {
                        // SECURITY (CWE-20): Validate VM name override
                        app.import_export_state_mut().error =
                            Some(t!("import.invalid-name", err = e).to_string());
                    } else {
                        app.action_import_ova();
                    }
                }

                if ui.button(t!("import.cancel")).clicked() {
                    app.import_export_state_mut().close();
                }
            });
        });

    if !open {
        app.import_export_state_mut().close();
    }
}

/// Render export dialog.
pub fn render_export(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let should_show = app.import_export_state().show_export;
    if !should_show {
        return;
    }

    let mut open = true;
    egui::Window::new(t!("export.title"))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .min_width(450.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            let vm_name = app.import_export_state().export_vm_name.clone();

            ui.label(
                egui::RichText::new(t!("export.subtitle", name = &vm_name).to_string())
                    .size(FontSize::BODY)
                    .color(AppColors::TEXT_DIM),
            );
            ui.add_space(Spacing::SM);

            // Format selector
            ui.horizontal(|ui| {
                ui.label(t!("export.format"));
                let current = app.import_export_state().export_format.clone();
                egui::ComboBox::from_id_salt("export_format")
                    .selected_text(current.to_string())
                    .show_ui(ui, |ui| {
                        for fmt in [
                            ExportFormat::Ova,
                            ExportFormat::Vmdk,
                            ExportFormat::Vhd,
                            ExportFormat::Raw,
                            ExportFormat::Qcow2,
                        ] {
                            if ui
                                .selectable_label(current == fmt, fmt.to_string())
                                .clicked()
                            {
                                app.import_export_state_mut().export_format = fmt;
                            }
                        }
                    });
            });
            ui.add_space(Spacing::XS);

            // Output path
            let _ext = app.import_export_state().export_format.extension();
            ui.horizontal(|ui| {
                ui.label(t!("export.save-to"));
                let state = app.import_export_state_mut();
                ui.add(egui::TextEdit::singleline(&mut state.export_path).desired_width(280.0));
                if ui.button(t!("export.browse")).clicked() {
                    let ext_str = state.export_format.extension();
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Export Files", &[ext_str])
                        .set_file_name(&format!(
                            "{}.{}",
                            sanitize_filename(&state.export_vm_name),
                            ext_str
                        ))
                        .save_file()
                    {
                        state.export_path = path.display().to_string();
                    }
                }
            });

            ui.add_space(Spacing::SM);

            let format_hint = match app.import_export_state().export_format {
                ExportFormat::Ova => t!("export.hint-ova").to_string(),
                ExportFormat::Vmdk => t!("export.hint-vmdk").to_string(),
                ExportFormat::Vhd => t!("export.hint-vhd").to_string(),
                ExportFormat::Raw => t!("export.hint-raw").to_string(),
                ExportFormat::Qcow2 => t!("export.hint-qcow2").to_string(),
            };
            ui.label(
                egui::RichText::new(format_hint)
                    .size(FontSize::SMALL)
                    .color(AppColors::TEXT_DIM),
            );

            ui.add_space(Spacing::SM);

            // Error
            if let Some(error) = app.import_export_state().error.clone() {
                ui.label(
                    egui::RichText::new(format!("{}: {}", t!("common.error"), error))
                        .color(AppColors::DANGER)
                        .size(FontSize::LABEL),
                );
                ui.add_space(Spacing::XS);
            }

            // Actions
            let in_progress = app.import_export_state().in_progress;
            let can_export = !app.import_export_state().export_path.is_empty() && !in_progress;

            ui.horizontal(|ui| {
                let export_btn = egui::Button::new(
                    egui::RichText::new(if in_progress {
                        t!("export.exporting").to_string()
                    } else {
                        t!("export.export").to_string()
                    })
                    .color(egui::Color32::WHITE),
                )
                .fill(if can_export {
                    AppColors::SUCCESS
                } else {
                    AppColors::MUTED
                })
                .rounding(ThemeRounding::BUTTON)
                .min_size(egui::vec2(100.0, 30.0));

                if ui.add_enabled(can_export, export_btn).clicked() {
                    let export_path = app.import_export_state().export_path.clone();
                    // SECURITY (CWE-22, CWE-73): Validate export path for traversal
                    // and block writes to sensitive system directories
                    if let Err(e) = validate_file_path(&export_path, true) {
                        app.import_export_state_mut().error =
                            Some(t!("export.invalid-path", err = e).to_string());
                    } else {
                        app.action_export_ova();
                    }
                }

                if ui.button(t!("export.cancel")).clicked() {
                    app.import_export_state_mut().close();
                }
            });
        });

    if !open {
        app.import_export_state_mut().close();
    }
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Validate a file path for import/export operations.
/// Prevents path traversal and writing to sensitive system locations (CWE-22, CWE-73).
fn validate_file_path(path: &str, is_export: bool) -> Result<(), String> {
    if path.is_empty() {
        return Err("Path cannot be empty".to_string());
    }
    // CWE-20: Reject null bytes which truncate paths in C libraries
    if path.contains('\0') {
        return Err("Path contains null bytes".to_string());
    }
    // CWE-22: Reject path traversal
    if path.contains("..") {
        return Err("Path must not contain '..' (path traversal)".to_string());
    }
    // CWE-73: Require absolute path
    if !path.starts_with('/') {
        return Err("Path must be absolute (start with '/')".to_string());
    }
    // CWE-78: Reject shell metacharacters
    if path.chars().any(|c| ";|&`$\\\"'<>!{}".contains(c)) {
        return Err("Path contains unsafe shell characters".to_string());
    }
    if is_export {
        // CWE-22: Block writes to sensitive system directories
        let blocked_prefixes = [
            "/etc/", "/boot/", "/usr/", "/bin/", "/sbin/", "/lib/", "/proc/", "/sys/", "/dev/",
            "/run/",
        ];
        for prefix in &blocked_prefixes {
            if path.starts_with(prefix) {
                return Err(format!("Cannot export to system directory: {}", prefix));
            }
        }
    }
    Ok(())
}

/// Validate a VM name for import (same rules as create).
fn validate_import_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Ok(()); // empty is fine, uses name from OVA
    }
    if let Some(err) = vmm_core::config::validate_vm_name(name) {
        return Err(err.to_string());
    }
    Ok(())
}
