//! Import Wizard — step-by-step import of VMs from other hypervisors.
//!
//! Supports .vmx (VMware), .vbox (VirtualBox), .xml (libvirt), and .conf files.
//! Multi-step process: select file → review config → disk handling → confirm & import.

use crate::app::LibreVmmApp;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;

// ---------------------------------------------------------------------------
// Types (will migrate to vmm_core::import once that module exists)
// ---------------------------------------------------------------------------

/// How to handle source disks during import.
#[derive(Debug, Clone, PartialEq)]
pub enum DiskAction {
    /// Create a symlink to the original disk (no copy).
    Symlink,
    /// Copy the disk file into Libre VMM storage.
    Copy,
    /// Move the disk file into Libre VMM storage.
    Move,
    /// Convert the disk to qcow2 format.
    Convert,
}

impl Default for DiskAction {
    fn default() -> Self {
        Self::Copy
    }
}

impl DiskAction {
    /// Map the wizard's local DiskAction to the core import DiskAction.
    pub fn to_core(&self) -> vmm_core::import::DiskAction {
        match self {
            DiskAction::Symlink => vmm_core::import::DiskAction::Symlink,
            DiskAction::Copy => vmm_core::import::DiskAction::Copy,
            DiskAction::Move => vmm_core::import::DiskAction::Move,
            DiskAction::Convert => vmm_core::import::DiskAction::Convert,
        }
    }
}

/// Detected source format from file extension.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceFormat {
    Vmx,
    Vbox,
    LibvirtXml,
    Conf,
    Unknown,
}

impl SourceFormat {
    pub fn from_extension(path: &str) -> Self {
        let lower = path.to_lowercase();
        if lower.ends_with(".vmx") {
            Self::Vmx
        } else if lower.ends_with(".vbox") {
            Self::Vbox
        } else if lower.ends_with(".xml") {
            Self::LibvirtXml
        } else if lower.ends_with(".conf") {
            Self::Conf
        } else {
            Self::Unknown
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Vmx => "VMware (.vmx)",
            Self::Vbox => "VirtualBox (.vbox)",
            Self::LibvirtXml => "Libvirt XML (.xml)",
            Self::Conf => "Configuration (.conf)",
            Self::Unknown => "Unknown",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Vmx => "\u{1F4BB}",        // laptop
            Self::Vbox => "\u{1F4E6}",       // package
            Self::LibvirtXml => "\u{1F4C4}", // document
            Self::Conf => "\u{2699}",        // gear
            Self::Unknown => "\u{2753}",     // question mark
        }
    }
}

/// Parsed disk information from the source config.
#[derive(Debug, Clone)]
pub struct ImportedDisk {
    pub path: String,
    pub size_bytes: u64,
    pub format: String,
}

/// Parsed VM details from the source config.
#[derive(Debug, Clone)]
pub struct ImportedVm {
    pub name: String,
    pub os_type: String,
    pub cpus: u32,
    pub memory_mib: u64,
    pub disks: Vec<ImportedDisk>,
    pub network: String,
    pub display: String,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
}

/// Full state of the import wizard.
#[derive(Debug, Clone)]
pub struct ImportWizardState {
    pub step: u8,
    pub file_path: String,
    pub source_format: SourceFormat,
    pub imported: Option<ImportedVm>,
    /// The raw vmm_core ImportedVm preserved for execute_import. Populated alongside `imported`.
    pub parsed_vm: Option<vmm_core::import::ImportedVm>,
    pub disk_action: DiskAction,
    pub vm_name_override: String,
    pub error: Option<String>,
    pub importing: bool,
    pub success: bool,
}

impl Default for ImportWizardState {
    fn default() -> Self {
        Self {
            step: 0,
            file_path: String::new(),
            source_format: SourceFormat::Unknown,
            imported: None,
            parsed_vm: None,
            disk_action: DiskAction::default(),
            vm_name_override: String::new(),
            error: None,
            importing: false,
            success: false,
        }
    }
}

impl ImportWizardState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Render the import wizard as a floating dialog window (egui::Window).
pub fn render_import_wizard(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let state = app.import_wizard_state();
    if state.is_none() {
        return;
    }

    let mut open = true;
    egui::Window::new(t!("import-wizard.title").to_string())
        .open(&mut open)
        .resizable(true)
        .default_width(600.0)
        .min_width(500.0)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            render_content(app, ui);
        });

    if !open {
        app.close_import_wizard();
    }
}

fn render_content(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let step = app
        .import_wizard_state()
        .as_ref()
        .map(|s| s.step)
        .unwrap_or(0);

    // Progress indicator
    render_step_indicator(ui, step);

    ui.add_space(Spacing::MD);
    ui.separator();
    ui.add_space(Spacing::MD);

    match step {
        0 => render_step_select_source(app, ui),
        1 => render_step_review(app, ui),
        2 => render_step_disk_handling(app, ui),
        3 => render_step_confirm(app, ui),
        _ => {},
    }
}

fn render_step_indicator(ui: &mut egui::Ui, current_step: u8) {
    ui.horizontal(|ui| {
        let steps = [
            (t!("import-wizard.select-file").to_string(), 0u8),
            (t!("import-wizard.review").to_string(), 1),
            (t!("import-wizard.disk-handling").to_string(), 2),
            (t!("import-wizard.confirm").to_string(), 3),
        ];
        for (i, (label, step)) in steps.iter().enumerate() {
            let active = current_step == *step;
            let color = if active {
                AppColors::PRIMARY
            } else {
                AppColors::TEXT_DIM
            };
            ui.label(egui::RichText::new(label.as_str()).color(color).strong());
            if i < steps.len() - 1 {
                ui.label(egui::RichText::new("\u{25B8}").color(AppColors::TEXT_DIM));
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Step 0: Select Source
// ---------------------------------------------------------------------------

fn render_step_select_source(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.heading(t!("import-wizard.select-file").to_string());
    ui.label(
        egui::RichText::new(t!("import-wizard.select-file-sub").to_string())
            .color(AppColors::TEXT_DIM),
    );
    ui.add_space(Spacing::MD);

    let Some(state) = app.import_wizard_state_mut().as_mut() else {
        return;
    };

    // File path + browse
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.file_path)
                .hint_text(t!("import-wizard.path-hint").to_string())
                .desired_width(350.0),
        );
        if ui.button(t!("import-wizard.browse").to_string()).clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("VM Configs", &["xml", "vmx", "vbox", "conf"])
                .pick_file()
            {
                let path_str = path.display().to_string();
                state.file_path = path_str.clone();
                state.source_format = SourceFormat::from_extension(&path_str);
                // Auto-parse: store both the raw vmm_core ImportedVm (for execute_import)
                // and the local UI display struct (for the review/disk steps).
                let (display, parsed) = parse_stub(&path_str, &state.source_format);
                state.imported = Some(display);
                state.parsed_vm = parsed;
                if let Some(ref vm) = state.imported {
                    state.vm_name_override = vm.name.clone();
                }
            }
        }
    });

    // Show detected format if a file was selected
    if !state.file_path.is_empty() {
        ui.add_space(Spacing::SM);
        egui::Frame::none()
            .fill(AppColors::BG_CARD)
            .rounding(ThemeRounding::CARD)
            .inner_margin(Spacing::MD)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(state.source_format.icon()).size(FontSize::PAGE_TITLE),
                    );
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(t!("import-wizard.detected-format").to_string())
                                .color(AppColors::TEXT_DIM)
                                .size(FontSize::LABEL),
                        );
                        ui.label(
                            egui::RichText::new(state.source_format.label())
                                .color(AppColors::TEXT)
                                .size(FontSize::SUBHEADING)
                                .strong(),
                        );
                    });
                });
            });
    }

    // Error display
    if let Some(ref err) = state.error {
        ui.add_space(Spacing::SM);
        ui.label(
            egui::RichText::new(err.as_str())
                .color(AppColors::DANGER)
                .size(FontSize::BODY),
        );
    }

    ui.add_space(Spacing::LG);

    // Navigation — collect actions to avoid borrow conflicts
    let can_proceed = !state.file_path.is_empty()
        && state.source_format != SourceFormat::Unknown
        && state.imported.is_some();

    let mut go_cancel = false;
    let mut go_next = false;
    ui.horizontal(|ui| {
        if ui.button(t!("wizard.cancel").to_string()).clicked() {
            go_cancel = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let next = egui::Button::new(
                egui::RichText::new(t!("wizard.next").to_string()).color(egui::Color32::WHITE),
            )
            .fill(if can_proceed {
                AppColors::PRIMARY
            } else {
                AppColors::MUTED
            });
            if ui.add_enabled(can_proceed, next).clicked() {
                go_next = true;
            }
        });
    });

    // Apply deferred actions
    if go_next {
        state.error = None;
        state.step = 1;
    }
    let _ = state;
    if go_cancel {
        app.close_import_wizard();
    }
}

// ---------------------------------------------------------------------------
// Step 1: Review Configuration
// ---------------------------------------------------------------------------

fn render_step_review(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.heading(t!("import-wizard.review").to_string());
    ui.add_space(Spacing::SM);

    let Some(state) = app.import_wizard_state_mut().as_mut() else {
        return;
    };
    let imported = state.imported.clone().unwrap_or_else(|| ImportedVm {
        name: String::new(),
        os_type: String::new(),
        cpus: 0,
        memory_mib: 0,
        disks: Vec::new(),
        network: String::new(),
        display: String::new(),
        warnings: Vec::new(),
        notes: Vec::new(),
    });

    // Editable VM name
    egui::Grid::new("import_review_grid")
        .num_columns(2)
        .spacing([Spacing::XL, Spacing::SM])
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(t!("import-wizard.vm-name").to_string())
                    .color(AppColors::TEXT_DIM),
            );
            ui.add(egui::TextEdit::singleline(&mut state.vm_name_override).desired_width(300.0));
            ui.end_row();
        });

    ui.add_space(Spacing::SM);

    // Read-only details
    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::CARD)
        .inner_margin(Spacing::LG)
        .show(ui, |ui| {
            egui::Grid::new("import_details_grid")
                .num_columns(2)
                .spacing([Spacing::XL, Spacing::SM])
                .show(ui, |ui| {
                    review_row(
                        ui,
                        &t!("import-wizard.lbl-os").to_string(),
                        &imported.os_type,
                    );
                    review_row(
                        ui,
                        &t!("import-wizard.lbl-cpus").to_string(),
                        &imported.cpus.to_string(),
                    );
                    review_row(
                        ui,
                        &t!("import-wizard.lbl-memory").to_string(),
                        &format!("{} MiB", imported.memory_mib),
                    );
                    review_row(
                        ui,
                        &t!("import-wizard.lbl-network").to_string(),
                        &imported.network,
                    );
                    review_row(
                        ui,
                        &t!("import-wizard.lbl-display").to_string(),
                        &imported.display,
                    );

                    // Disks
                    for (i, disk) in imported.disks.iter().enumerate() {
                        let size_label = if disk.size_bytes > 0 {
                            format!(
                                "{} ({:.1} GiB)",
                                disk.format,
                                disk.size_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                            )
                        } else {
                            disk.format.clone()
                        };
                        review_row(
                            ui,
                            &format!("{} {}", t!("import-wizard.lbl-disk").to_string(), i + 1),
                            &size_label,
                        );
                    }
                });
        });

    // Warnings
    if !imported.warnings.is_empty() {
        ui.add_space(Spacing::SM);
        egui::Frame::none()
            .fill(AppColors::WARNING.linear_multiply(0.15))
            .rounding(ThemeRounding::CARD)
            .inner_margin(Spacing::MD)
            .stroke(egui::Stroke::new(0.5, AppColors::WARNING))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(t!("import-wizard.warnings").to_string())
                        .color(AppColors::WARNING)
                        .strong(),
                );
                for w in &imported.warnings {
                    ui.label(
                        egui::RichText::new(format!("\u{26A0} {}", w))
                            .color(AppColors::WARNING)
                            .size(FontSize::BODY),
                    );
                }
            });
    }

    // Notes
    if !imported.notes.is_empty() {
        ui.add_space(Spacing::SM);
        egui::Frame::none()
            .fill(AppColors::PRIMARY.linear_multiply(0.1))
            .rounding(ThemeRounding::CARD)
            .inner_margin(Spacing::MD)
            .stroke(egui::Stroke::new(0.5, AppColors::PRIMARY))
            .show(ui, |ui| {
                for n in &imported.notes {
                    ui.label(
                        egui::RichText::new(format!("\u{2139} {}", n))
                            .color(AppColors::TEXT_DIM)
                            .size(FontSize::BODY),
                    );
                }
            });
    }

    ui.add_space(Spacing::LG);

    // Navigation — collect actions to avoid borrow conflicts
    let mut go_back = false;
    let mut go_cancel = false;
    let mut go_next = false;
    ui.horizontal(|ui| {
        if ui.button(t!("wizard.back").to_string()).clicked() {
            go_back = true;
        }
        if ui.button(t!("wizard.cancel").to_string()).clicked() {
            go_cancel = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let next = egui::Button::new(
                egui::RichText::new(t!("wizard.next").to_string()).color(egui::Color32::WHITE),
            )
            .fill(AppColors::PRIMARY);
            if ui.add(next).clicked() {
                go_next = true;
            }
        });
    });

    // Apply deferred actions
    if go_back {
        state.step = 0;
    }
    if go_next {
        state.step = 2;
    }
    let _ = state;
    if go_cancel {
        app.close_import_wizard();
    }
}

// ---------------------------------------------------------------------------
// Step 2: Disk Handling
// ---------------------------------------------------------------------------

fn render_step_disk_handling(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.heading(t!("import-wizard.disk-handling").to_string());
    ui.label(
        egui::RichText::new(t!("import-wizard.disk-handling-sub").to_string())
            .color(AppColors::TEXT_DIM),
    );
    ui.add_space(Spacing::MD);

    let Some(state) = app.import_wizard_state_mut().as_mut() else {
        return;
    };

    // Show disk list
    if let Some(ref imported) = state.imported {
        for (i, disk) in imported.disks.iter().enumerate() {
            let size_gib = disk.size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            egui::Frame::none()
                .fill(AppColors::BG_CARD)
                .rounding(ThemeRounding::CARD)
                .inner_margin(Spacing::MD)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {} — {} ({:.1} GiB)",
                            t!("import-wizard.lbl-disk").to_string(),
                            i + 1,
                            disk.path,
                            size_gib,
                        ))
                        .color(AppColors::TEXT)
                        .size(FontSize::BODY),
                    );
                });
            ui.add_space(Spacing::XS);
        }
    }

    ui.add_space(Spacing::MD);

    // Radio buttons for disk action
    ui.label(
        egui::RichText::new(t!("import-wizard.disk-action-label").to_string())
            .color(AppColors::TEXT)
            .strong(),
    );
    ui.add_space(Spacing::SM);

    ui.radio_value(
        &mut state.disk_action,
        DiskAction::Symlink,
        t!("import-wizard.symlink").to_string(),
    );
    ui.radio_value(
        &mut state.disk_action,
        DiskAction::Copy,
        t!("import-wizard.copy").to_string(),
    );
    ui.radio_value(
        &mut state.disk_action,
        DiskAction::Move,
        t!("import-wizard.move").to_string(),
    );

    // Convert option with format label
    let convert_label = if let Some(ref imported) = state.imported {
        if let Some(disk) = imported.disks.first() {
            let src_fmt = &disk.format;
            format!(
                "{} ({} \u{2192} qcow2)",
                t!("import-wizard.convert").to_string(),
                src_fmt,
            )
        } else {
            t!("import-wizard.convert").to_string()
        }
    } else {
        t!("import-wizard.convert").to_string()
    };
    ui.radio_value(&mut state.disk_action, DiskAction::Convert, convert_label);

    ui.add_space(Spacing::LG);

    // Navigation — collect actions to avoid borrow conflicts
    let mut go_back = false;
    let mut go_cancel = false;
    let mut go_next = false;
    ui.horizontal(|ui| {
        if ui.button(t!("wizard.back").to_string()).clicked() {
            go_back = true;
        }
        if ui.button(t!("wizard.cancel").to_string()).clicked() {
            go_cancel = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let next = egui::Button::new(
                egui::RichText::new(t!("wizard.next").to_string()).color(egui::Color32::WHITE),
            )
            .fill(AppColors::PRIMARY);
            if ui.add(next).clicked() {
                go_next = true;
            }
        });
    });

    // Apply deferred actions
    if go_back {
        state.step = 1;
    }
    if go_next {
        state.step = 3;
    }
    // go_cancel must be applied after dropping state borrow
    let _ = state;
    if go_cancel {
        app.close_import_wizard();
    }
}

// ---------------------------------------------------------------------------
// Step 3: Confirm & Import
// ---------------------------------------------------------------------------

fn render_step_confirm(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.heading(t!("import-wizard.confirm").to_string());
    ui.add_space(Spacing::SM);

    // Read state values we need without holding a mutable borrow across UI calls
    let is_success = app
        .import_wizard_state()
        .as_ref()
        .map_or(false, |s| s.success);
    let is_importing = app
        .import_wizard_state()
        .as_ref()
        .map_or(false, |s| s.importing);

    // Success screen
    if is_success {
        egui::Frame::none()
            .fill(AppColors::SUCCESS.linear_multiply(0.15))
            .rounding(ThemeRounding::CARD)
            .inner_margin(Spacing::LG)
            .stroke(egui::Stroke::new(0.5, AppColors::SUCCESS))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(t!("import-wizard.success").to_string())
                        .color(AppColors::SUCCESS)
                        .size(FontSize::SUBHEADING)
                        .strong(),
                );
            });
        ui.add_space(Spacing::LG);
        if ui.button(t!("import-wizard.close").to_string()).clicked() {
            app.close_import_wizard();
        }
        return;
    }

    // Importing spinner
    if is_importing {
        let err_msg = app
            .import_wizard_state()
            .as_ref()
            .and_then(|s| s.error.clone());
        ui.add_space(Spacing::LG);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                egui::RichText::new(t!("import-wizard.importing").to_string())
                    .color(AppColors::PRIMARY)
                    .size(FontSize::BODY),
            );
        });
        if let Some(err) = err_msg {
            ui.add_space(Spacing::SM);
            ui.label(
                egui::RichText::new(err)
                    .color(AppColors::DANGER)
                    .size(FontSize::BODY),
            );
        }
        return;
    }

    // Snapshot values for the summary display
    let Some(state) = app.import_wizard_state().as_ref() else {
        return;
    };
    let vm_name = state.vm_name_override.clone();
    let format_label = state.source_format.label().to_string();
    let file_path = state.file_path.clone();
    let disk_action_label = match state.disk_action {
        DiskAction::Symlink => t!("import-wizard.symlink").to_string(),
        DiskAction::Copy => t!("import-wizard.copy").to_string(),
        DiskAction::Move => t!("import-wizard.move").to_string(),
        DiskAction::Convert => t!("import-wizard.convert").to_string(),
    };
    let imported = state.imported.clone().unwrap_or_else(|| ImportedVm {
        name: String::new(),
        os_type: String::new(),
        cpus: 0,
        memory_mib: 0,
        disks: Vec::new(),
        network: String::new(),
        display: String::new(),
        warnings: Vec::new(),
        notes: Vec::new(),
    });
    let err_msg = state.error.clone();

    // Summary card
    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::CARD)
        .inner_margin(Spacing::LG)
        .show(ui, |ui| {
            egui::Grid::new("import_confirm_grid")
                .num_columns(2)
                .spacing([Spacing::XL, Spacing::SM])
                .show(ui, |ui| {
                    review_row(ui, &t!("import-wizard.vm-name").to_string(), &vm_name);
                    review_row(
                        ui,
                        &t!("import-wizard.detected-format").to_string(),
                        &format_label,
                    );
                    review_row(
                        ui,
                        &t!("import-wizard.lbl-os").to_string(),
                        &imported.os_type,
                    );
                    review_row(
                        ui,
                        &t!("import-wizard.lbl-cpus").to_string(),
                        &imported.cpus.to_string(),
                    );
                    review_row(
                        ui,
                        &t!("import-wizard.lbl-memory").to_string(),
                        &format!("{} MiB", imported.memory_mib),
                    );
                    review_row(
                        ui,
                        &t!("import-wizard.disk-handling").to_string(),
                        &disk_action_label,
                    );
                    review_row(ui, &t!("import-wizard.source-file").to_string(), &file_path);
                });
        });

    // Error display
    if let Some(err) = err_msg {
        ui.add_space(Spacing::SM);
        ui.label(
            egui::RichText::new(err)
                .color(AppColors::DANGER)
                .size(FontSize::BODY),
        );
    }

    ui.add_space(Spacing::LG);

    // Navigation — collect actions, apply after
    let mut go_back = false;
    let mut go_cancel = false;
    let mut do_import = false;
    ui.horizontal(|ui| {
        if ui.button(t!("wizard.back").to_string()).clicked() {
            go_back = true;
        }
        if ui.button(t!("wizard.cancel").to_string()).clicked() {
            go_cancel = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let import_btn = egui::Button::new(
                egui::RichText::new(t!("import-wizard.import-btn").to_string())
                    .color(egui::Color32::WHITE)
                    .size(FontSize::SUBHEADING + 1.0),
            )
            .fill(AppColors::SUCCESS)
            .min_size(egui::vec2(200.0, 36.0))
            .rounding(ThemeRounding::CARD);

            if ui.add(import_btn).clicked() {
                do_import = true;
            }
        });
    });

    // Apply deferred actions
    if go_back {
        if let Some(ref mut ws) = app.import_wizard_state_mut() {
            ws.step = 2;
        }
    }
    if do_import {
        // Validate and perform import
        let name = vm_name;
        if name.trim().is_empty() {
            if let Some(ref mut ws) = app.import_wizard_state_mut() {
                ws.error = Some(t!("import-wizard.error-no-name").to_string());
            }
        } else if let Some(err) = vmm_core::config::validate_vm_name(&name) {
            if let Some(ref mut ws) = app.import_wizard_state_mut() {
                ws.error = Some(t!("import.invalid-name", err = err).to_string());
            }
        } else {
            // Snapshot what action_execute_import needs without holding a borrow on app.
            let exec_args: Option<(vmm_core::import::ImportedVm, vmm_core::import::DiskAction)> =
                app.import_wizard_state().as_ref().and_then(|ws| {
                    ws.parsed_vm
                        .as_ref()
                        .map(|p| (p.clone(), ws.disk_action.to_core()))
                });
            match exec_args {
                Some((parsed, core_action)) => {
                    match app.action_execute_import(&parsed, core_action, &name) {
                        Ok(()) => {
                            if let Some(ref mut ws) = app.import_wizard_state_mut() {
                                ws.error = None;
                                ws.success = true;
                            }
                        },
                        Err(e) => {
                            if let Some(ref mut ws) = app.import_wizard_state_mut() {
                                ws.error = Some(e);
                            }
                        },
                    }
                },
                None => {
                    if let Some(ref mut ws) = app.import_wizard_state_mut() {
                        ws.error = Some(t!("import-wizard.error-not-parsed").to_string());
                    }
                },
            }
        }
    }
    if go_cancel {
        app.close_import_wizard();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn review_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(
        egui::RichText::new(label)
            .color(AppColors::TEXT_DIM)
            .size(FontSize::BODY),
    );
    ui.label(
        egui::RichText::new(value)
            .color(AppColors::TEXT)
            .size(FontSize::BODY)
            .strong(),
    );
    ui.end_row();
}

/// Parse a VM config file using the real vmm_core::import backend.
/// Returns the local UI display struct and (on success) the raw vmm_core
/// ImportedVm so the import step can pass it to execute_import.
/// Falls back to a stub if parsing fails (shows the error as a warning).
fn parse_stub(
    path: &str,
    _format: &SourceFormat,
) -> (ImportedVm, Option<vmm_core::import::ImportedVm>) {
    match vmm_core::import::parse_import(std::path::Path::new(path)) {
        Ok(real) => {
            // Convert vmm_core::import::ImportedVm → our local ImportedVm
            let os_label = format!("{:?}", real.os_type);
            let disks = real
                .disk_paths
                .iter()
                .map(|p| {
                    let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                    ImportedDisk {
                        path: p.display().to_string(),
                        size_bytes: size,
                        format: real.disk_format.clone(),
                    }
                })
                .collect();
            let display = ImportedVm {
                name: real.name.clone(),
                os_type: os_label,
                cpus: real.vcpus,
                memory_mib: real.memory_mib,
                disks,
                network: format!("{:?}", real.network_mode),
                display: format!("{}", real.display_protocol),
                warnings: real.warnings.clone(),
                notes: real.notes.clone(),
            };
            (display, Some(real))
        },
        Err(e) => {
            // Fallback: use file stem as name, show parse error
            let file_stem = std::path::Path::new(path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Imported VM".to_string());
            let display = ImportedVm {
                name: file_stem,
                os_type: "Unknown".to_string(),
                cpus: 2,
                memory_mib: 2048,
                disks: Vec::new(),
                network: "NAT".to_string(),
                display: "VNC".to_string(),
                warnings: vec![format!("Parse error: {}", e)],
                notes: Vec::new(),
            };
            (display, None)
        },
    }
}
