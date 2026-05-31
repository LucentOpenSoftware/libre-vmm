//! ISO Library browser — scan and pick ISOs from the library.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::AppColors;
use eframe::egui;
use rust_i18n::t;
use vmm_core::iso_library::{self, IsoEntry};

/// State for the ISO library picker.
pub struct IsoLibraryState {
    pub isos: Vec<IsoEntry>,
    pub search: String,
    pub last_scan: std::time::Instant,
}

impl Default for IsoLibraryState {
    fn default() -> Self {
        Self {
            isos: Vec::new(),
            search: String::new(),
            last_scan: std::time::Instant::now() - std::time::Duration::from_secs(60),
        }
    }
}

impl IsoLibraryState {
    /// Refresh the ISO list.
    pub fn refresh(&mut self) {
        self.isos = iso_library::scan_isos();
        self.last_scan = std::time::Instant::now();
    }

    /// Ensure ISOs are loaded (lazy init).
    pub fn ensure_loaded(&mut self) {
        if self.isos.is_empty() || self.last_scan.elapsed() > std::time::Duration::from_secs(30) {
            self.refresh();
        }
    }

    /// Get filtered ISOs based on search query.
    pub fn filtered(&self) -> Vec<&IsoEntry> {
        if self.search.is_empty() {
            self.isos.iter().collect()
        } else {
            let query = self.search.to_lowercase();
            self.isos
                .iter()
                .filter(|iso| iso.name.to_lowercase().contains(&query))
                .collect()
        }
    }
}

/// Render the ISO library picker as a popup/inline panel.
/// Returns Some(path) if an ISO was selected.
pub fn render_picker(state: &mut IsoLibraryState, ui: &mut egui::Ui) -> Option<String> {
    state.ensure_loaded();

    let mut selected_path: Option<String> = None;

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("ISO Library")
                .size(14.0)
                .strong()
                .color(AppColors::TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("\u{1F504} Refresh").clicked() {
                state.refresh();
            }
        });
    });

    ui.add_space(theme::Spacing::XS);
    ui.label(
        egui::RichText::new(format!(
            "Scanning: {} and ~/Downloads",
            iso_library::iso_library_dir()
        ))
        .size(theme::FontSize::SMALL)
        .color(AppColors::TEXT_DIM),
    );
    ui.add_space(theme::Spacing::XS);

    // Search
    ui.add(
        egui::TextEdit::singleline(&mut state.search)
            .hint_text("Search ISOs...")
            .desired_width(280.0),
    );
    ui.add_space(theme::Spacing::XS);

    let filtered = state.filtered();

    if filtered.is_empty() {
        ui.label(
            egui::RichText::new("No ISO images found. Place .iso files in the library directory.")
                .size(12.0)
                .color(AppColors::TEXT_DIM),
        );
    } else {
        ui.label(
            egui::RichText::new(format!(
                "{} ISO{} found",
                filtered.len(),
                if filtered.len() == 1 { "" } else { "s" }
            ))
            .size(theme::FontSize::SMALL)
            .color(AppColors::TEXT_DIM),
        );
        ui.add_space(2.0);

        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                for iso in &filtered {
                    let response = egui::Frame::none()
                        .fill(AppColors::BG_CARD)
                        .rounding(theme::ThemeRounding::BUTTON_SMALL)
                        .inner_margin(theme::Spacing::SM)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("\u{1F4BF}").size(14.0));
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(&iso.name)
                                            .size(12.0)
                                            .strong()
                                            .color(AppColors::TEXT),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} | {}",
                                            iso.size_display(),
                                            iso.path
                                        ))
                                        .size(10.0)
                                        .color(AppColors::TEXT_DIM),
                                    );
                                });
                            });
                        })
                        .response;

                    if response.interact(egui::Sense::click()).clicked() {
                        selected_path = Some(iso.path.clone());
                    }

                    ui.add_space(2.0);
                }
            });
    }

    selected_path
}

/// Render ISO Library as a standalone window dialog.
pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    if !app.show_iso_picker() {
        return;
    }

    // Check Esc to close the dialog (CWE-1216 — UX accessibility)
    if theme::escape_pressed(ctx) {
        app.set_show_iso_picker(false);
        return;
    }

    let mut open = true;
    egui::Window::new(t!("menu.file.iso-library"))
        .open(&mut open)
        .resizable(true)
        .default_width(500.0)
        .default_height(400.0)
        .show(ctx, |ui| {
            let state = app.iso_library_state_mut();
            if let Some(path) = render_picker(state, ui) {
                // If an ISO was selected, set it as the boot ISO for the current VM
                if let Some(ref mut config) = app.editing_config_mut() {
                    config.iso_path = Some(path);
                }
                app.set_show_iso_picker(false);
            }
        });

    if !open {
        app.set_show_iso_picker(false);
    }
}
