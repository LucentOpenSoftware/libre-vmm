//! Encryption password dialog — create encrypted disks, change passphrase, unlock VMs.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::AppColors;
use eframe::egui;
use rust_i18n::t;

/// Dialog mode for the encryption dialog.
#[derive(Debug, Clone, PartialEq)]
pub enum EncryptionMode {
    /// Setting up encryption for a new or existing VM.
    CreateNew,
    /// Changing passphrase on an existing encrypted disk.
    ChangePassphrase,
}

/// State for the encryption dialog.
pub struct EncryptionDialogState {
    pub open: bool,
    pub mode: EncryptionMode,
    pub passphrase: String,
    pub confirm_passphrase: String,
    pub old_passphrase: String,
    pub show_passphrase: bool,
    pub error: Option<String>,
    pub success: Option<String>,
    pub vm_name: String,
}

impl Default for EncryptionDialogState {
    fn default() -> Self {
        Self {
            open: false,
            mode: EncryptionMode::CreateNew,
            passphrase: String::new(),
            confirm_passphrase: String::new(),
            old_passphrase: String::new(),
            show_passphrase: false,
            error: None,
            success: None,
            vm_name: String::new(),
        }
    }
}

impl EncryptionDialogState {
    pub fn open_create(&mut self, vm_name: &str) {
        self.open = true;
        self.mode = EncryptionMode::CreateNew;
        self.vm_name = vm_name.to_string();
        self.passphrase.clear();
        self.confirm_passphrase.clear();
        self.old_passphrase.clear();
        self.show_passphrase = false;
        self.error = None;
        self.success = None;
    }

    pub fn open_change(&mut self, vm_name: &str) {
        self.open = true;
        self.mode = EncryptionMode::ChangePassphrase;
        self.vm_name = vm_name.to_string();
        self.passphrase.clear();
        self.confirm_passphrase.clear();
        self.old_passphrase.clear();
        self.show_passphrase = false;
        self.error = None;
        self.success = None;
    }

    /// SECURITY: CWE-316 — Zeroize all passphrase fields on close.
    pub fn close_and_zeroize(&mut self) {
        self.open = false;
        zeroize_string(&mut self.passphrase);
        zeroize_string(&mut self.confirm_passphrase);
        zeroize_string(&mut self.old_passphrase);
    }
}

fn zeroize_string(s: &mut String) {
    let bytes = unsafe { s.as_mut_vec() };
    for byte in bytes.iter_mut() {
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
    s.clear();
}

/// Password strength indicator.
fn password_strength(pass: &str) -> (String, egui::Color32) {
    let len = pass.len();
    let has_upper = pass.chars().any(|c| c.is_uppercase());
    let has_lower = pass.chars().any(|c| c.is_lowercase());
    let has_digit = pass.chars().any(|c| c.is_ascii_digit());
    let has_symbol = pass.chars().any(|c| !c.is_alphanumeric());

    let score = (if len >= 8 { 1 } else { 0 })
        + (if len >= 12 { 1 } else { 0 })
        + (if has_upper && has_lower { 1 } else { 0 })
        + (if has_digit { 1 } else { 0 })
        + (if has_symbol { 1 } else { 0 });

    match score {
        0..=1 => (t!("encrypt.strength-weak").to_string(), AppColors::DANGER),
        2 => (t!("encrypt.strength-fair").to_string(), AppColors::WARNING),
        3 => (
            t!("encrypt.strength-good").to_string(),
            AppColors::STAR_COLOR,
        ),
        _ => (
            t!("encrypt.strength-strong").to_string(),
            AppColors::RUNNING,
        ),
    }
}

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let state = app.encryption_dialog_state();
    if !state.open {
        return;
    }

    // Check Esc to close the dialog (CWE-1216 — UX accessibility)
    // Use close_and_zeroize to wipe passphrase fields on dismissal (CWE-316).
    if theme::escape_pressed(ctx) {
        app.encryption_dialog_state_mut().close_and_zeroize();
        return;
    }

    let title = match state.mode {
        EncryptionMode::CreateNew => t!("encrypt.title-create", name = &state.vm_name).to_string(),
        EncryptionMode::ChangePassphrase => {
            t!("encrypt.title-change", name = &state.vm_name).to_string()
        },
    };

    let mut open = true;
    let mut do_action = false;

    egui::Window::new(&title)
        .open(&mut open)
        .resizable(false)
        .default_width(400.0)
        .collapsible(false)
        .show(ctx, |ui| {
            let mode = app.encryption_dialog_state().mode.clone();

            // Old passphrase (for change mode)
            if mode == EncryptionMode::ChangePassphrase {
                ui.label(t!("encrypt.current-passphrase").to_string());
                let show = app.encryption_dialog_state().show_passphrase;
                let old = &mut app.encryption_dialog_state_mut().old_passphrase;
                ui.add(
                    egui::TextEdit::singleline(old)
                        .password(!show)
                        .desired_width(350.0),
                );
                ui.add_space(theme::Spacing::SM);
            }

            // New passphrase
            ui.label(if mode == EncryptionMode::ChangePassphrase {
                t!("encrypt.new-passphrase").to_string()
            } else {
                t!("encrypt.passphrase").to_string()
            });
            let show = app.encryption_dialog_state().show_passphrase;
            let pass = &mut app.encryption_dialog_state_mut().passphrase;
            ui.add(
                egui::TextEdit::singleline(pass)
                    .password(!show)
                    .desired_width(350.0),
            );

            // Strength indicator
            let pass_val = app.encryption_dialog_state().passphrase.clone();
            if !pass_val.is_empty() {
                let (label, color) = password_strength(&pass_val);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(t!("encrypt.strength").to_string())
                            .size(theme::FontSize::SMALL)
                            .color(AppColors::TEXT_DIM),
                    );
                    ui.label(
                        egui::RichText::new(&label)
                            .size(theme::FontSize::SMALL)
                            .color(color)
                            .strong(),
                    );
                });
            }

            ui.add_space(theme::Spacing::XS);

            // Confirm passphrase
            ui.label(t!("encrypt.confirm-passphrase").to_string());
            let show = app.encryption_dialog_state().show_passphrase;
            let confirm = &mut app.encryption_dialog_state_mut().confirm_passphrase;
            ui.add(
                egui::TextEdit::singleline(confirm)
                    .password(!show)
                    .desired_width(350.0),
            );

            // Match indicator
            let state = app.encryption_dialog_state();
            if !state.confirm_passphrase.is_empty() && state.passphrase != state.confirm_passphrase
            {
                ui.label(
                    egui::RichText::new(t!("encrypt.mismatch").to_string())
                        .size(theme::FontSize::SMALL)
                        .color(AppColors::DANGER),
                );
            }

            ui.add_space(theme::Spacing::XS);

            // Show password toggle
            let mut show = app.encryption_dialog_state().show_passphrase;
            if ui
                .checkbox(&mut show, t!("encrypt.show-passphrase").to_string())
                .changed()
            {
                app.encryption_dialog_state_mut().show_passphrase = show;
            }

            ui.add_space(theme::Spacing::SM);

            // Error/success messages
            if let Some(err) = app.encryption_dialog_state().error.clone() {
                ui.label(
                    egui::RichText::new(&err)
                        .color(AppColors::DANGER)
                        .size(12.0),
                );
            }
            if let Some(msg) = app.encryption_dialog_state().success.clone() {
                ui.label(
                    egui::RichText::new(&msg)
                        .color(AppColors::RUNNING)
                        .size(12.0),
                );
            }

            ui.add_space(theme::Spacing::SM);
            ui.separator();

            // Buttons
            ui.horizontal(|ui| {
                if ui.button(t!("common.cancel").to_string()).clicked() {
                    app.encryption_dialog_state_mut().close_and_zeroize();
                }

                let state = app.encryption_dialog_state();
                let can_submit = !state.passphrase.is_empty()
                    && state.passphrase == state.confirm_passphrase
                    && state.passphrase.len() >= 8;

                if ui
                    .add_enabled(
                        can_submit,
                        egui::Button::new(
                            egui::RichText::new(t!("encrypt.encrypt").to_string())
                                .color(egui::Color32::WHITE),
                        )
                        .fill(AppColors::PRIMARY),
                    )
                    .clicked()
                {
                    do_action = true;
                }
            });

            ui.add_space(theme::Spacing::XS);
            ui.label(
                egui::RichText::new(t!("encrypt.hint").to_string())
                    .size(10.0)
                    .color(AppColors::TEXT_DIM),
            );
        });

    if !open {
        app.encryption_dialog_state_mut().close_and_zeroize();
    }

    if do_action {
        app.action_apply_encryption();
    }
}
