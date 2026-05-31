//! Guest File Manager — browse guest filesystem via QGA.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::AppColors;
use eframe::egui;
use rust_i18n::t;

/// State for the guest file manager.
pub struct GuestFileManagerState {
    pub open: bool,
    pub vm_name: String,
    pub current_path: String,
    pub entries: Vec<vmm_core::guest_file_manager::GuestFileEntry>,
    pub selected_entry: Option<usize>,
    pub history: Vec<String>,
    pub history_idx: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub preview_content: Option<String>,
    pub path_input: String,
    pub new_dir_name: String,
    pub show_new_dir: bool,
}

impl Default for GuestFileManagerState {
    fn default() -> Self {
        Self {
            open: false,
            vm_name: String::new(),
            current_path: "/".to_string(),
            entries: Vec::new(),
            selected_entry: None,
            history: vec!["/".to_string()],
            history_idx: 0,
            loading: false,
            error: None,
            preview_content: None,
            path_input: "/".to_string(),
            new_dir_name: String::new(),
            show_new_dir: false,
        }
    }
}

impl GuestFileManagerState {
    pub fn open_for(&mut self, vm_name: &str) {
        self.open = true;
        self.vm_name = vm_name.to_string();
        self.current_path = "/".to_string();
        self.path_input = "/".to_string();
        self.entries.clear();
        self.selected_entry = None;
        self.history = vec!["/".to_string()];
        self.history_idx = 0;
        self.error = None;
        self.preview_content = None;
        self.loading = true;
    }
}

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let state = app.guest_file_manager_state();
    if !state.open {
        return;
    }

    let mut open = true;
    let mut navigate_to: Option<String> = None;
    let mut do_refresh = false;
    let mut do_back = false;
    let mut do_up = false;
    let mut do_delete: Option<String> = None;
    let mut do_mkdir = false;
    let mut do_preview: Option<String> = None;

    egui::Window::new(t!("gfm.title"))
        .open(&mut open)
        .resizable(true)
        .default_width(600.0)
        .default_height(450.0)
        .show(ctx, |ui| {
            let vm_name = app.guest_file_manager_state().vm_name.clone();

            // Path bar
            ui.horizontal(|ui| {
                if ui
                    .small_button("\u{2190}")
                    .on_hover_text(t!("gfm.back-tooltip"))
                    .clicked()
                {
                    do_back = true;
                }
                if ui
                    .small_button("\u{2191}")
                    .on_hover_text(t!("gfm.up-tooltip"))
                    .clicked()
                {
                    do_up = true;
                }
                if ui
                    .small_button("\u{21BB}")
                    .on_hover_text(t!("gfm.refresh-tooltip"))
                    .clicked()
                {
                    do_refresh = true;
                }
                if ui
                    .small_button("\u{1F4C1}")
                    .on_hover_text(t!("gfm.new-folder-tooltip"))
                    .clicked()
                {
                    app.guest_file_manager_state_mut().show_new_dir = true;
                }

                ui.separator();

                let path_input = &mut app.guest_file_manager_state_mut().path_input;
                let response = ui.add(
                    egui::TextEdit::singleline(path_input)
                        .desired_width(ui.available_width() - 60.0)
                        .font(egui::TextStyle::Monospace),
                );
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let path = app.guest_file_manager_state().path_input.clone();
                    navigate_to = Some(path);
                }
            });

            // New directory input
            if app.guest_file_manager_state().show_new_dir {
                ui.horizontal(|ui| {
                    ui.label(t!("gfm.new-folder-label"));
                    let name = &mut app.guest_file_manager_state_mut().new_dir_name;
                    ui.add(egui::TextEdit::singleline(name).desired_width(200.0));
                    if ui.small_button(t!("gfm.create")).clicked() {
                        do_mkdir = true;
                    }
                    if ui.small_button(t!("gfm.cancel")).clicked() {
                        app.guest_file_manager_state_mut().show_new_dir = false;
                        app.guest_file_manager_state_mut().new_dir_name.clear();
                    }
                });
            }

            ui.separator();

            // Error display
            if let Some(err) = app.guest_file_manager_state().error.clone() {
                ui.label(
                    egui::RichText::new(&err)
                        .color(AppColors::DANGER)
                        .size(theme::FontSize::SMALL),
                );
            }

            // Loading indicator
            if app.guest_file_manager_state().loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t!("gfm.loading"));
                });
                return;
            }

            // File list
            let entries = app.guest_file_manager_state().entries.clone();
            let selected = app.guest_file_manager_state().selected_entry;

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(ui.available_height() - 40.0)
                .show(ui, |ui| {
                    // Header
                    egui::Grid::new("fm_header")
                        .num_columns(4)
                        .spacing([8.0, 2.0])
                        .min_col_width(60.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(t!("gfm.col-name"))
                                    .strong()
                                    .size(theme::FontSize::SMALL),
                            );
                            ui.label(
                                egui::RichText::new(t!("gfm.col-size"))
                                    .strong()
                                    .size(theme::FontSize::SMALL),
                            );
                            ui.label(
                                egui::RichText::new(t!("gfm.col-perms"))
                                    .strong()
                                    .size(theme::FontSize::SMALL),
                            );
                            ui.label(
                                egui::RichText::new(t!("gfm.col-actions"))
                                    .strong()
                                    .size(theme::FontSize::SMALL),
                            );
                            ui.end_row();
                        });
                    ui.separator();

                    if entries.is_empty() {
                        ui.label(
                            egui::RichText::new(t!("gfm.empty-dir")).color(AppColors::TEXT_DIM),
                        );
                    }

                    for (i, entry) in entries.iter().enumerate() {
                        let is_selected = selected == Some(i);
                        let bg = if is_selected {
                            AppColors::BG_HOVER
                        } else {
                            egui::Color32::TRANSPARENT
                        };

                        egui::Frame::none().fill(bg).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Icon + name
                                let icon = if entry.is_dir {
                                    "\u{1F4C1}"
                                } else {
                                    "\u{1F4C4}"
                                };
                                let name_text = format!("{} {}", icon, entry.name);

                                let response = ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&name_text).size(12.0).color(
                                            if entry.is_dir {
                                                AppColors::PRIMARY
                                            } else {
                                                AppColors::TEXT
                                            },
                                        ),
                                    )
                                    .sense(egui::Sense::click()),
                                );

                                if response.clicked() {
                                    app.guest_file_manager_state_mut().selected_entry = Some(i);
                                }
                                if response.double_clicked() {
                                    if entry.is_dir {
                                        navigate_to = Some(entry.path.clone());
                                    } else if entry.size < 1024 * 1024 {
                                        do_preview = Some(entry.path.clone());
                                    }
                                }

                                // Size
                                let size_str = if entry.is_dir {
                                    "-".to_string()
                                } else {
                                    format_size(entry.size)
                                };
                                ui.label(
                                    egui::RichText::new(&size_str)
                                        .size(theme::FontSize::SMALL)
                                        .color(AppColors::TEXT_DIM),
                                );

                                // Permissions
                                ui.label(
                                    egui::RichText::new(&entry.permissions)
                                        .size(10.0)
                                        .color(AppColors::TEXT_DIM)
                                        .monospace(),
                                );

                                // Delete button
                                if !entry.is_dir {
                                    if ui
                                        .small_button("\u{1F5D1}")
                                        .on_hover_text(t!("gfm.delete-tooltip"))
                                        .clicked()
                                    {
                                        do_delete = Some(entry.path.clone());
                                    }
                                }
                            });
                        });
                    }
                });

            // Status bar
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(t!("gfm.items-count", count = entries.len()))
                        .size(10.0)
                        .color(AppColors::TEXT_DIM),
                );
                ui.label(
                    egui::RichText::new(format!("  {}", t!("gfm.vm-label", name = vm_name)))
                        .size(10.0)
                        .color(AppColors::TEXT_DIM),
                );
            });

            // Preview panel
            if let Some(ref content) = app.guest_file_manager_state().preview_content {
                ui.separator();
                ui.label(
                    egui::RichText::new(t!("gfm.file-preview"))
                        .strong()
                        .size(theme::FontSize::SMALL),
                );
                egui::ScrollArea::vertical()
                    .max_height(150.0)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(content)
                                .monospace()
                                .size(theme::FontSize::SMALL),
                        );
                    });
            }
        });

    if !open {
        app.guest_file_manager_state_mut().open = false;
    }

    // Process deferred actions
    if let Some(path) = navigate_to {
        app.action_guest_fm_navigate(&path);
    }
    if do_refresh {
        let path = app.guest_file_manager_state().current_path.clone();
        app.action_guest_fm_navigate(&path);
    }
    if do_back {
        app.action_guest_fm_back();
    }
    if do_up {
        app.action_guest_fm_up();
    }
    if let Some(path) = do_delete {
        app.action_guest_fm_delete(&path);
    }
    if do_mkdir {
        app.action_guest_fm_mkdir();
    }
    if let Some(path) = do_preview {
        app.action_guest_fm_preview(&path);
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
