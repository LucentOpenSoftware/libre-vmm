//! Template Manager — create, browse, import/export VM templates.
//! Integrates into the wizard (step 1) and as a dialog from the sidebar context menu.

use crate::app::LibreVmmApp;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding, GRID_SPACING};
use eframe::egui;
use vmm_core::template_library::VmTemplate;

/// State for the template manager dialog.
#[derive(Debug, Default)]
pub struct TemplateManagerState {
    pub visible: bool,
    pub templates: Vec<VmTemplate>,
    pub save_name: String,
    pub save_description: String,
    pub import_path: String,
    pub export_path: String,
    pub error: Option<String>,
    pub mode: TemplateMode,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum TemplateMode {
    #[default]
    Browse,
    SaveNew,
    Import,
}

impl TemplateManagerState {
    pub fn open(&mut self) {
        self.visible = true;
        self.error = None;
        self.mode = TemplateMode::Browse;
        self.refresh();
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.save_name.clear();
        self.save_description.clear();
        self.import_path.clear();
        self.export_path.clear();
        self.error = None;
    }

    pub fn refresh(&mut self) {
        self.templates = VmTemplate::list_all().unwrap_or_default();
    }

    pub fn open_save(&mut self, vm_name: &str) {
        self.visible = true;
        self.mode = TemplateMode::SaveNew;
        self.save_name = format!("{} Template", vm_name);
        self.save_description.clear();
        self.error = None;
        self.refresh();
    }
}

/// Render the template manager as a floating window.
pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let visible = app.template_manager_state().visible;
    if !visible {
        return;
    }

    let mut open = true;
    egui::Window::new("Template Library")
        .open(&mut open)
        .resizable(true)
        .default_size([500.0, 400.0])
        .show(ctx, |ui| {
            render_inner(app, ui);
        });

    if !open {
        app.template_manager_state_mut().close();
    }
}

fn render_inner(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let mode = app.template_manager_state().mode.clone();

    // Mode selector
    ui.horizontal(|ui| {
        if ui
            .selectable_label(mode == TemplateMode::Browse, "Browse")
            .clicked()
        {
            app.template_manager_state_mut().mode = TemplateMode::Browse;
        }
        if ui
            .selectable_label(mode == TemplateMode::SaveNew, "Save from VM")
            .clicked()
        {
            app.template_manager_state_mut().mode = TemplateMode::SaveNew;
        }
        if ui
            .selectable_label(mode == TemplateMode::Import, "Import")
            .clicked()
        {
            app.template_manager_state_mut().mode = TemplateMode::Import;
        }
    });

    ui.separator();

    // Error display
    if let Some(error) = app.template_manager_state().error.clone() {
        ui.label(
            egui::RichText::new(&error)
                .color(AppColors::DANGER)
                .size(FontSize::LABEL),
        );
        ui.add_space(Spacing::XS);
    }

    match mode {
        TemplateMode::Browse => render_browse(app, ui),
        TemplateMode::SaveNew => render_save(app, ui),
        TemplateMode::Import => render_import(app, ui),
    }
}

fn render_browse(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let templates = app.template_manager_state().templates.clone();

    if templates.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(
                egui::RichText::new("No custom templates yet")
                    .size(FontSize::SUBHEADING)
                    .color(AppColors::TEXT_DIM),
            );
            ui.add_space(Spacing::SM);
            ui.label(
                egui::RichText::new("Save a VM as a template or import one to get started.")
                    .size(FontSize::LABEL)
                    .color(AppColors::MUTED),
            );
        });
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut delete_idx: Option<usize> = None;
        let mut export_idx: Option<usize> = None;

        for (i, template) in templates.iter().enumerate() {
            egui::Frame::none()
                .fill(AppColors::BG_CARD)
                .rounding(ThemeRounding::BUTTON)
                .inner_margin(Spacing::MD)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(&template.name)
                                    .size(FontSize::SUBHEADING)
                                    .strong()
                                    .color(AppColors::TEXT),
                            );
                            ui.label(
                                egui::RichText::new(template.summary())
                                    .size(FontSize::SMALL)
                                    .color(AppColors::TEXT_DIM),
                            );
                            if !template.description.is_empty() {
                                ui.label(
                                    egui::RichText::new(&template.description)
                                        .size(FontSize::SMALL)
                                        .color(AppColors::MUTED),
                                );
                            }
                            ui.label(
                                egui::RichText::new(format!("Created: {}", template.created_at))
                                    .size(FontSize::TINY)
                                    .color(AppColors::MUTED),
                            );
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button(
                                    egui::RichText::new("Delete").color(AppColors::DANGER),
                                )
                                .clicked()
                            {
                                delete_idx = Some(i);
                            }
                            if ui.small_button("Export").clicked() {
                                export_idx = Some(i);
                            }
                        });
                    });
                });
            ui.add_space(Spacing::XS);
        }

        // Handle actions
        if let Some(idx) = delete_idx {
            if let Some(template) = templates.get(idx) {
                if let Err(e) = template.delete() {
                    app.template_manager_state_mut().error = Some(format!("Delete failed: {}", e));
                }
                app.template_manager_state_mut().refresh();
            }
        }
        if let Some(idx) = export_idx {
            if let Some(template) = templates.get(idx) {
                let template_dir = VmTemplate::template_dir();
                let export_path = format!(
                    "{}/{}.vmtemplate.json",
                    template_dir,
                    template.name.replace(' ', "_").to_lowercase()
                );
                match template.export_to(&export_path) {
                    Ok(()) => {
                        app.template_manager_state_mut().error = None;
                        // Use error field for success message too
                        app.template_manager_state_mut().export_path = export_path;
                    },
                    Err(e) => {
                        app.template_manager_state_mut().error =
                            Some(format!("Export failed: {}", e));
                    },
                }
            }
        }
    });

    // Show export success
    let export_path = app.template_manager_state().export_path.clone();
    if !export_path.is_empty() {
        ui.add_space(Spacing::XS);
        ui.label(
            egui::RichText::new(format!("Exported to: {}", export_path))
                .size(FontSize::SMALL)
                .color(AppColors::RUNNING),
        );
    }
}

fn render_save(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let has_config = app.selected_vm_config().is_some();

    if !has_config {
        ui.label(
            egui::RichText::new("Select a VM first to save it as a template.")
                .color(AppColors::TEXT_DIM),
        );
        return;
    }

    ui.label("Save the selected VM's settings as a reusable template.");
    ui.add_space(Spacing::SM);

    egui::Grid::new("save_template_grid")
        .num_columns(2)
        .spacing(GRID_SPACING)
        .show(ui, |ui| {
            ui.label("Template Name:");
            let name = app.template_manager_state_mut().save_name.clone();
            let mut name_edit = name;
            ui.add(egui::TextEdit::singleline(&mut name_edit).desired_width(300.0));
            app.template_manager_state_mut().save_name = name_edit;
            ui.end_row();

            ui.label("Description:");
            let desc = app.template_manager_state_mut().save_description.clone();
            let mut desc_edit = desc;
            ui.add(
                egui::TextEdit::multiline(&mut desc_edit)
                    .desired_width(300.0)
                    .desired_rows(3),
            );
            app.template_manager_state_mut().save_description = desc_edit;
            ui.end_row();
        });

    ui.add_space(Spacing::MD);

    let save_name = app.template_manager_state().save_name.clone();
    let save_desc = app.template_manager_state().save_description.clone();
    let can_save = !save_name.is_empty();

    if ui
        .add_enabled(
            can_save,
            egui::Button::new(egui::RichText::new("Save Template").color(egui::Color32::WHITE))
                .fill(AppColors::PRIMARY)
                .rounding(ThemeRounding::BUTTON),
        )
        .clicked()
    {
        if let Some(config) = app.selected_vm_config().cloned() {
            let template = VmTemplate::from_config(&config, &save_name, &save_desc);
            match template.save() {
                Ok(()) => {
                    app.template_manager_state_mut().error = None;
                    app.template_manager_state_mut().mode = TemplateMode::Browse;
                    app.template_manager_state_mut().refresh();
                },
                Err(e) => {
                    app.template_manager_state_mut().error = Some(format!("Save failed: {}", e));
                },
            }
        }
    }
}

fn render_import(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.label("Import a template from a .vmtemplate.json file.");
    ui.add_space(Spacing::SM);

    ui.horizontal(|ui| {
        ui.label("File path:");
        let path = app.template_manager_state_mut().import_path.clone();
        let mut path_edit = path;
        ui.add(
            egui::TextEdit::singleline(&mut path_edit)
                .desired_width(300.0)
                .hint_text("/path/to/template.vmtemplate.json"),
        );
        app.template_manager_state_mut().import_path = path_edit;
    });

    ui.add_space(Spacing::SM);

    let import_path = app.template_manager_state().import_path.clone();
    let can_import = !import_path.is_empty();

    if ui
        .add_enabled(
            can_import,
            egui::Button::new(egui::RichText::new("Import Template").color(egui::Color32::WHITE))
                .fill(AppColors::PRIMARY)
                .rounding(ThemeRounding::BUTTON),
        )
        .clicked()
    {
        match VmTemplate::import_from(&import_path) {
            Ok(_template) => {
                app.template_manager_state_mut().error = None;
                app.template_manager_state_mut().mode = TemplateMode::Browse;
                app.template_manager_state_mut().refresh();
                // Log success via the main app
            },
            Err(e) => {
                app.template_manager_state_mut().error = Some(format!("Import failed: {}", e));
            },
        }
    }
}
