//! CD/DVD Media dialog — change or eject optical media on a running VM.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;

/// State for the CD/DVD media dialog.
pub struct MediaDialogState {
    pub open: bool,
    pub current_media: Option<String>,
    pub new_iso_path: String,
    pub error: Option<String>,
    pub success_msg: Option<String>,
}

impl Default for MediaDialogState {
    fn default() -> Self {
        Self {
            open: false,
            current_media: None,
            new_iso_path: String::new(),
            error: None,
            success_msg: None,
        }
    }
}

impl MediaDialogState {
    pub fn open(&mut self) {
        self.open = true;
        self.error = None;
        self.success_msg = None;
        self.new_iso_path.clear();
    }

    pub fn close(&mut self) {
        self.open = false;
    }
}

/// Detect current CD/DVD media by parsing `virsh dumpxml` output.
pub fn detect_current_media(vm_name: &str) -> Option<String> {
    let output = std::process::Command::new("virsh")
        .args(["dumpxml", "--", vm_name])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let xml = String::from_utf8_lossy(&output.stdout);
    // Look for <disk ... device='cdrom'> blocks containing <source file='...'/>.
    // Simple line-based parsing: find cdrom section, then grab source file.
    let mut in_cdrom = false;
    for line in xml.lines() {
        let trimmed = line.trim();
        if trimmed.contains("device='cdrom'") || trimmed.contains("device=\"cdrom\"") {
            in_cdrom = true;
        }
        if in_cdrom {
            if let Some(start) = trimmed.find("file='").or_else(|| trimmed.find("file=\"")) {
                let quote_char = trimmed.as_bytes()[start + 5] as char;
                let rest = &trimmed[start + 6..];
                if let Some(end) = rest.find(quote_char) {
                    return Some(rest[..end].to_string());
                }
            }
            if trimmed.starts_with("</disk") {
                in_cdrom = false;
            }
        }
    }
    None
}

/// Detect the first CDROM target device name (e.g. "sdb") from virsh dumpxml.
fn detect_cdrom_target(vm_name: &str) -> Option<String> {
    let output = std::process::Command::new("virsh")
        .args(["dumpxml", vm_name])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let xml = String::from_utf8_lossy(&output.stdout);
    let mut in_cdrom = false;
    for line in xml.lines() {
        let trimmed = line.trim();
        if trimmed.contains("device='cdrom'") || trimmed.contains("device=\"cdrom\"") {
            in_cdrom = true;
        }
        if in_cdrom {
            if let Some(start) = trimmed.find("dev='").or_else(|| trimmed.find("dev=\"")) {
                let quote_char = trimmed.as_bytes()[start + 4] as char;
                let rest = &trimmed[start + 5..];
                if let Some(end) = rest.find(quote_char) {
                    return Some(rest[..end].to_string());
                }
            }
            if trimmed.starts_with("</disk") {
                in_cdrom = false;
            }
        }
    }
    None
}

/// Render the CD/DVD media dialog as a floating window.
pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    if !app.media_dialog_state().open {
        return;
    }

    // Check Esc to close the dialog (CWE-1216 — UX accessibility)
    if theme::escape_pressed(ctx) {
        app.media_dialog_state_mut().close();
        return;
    }

    let Some(vm_name) = app.selected_vm().map(|s| s.to_string()) else {
        app.media_dialog_state_mut().open = false;
        return;
    };

    // Detect current media on first frame (when current_media is None and no error yet)
    if app.media_dialog_state().current_media.is_none()
        && app.media_dialog_state().error.is_none()
        && app.media_dialog_state().success_msg.is_none()
    {
        let detected = detect_current_media(&vm_name);
        app.media_dialog_state_mut().current_media = detected;
    }

    let mut open = true;
    egui::Window::new(t!("media.title"))
        .id(egui::Id::new("media_dialog"))
        .default_size([420.0, 260.0])
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            // Current media display
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(t!("media.current").to_string())
                        .size(FontSize::BODY)
                        .color(AppColors::TEXT),
                );
                match app.media_dialog_state().current_media.as_deref() {
                    Some(path) => {
                        // Show just the filename for brevity
                        let display = std::path::Path::new(path)
                            .file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_else(|| path.to_string());
                        ui.label(
                            egui::RichText::new(display)
                                .size(FontSize::BODY)
                                .color(AppColors::TEXT),
                        );
                    },
                    None => {
                        ui.label(
                            egui::RichText::new(t!("media.empty").to_string())
                                .size(FontSize::BODY)
                                .color(AppColors::TEXT_DIM),
                        );
                    },
                }
            });

            ui.add_space(Spacing::SM);
            ui.separator();
            ui.add_space(Spacing::SM);

            // Insert ISO section
            ui.label(
                egui::RichText::new(t!("media.insert-iso").to_string())
                    .size(FontSize::SUBHEADING)
                    .strong()
                    .color(AppColors::TEXT),
            );
            ui.add_space(Spacing::XS);

            ui.horizontal(|ui| {
                let state = app.media_dialog_state_mut();
                ui.add(
                    egui::TextEdit::singleline(&mut state.new_iso_path)
                        .desired_width(280.0)
                        .hint_text("/path/to/image.iso"),
                );
                if ui.button(t!("media.browse")).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ISO images", &["iso", "img"])
                        .pick_file()
                    {
                        app.media_dialog_state_mut().new_iso_path =
                            path.to_string_lossy().to_string();
                    }
                }
            });

            ui.add_space(Spacing::SM);

            ui.horizontal(|ui| {
                // Insert button
                let insert_btn = egui::Button::new(
                    egui::RichText::new(t!("media.insert").to_string()).color(egui::Color32::WHITE),
                )
                .fill(AppColors::PRIMARY)
                .rounding(ThemeRounding::BUTTON)
                .min_size(egui::vec2(80.0, 28.0));

                if ui.add(insert_btn).clicked() {
                    let iso = app.media_dialog_state().new_iso_path.clone();
                    if iso.is_empty() {
                        app.media_dialog_state_mut().error =
                            Some(t!("media.select-first").to_string());
                        app.media_dialog_state_mut().success_msg = None;
                    } else {
                        // Detect the actual CDROM target from the VM XML
                        let cdrom_target =
                            detect_cdrom_target(&vm_name).unwrap_or_else(|| "sda".to_string());
                        // First eject any existing media, then insert the new one.
                        // --update handles both cases (eject+insert), but if media is already
                        // present we need --force to avoid "already has media" errors.
                        let output = std::process::Command::new("virsh")
                            .args([
                                "change-media",
                                "--domain",
                                &vm_name,
                                "--path",
                                &cdrom_target,
                                "--source",
                                &iso,
                                "--update",
                                "--force",
                                "--live",
                            ])
                            .stdin(std::process::Stdio::null())
                            .output();
                        match output {
                            Ok(o) if o.status.success() => {
                                app.media_dialog_state_mut().current_media = Some(iso);
                                app.media_dialog_state_mut().success_msg =
                                    Some(t!("media.insert-ok").to_string());
                                app.media_dialog_state_mut().error = None;
                            },
                            Ok(o) => {
                                let stderr = String::from_utf8_lossy(&o.stderr);
                                app.media_dialog_state_mut().error =
                                    Some(t!("media.insert-fail", err = stderr.trim()).to_string());
                                app.media_dialog_state_mut().success_msg = None;
                            },
                            Err(e) => {
                                app.media_dialog_state_mut().error =
                                    Some(t!("media.virsh-fail", err = e.to_string()).to_string());
                                app.media_dialog_state_mut().success_msg = None;
                            },
                        }
                    }
                }

                ui.add_space(Spacing::SM);

                // Eject button
                let eject_btn = egui::Button::new(
                    egui::RichText::new(t!("media.eject").to_string()).color(egui::Color32::WHITE),
                )
                .fill(AppColors::WARNING)
                .rounding(ThemeRounding::BUTTON)
                .min_size(egui::vec2(80.0, 28.0));

                if ui.add(eject_btn).clicked() {
                    let cdrom_target =
                        detect_cdrom_target(&vm_name).unwrap_or_else(|| "sda".to_string());
                    let output = std::process::Command::new("virsh")
                        .args([
                            "change-media",
                            "--domain",
                            &vm_name,
                            "--path",
                            &cdrom_target,
                            "--eject",
                        ])
                        .stdin(std::process::Stdio::null())
                        .output();
                    match output {
                        Ok(o) if o.status.success() => {
                            app.media_dialog_state_mut().current_media = None;
                            app.media_dialog_state_mut().success_msg =
                                Some(t!("media.eject-ok").to_string());
                            app.media_dialog_state_mut().error = None;
                        },
                        Ok(o) => {
                            let stderr = String::from_utf8_lossy(&o.stderr);
                            app.media_dialog_state_mut().error =
                                Some(t!("media.eject-fail", err = stderr.trim()).to_string());
                            app.media_dialog_state_mut().success_msg = None;
                        },
                        Err(e) => {
                            app.media_dialog_state_mut().error =
                                Some(t!("media.virsh-fail", err = e.to_string()).to_string());
                            app.media_dialog_state_mut().success_msg = None;
                        },
                    }
                }
            });

            ui.add_space(Spacing::SM);

            // Error / success messages
            if let Some(ref err) = app.media_dialog_state().error {
                ui.label(
                    egui::RichText::new(err)
                        .size(FontSize::LABEL)
                        .color(AppColors::DANGER),
                );
            }
            if let Some(ref msg) = app.media_dialog_state().success_msg {
                ui.label(
                    egui::RichText::new(msg)
                        .size(FontSize::LABEL)
                        .color(AppColors::SUCCESS),
                );
            }

            ui.add_space(Spacing::SM);

            // Close button
            ui.horizontal(|ui| {
                if ui.button(t!("media.close")).clicked() {
                    app.media_dialog_state_mut().close();
                }
            });
        });

    if !open {
        app.media_dialog_state_mut().close();
    }
}
