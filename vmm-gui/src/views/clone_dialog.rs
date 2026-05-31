//! Clone VM dialog — select clone type and name.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use vmm_core::clone::CloneType;

/// State for the clone dialog.
pub struct CloneDialogState {
    pub visible: bool,
    pub source_vm_name: String,
    pub new_name: String,
    pub clone_type: CloneType,
    pub in_progress: bool,
    pub error: Option<String>,
}

impl Default for CloneDialogState {
    fn default() -> Self {
        Self {
            visible: false,
            source_vm_name: String::new(),
            new_name: String::new(),
            clone_type: CloneType::Full,
            in_progress: false,
            error: None,
        }
    }
}

impl CloneDialogState {
    pub fn open(&mut self, source_name: &str) {
        self.visible = true;
        self.source_vm_name = source_name.to_string();
        self.new_name = format!("{} - Clone", source_name);
        self.clone_type = CloneType::Full;
        self.in_progress = false;
        self.error = None;
    }

    pub fn close(&mut self) {
        self.visible = false;
        // Clear all user-input fields so reopening the dialog doesn't show
        // the previous VM's data pre-filled.
        self.source_vm_name.clear();
        self.new_name.clear();
        self.clone_type = CloneType::Full;
        self.in_progress = false;
        self.error = None;
    }
}

/// Render the clone dialog as a floating window.
pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let should_show = app.clone_dialog_state().visible;
    if !should_show {
        return;
    }

    // Check Esc to close the dialog (CWE-1216 — UX accessibility)
    if theme::escape_pressed(ctx) {
        app.clone_dialog_state_mut().close();
        return;
    }

    let mut open = true;
    egui::Window::new("Clone Virtual Machine")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .min_width(400.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            // Borrow fields without cloning Strings per frame
            let in_progress = app.clone_dialog_state().in_progress;

            // Use write! into a thread-local buffer instead of format!() heap alloc
            thread_local! {
                static BUF: std::cell::RefCell<String> = std::cell::RefCell::new(String::with_capacity(64));
            }
            let clone_title = BUF.with(|buf| {
                let mut buf = buf.borrow_mut();
                buf.clear();
                buf.push_str("Clone: ");
                buf.push_str(&app.clone_dialog_state().source_vm_name);
                buf.clone()
            });

            ui.label(
                egui::RichText::new(clone_title)
                    .size(FontSize::SUBHEADING)
                    .color(AppColors::TEXT_DIM),
            );
            ui.add_space(Spacing::SM);

            // Clone name
            ui.horizontal(|ui| {
                ui.label("New Name:");
                let state = app.clone_dialog_state_mut();
                ui.add(
                    egui::TextEdit::singleline(&mut state.new_name)
                        .desired_width(280.0)
                        .hint_text("Name for the clone"),
                );
            });

            ui.add_space(Spacing::SM);

            // Clone type selection
            ui.label(
                egui::RichText::new("Clone Type:")
                    .size(FontSize::BODY)
                    .color(AppColors::TEXT),
            );
            ui.add_space(Spacing::XS);

            let current_type = app.clone_dialog_state().clone_type.clone();

            // Full clone option
            let full_frame = egui::Frame::none()
                .fill(if current_type == CloneType::Full {
                    AppColors::PRIMARY.linear_multiply(0.2)
                } else {
                    AppColors::BG_CARD
                })
                .rounding(ThemeRounding::BUTTON)
                .inner_margin(Spacing::MD)
                .stroke(if current_type == CloneType::Full {
                    egui::Stroke::new(1.5, AppColors::PRIMARY)
                } else {
                    egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE)
                });

            let full_resp = full_frame.show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Full Clone")
                            .size(FontSize::BODY)
                            .strong()
                            .color(AppColors::TEXT),
                    );
                    ui.label(
                        egui::RichText::new("Independent copy of the entire disk. Larger, but self-contained.")
                            .size(FontSize::SMALL)
                            .color(AppColors::TEXT_DIM),
                    );
                });
            }).response;
            if full_resp.interact(egui::Sense::click()).clicked() {
                app.clone_dialog_state_mut().clone_type = CloneType::Full;
            }

            ui.add_space(Spacing::XS);

            // Linked clone option
            let linked_frame = egui::Frame::none()
                .fill(if current_type == CloneType::Linked {
                    AppColors::PRIMARY.linear_multiply(0.2)
                } else {
                    AppColors::BG_CARD
                })
                .rounding(ThemeRounding::BUTTON)
                .inner_margin(Spacing::MD)
                .stroke(if current_type == CloneType::Linked {
                    egui::Stroke::new(1.5, AppColors::PRIMARY)
                } else {
                    egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE)
                });

            let linked_resp = linked_frame.show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Linked Clone")
                            .size(FontSize::BODY)
                            .strong()
                            .color(AppColors::TEXT),
                    );
                    ui.label(
                        egui::RichText::new("Uses original as backing file (fast, saves space). Depends on source VM's disk.")
                            .size(FontSize::SMALL)
                            .color(AppColors::TEXT_DIM),
                    );
                });
            }).response;
            if linked_resp.interact(egui::Sense::click()).clicked() {
                app.clone_dialog_state_mut().clone_type = CloneType::Linked;
            }

            ui.add_space(Spacing::MD);

            // Error display — borrow instead of clone
            if let Some(ref error) = app.clone_dialog_state().error {
                ui.label(
                    egui::RichText::new(format!("Error: {}", error))
                        .color(AppColors::DANGER)
                        .size(FontSize::LABEL),
                );
                ui.add_space(Spacing::XS);
            }

            // Action buttons
            ui.horizontal(|ui| {
                let can_clone = !app.clone_dialog_state().new_name.is_empty() && !in_progress;

                let clone_btn = egui::Button::new(
                    egui::RichText::new(if in_progress { "Cloning..." } else { "Clone" })
                        .color(egui::Color32::WHITE),
                )
                .fill(if can_clone { AppColors::SUCCESS } else { AppColors::MUTED })
                .rounding(ThemeRounding::BUTTON)
                .min_size(egui::vec2(100.0, 30.0));

                if ui.add_enabled(can_clone, clone_btn).clicked() {
                    // SECURITY (CWE-20, CWE-78): Validate clone name at GUI layer.
                    // The core does a partial check (starts_with('-') after sanitization)
                    // but does NOT call validate_vm_name(). We must validate here to
                    // prevent injection of shell metacharacters or libvirt XML injection
                    // via the clone name.
                    let clone_name = app.clone_dialog_state().new_name.clone();
                    if let Some(err) = vmm_core::config::validate_vm_name(&clone_name) {
                        app.clone_dialog_state_mut().error =
                            Some(format!("Invalid clone name: {}", err));
                    } else {
                        app.action_clone_vm();
                    }
                }

                if ui.button("Cancel").clicked() {
                    app.clone_dialog_state_mut().close();
                }
            });
        });

    if !open {
        app.clone_dialog_state_mut().close();
    }
}
