//! Host-Guest Integration panel — shared applications, file transfer, guest interaction.
//! Parallels-inspired: seamless host-guest UX.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::{AppColors, FontSize, Spacing};
use eframe::egui;
use rust_i18n::t;
use vmm_core::domain::VmState;

/// State for the host-guest integration panel.
#[derive(Debug, Clone, Default)]
pub struct HostGuestState {
    /// Whether the panel is open.
    pub open: bool,
    /// Cached capabilities.
    pub capabilities: Option<vmm_core::host_guest::HostGuestCapabilities>,
    /// File path to send to guest.
    pub send_file_path: String,
    /// URL to open in guest.
    pub open_url: String,
    /// Command to execute in guest.
    pub exec_command: String,
    /// Command args (space-separated).
    pub exec_args: String,
    /// Last operation result message.
    pub result_message: Option<String>,
    /// Whether result was an error.
    pub result_is_error: bool,
}

/// Render the host-guest integration panel as a floating window.
pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    if !app.host_guest_state().open {
        return;
    }

    // Check Esc to close the dialog (CWE-1216 — UX accessibility)
    if theme::escape_pressed(ctx) {
        app.host_guest_state_mut().open = false;
        return;
    }

    let mut open = true;

    egui::Window::new(t!("hostguest.title"))
        .open(&mut open)
        .default_width(500.0)
        .default_height(450.0)
        .resizable(true)
        .collapsible(true)
        .show(ctx, |ui| {
            render_inner(app, ui);
        });

    if !open {
        app.host_guest_state_mut().open = false;
    }
}

fn render_inner(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let vm_name = match app.selected_vm() {
        Some(name) => name.to_string(),
        None => {
            ui.colored_label(AppColors::TEXT_DIM, t!("hostguest.no-vm"));
            return;
        },
    };

    let state = app.selected_vm_state();
    let is_running = matches!(state, Some(VmState::Running));

    if !is_running {
        ui.colored_label(AppColors::WARNING, t!("hostguest.vm-must-run"));
        ui.add_space(Spacing::XS);
        ui.label(
            egui::RichText::new(t!("hostguest.start-install").to_string())
                .color(AppColors::TEXT_DIM)
                .size(FontSize::LABEL),
        );
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Capabilities section
            ui.label(egui::RichText::new(t!("hostguest.status").to_string()).strong());
            ui.add_space(Spacing::XS);

            let caps = app.host_guest_state().capabilities.clone();
            if let Some(ref caps) = caps {
                egui::Grid::new("hg_caps")
                    .num_columns(2)
                    .spacing([Spacing::MD, Spacing::XS])
                    .show(ui, |ui| {
                        status_row(ui, &t!("hostguest.guest-agent"), caps.agent_available);
                        status_row(ui, &t!("hostguest.file-transfer"), caps.file_transfer);
                        status_row(ui, &t!("hostguest.command-exec"), caps.command_exec);
                        status_row(
                            ui,
                            &t!("hostguest.shared-folder"),
                            caps.shared_folder_mounted,
                        );
                        ui.label(t!("hostguest.guest-os"));
                        ui.label(format!("{:?}", caps.guest_os));
                        ui.end_row();
                    });
            } else {
                ui.label(
                    egui::RichText::new(t!("hostguest.detect-hint").to_string())
                        .color(AppColors::TEXT_DIM),
                );
            }

            ui.add_space(Spacing::XS);
            if ui
                .button(format!("\u{1F50D} {}", t!("hostguest.detect")))
                .clicked()
            {
                let caps = vmm_core::host_guest::detect_capabilities(&vm_name);
                app.host_guest_state_mut().capabilities = Some(caps);
            }

            ui.add_space(Spacing::MD);
            ui.separator();
            ui.add_space(Spacing::XS);

            let agent_ok = caps.as_ref().map(|c| c.agent_available).unwrap_or(false);

            // ===== Send File to Guest =====
            ui.label(
                egui::RichText::new(format!("\u{1F4C1} {}", t!("hostguest.send-file"))).strong(),
            );
            ui.add_space(Spacing::XS);

            ui.horizontal(|ui| {
                ui.label(t!("hostguest.file-label"));
                ui.add(
                    egui::TextEdit::singleline(app.host_guest_state_mut().send_file_path_mut())
                        .desired_width(300.0)
                        .hint_text("/path/to/file"),
                );
                if ui.button(t!("hostguest.browse")).clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        app.host_guest_state_mut().send_file_path =
                            path.to_string_lossy().to_string();
                    }
                }
            });

            if ui
                .add_enabled(
                    agent_ok && !app.host_guest_state().send_file_path.is_empty(),
                    egui::Button::new(format!("\u{1F4E4} {}", t!("hostguest.open-in-guest"))),
                )
                .on_hover_text(t!("hostguest.open-in-guest-tooltip").to_string())
                .clicked()
            {
                let path = app.host_guest_state().send_file_path.clone();
                let shared = app
                    .selected_vm_config()
                    .and_then(|c| c.shared_folder.clone());

                match vmm_core::host_guest::open_file_in_guest(
                    &vm_name,
                    std::path::Path::new(&path),
                    shared.as_deref(),
                ) {
                    Ok(()) => {
                        app.host_guest_state_mut().result_message =
                            Some(t!("hostguest.file-opened", path = &path).to_string());
                        app.host_guest_state_mut().result_is_error = false;
                    },
                    Err(e) => {
                        app.host_guest_state_mut().result_message =
                            Some(t!("hostguest.failed", err = e.to_string()).to_string());
                        app.host_guest_state_mut().result_is_error = true;
                    },
                }
            }

            ui.add_space(Spacing::SM);
            ui.separator();
            ui.add_space(Spacing::XS);

            // ===== Open URL in Guest =====
            ui.label(
                egui::RichText::new(format!("\u{1F310} {}", t!("hostguest.open-url"))).strong(),
            );
            ui.add_space(Spacing::XS);

            ui.horizontal(|ui| {
                ui.label(t!("hostguest.url-label"));
                ui.add(
                    egui::TextEdit::singleline(app.host_guest_state_mut().open_url_mut())
                        .desired_width(350.0)
                        .hint_text("https://example.com"),
                );
            });

            if ui
                .add_enabled(
                    agent_ok && !app.host_guest_state().open_url.is_empty(),
                    egui::Button::new(format!("\u{1F517} {}", t!("hostguest.open-in-browser"))),
                )
                .clicked()
            {
                let url = app.host_guest_state().open_url.clone();
                match vmm_core::host_guest::open_url_in_guest(&vm_name, &url) {
                    Ok(()) => {
                        app.host_guest_state_mut().result_message =
                            Some(t!("hostguest.url-opened", url = &url).to_string());
                        app.host_guest_state_mut().result_is_error = false;
                    },
                    Err(e) => {
                        app.host_guest_state_mut().result_message =
                            Some(t!("hostguest.failed", err = e.to_string()).to_string());
                        app.host_guest_state_mut().result_is_error = true;
                    },
                }
            }

            ui.add_space(Spacing::SM);
            ui.separator();
            ui.add_space(Spacing::XS);

            // ===== Execute Command in Guest =====
            ui.label(
                egui::RichText::new(format!("\u{26A1} {}", t!("hostguest.exec-command"))).strong(),
            );
            ui.add_space(Spacing::XS);

            ui.horizontal(|ui| {
                ui.label(t!("hostguest.command-label"));
                ui.add(
                    egui::TextEdit::singleline(app.host_guest_state_mut().exec_command_mut())
                        .desired_width(200.0)
                        .hint_text("ls"),
                );
            });
            ui.horizontal(|ui| {
                ui.label(t!("hostguest.args-label"));
                ui.add(
                    egui::TextEdit::singleline(app.host_guest_state_mut().exec_args_mut())
                        .desired_width(250.0)
                        .hint_text("-la /home"),
                );
            });

            let exec_ok = agent_ok
                && caps.as_ref().map(|c| c.command_exec).unwrap_or(false)
                && !app.host_guest_state().exec_command.is_empty();

            if ui
                .add_enabled(
                    exec_ok,
                    egui::Button::new(format!("\u{25B6} {}", t!("hostguest.execute"))),
                )
                .clicked()
            {
                let cmd = app.host_guest_state().exec_command.clone();
                let args_str = app.host_guest_state().exec_args.clone();
                let args: Vec<&str> = args_str.split_whitespace().collect();

                match vmm_core::host_guest::exec_in_guest(&vm_name, &cmd, &args) {
                    Ok(result) => {
                        let msg = if result.success {
                            format!(
                                "Exit code: 0\nstdout: {}\nstderr: {}",
                                result.stdout, result.stderr
                            )
                        } else {
                            format!(
                                "Exit code: {}\nstdout: {}\nstderr: {}",
                                result.exit_code, result.stdout, result.stderr
                            )
                        };
                        app.host_guest_state_mut().result_message = Some(msg);
                        app.host_guest_state_mut().result_is_error = !result.success;
                    },
                    Err(e) => {
                        app.host_guest_state_mut().result_message =
                            Some(t!("hostguest.failed", err = e.to_string()).to_string());
                        app.host_guest_state_mut().result_is_error = true;
                    },
                }
            }

            // ===== Result Message =====
            if let Some(ref msg) = app.host_guest_state().result_message.clone() {
                ui.add_space(Spacing::SM);
                ui.separator();
                ui.add_space(Spacing::XS);
                let color = if app.host_guest_state().result_is_error {
                    AppColors::DANGER
                } else {
                    AppColors::RUNNING
                };
                ui.colored_label(color, msg);
            }
        }); // ScrollArea
}

fn status_row(ui: &mut egui::Ui, label: &str, available: bool) {
    ui.label(format!("{}:", label));
    if available {
        ui.colored_label(
            AppColors::RUNNING,
            format!("\u{2713} {}", t!("hostguest.available")),
        );
    } else {
        ui.colored_label(
            AppColors::DANGER,
            format!("\u{2717} {}", t!("hostguest.unavailable")),
        );
    }
    ui.end_row();
}

impl HostGuestState {
    pub fn send_file_path_mut(&mut self) -> &mut String {
        &mut self.send_file_path
    }
    pub fn open_url_mut(&mut self) -> &mut String {
        &mut self.open_url
    }
    pub fn exec_command_mut(&mut self) -> &mut String {
        &mut self.exec_command
    }
    pub fn exec_args_mut(&mut self) -> &mut String {
        &mut self.exec_args
    }
}
