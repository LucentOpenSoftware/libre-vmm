//! Guest Tools installation dialog — detect guest OS, install qemu-guest-agent + spice-vdagent.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::AppColors;
use eframe::egui;
use rust_i18n::t;
use vmm_core::guest_tools::{GuestOsFamily, GuestToolsStatus, InstallStep, LinuxDistro};

/// State for the guest tools installation dialog.
pub struct GuestToolsState {
    pub open: bool,
    pub step: InstallStep,
    pub status: Option<GuestToolsStatus>,
    pub install_output: Option<String>,
    pub virtio_iso_path: String,
    /// Manual OS selection index (when auto-detect fails).
    pub manual_os_index: usize,
}

impl Default for GuestToolsState {
    fn default() -> Self {
        Self {
            open: false,
            step: InstallStep::Detecting,
            status: None,
            install_output: None,
            virtio_iso_path: String::new(),
            manual_os_index: 0,
        }
    }
}

impl GuestToolsState {
    /// Open the dialog and reset to the detecting state.
    pub fn open(&mut self) {
        self.open = true;
        self.step = InstallStep::Detecting;
        self.status = None;
        self.install_output = None;
        self.virtio_iso_path = String::new();
        self.manual_os_index = 0;
    }

    /// Close the dialog.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Reset to detecting state without closing.
    pub fn reset(&mut self) {
        self.step = InstallStep::Detecting;
        self.status = None;
        self.install_output = None;
    }
}

/// Render the guest tools dialog as a floating window.
pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    if !app.guest_tools_state().open {
        return;
    }

    let mut open = true;
    egui::Window::new(t!("gtools.title"))
        .id(egui::Id::new("guest_tools_dialog"))
        .default_size([480.0, 360.0])
        .resizable(true)
        .collapsible(false)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            let step = app.guest_tools_state().step.clone();

            match step {
                InstallStep::Detecting => render_detecting(app, ui),
                InstallStep::Detected(ref status) => {
                    let status = status.clone();
                    render_detected(app, ui, &status);
                },
                InstallStep::Installing => render_installing(ui),
                InstallStep::InstallComplete => render_complete(app, ui),
                InstallStep::Failed(ref msg) => {
                    let msg = msg.clone();
                    render_failed(app, ui, &msg);
                },
            }
        });

    if !open {
        app.guest_tools_state_mut().close();
    }
}

// ---------------------------------------------------------------------------
// Step renderers
// ---------------------------------------------------------------------------

fn render_detecting(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.add_space(theme::Spacing::SM);
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(
            egui::RichText::new(t!("gtools.detecting").to_string())
                .size(14.0)
                .color(AppColors::TEXT),
        );
    });
    ui.add_space(theme::Spacing::LG);

    ui.horizontal(|ui| {
        if ui.button(t!("gtools.detect")).clicked() {
            let vm_name = app.selected_vm().map(|s| s.to_string());
            if let Some(name) = vm_name {
                match vmm_core::guest_tools::check_tools_status(&name) {
                    Ok(status) => {
                        app.guest_tools_state_mut().status = Some(status.clone());
                        app.guest_tools_state_mut().step = InstallStep::Detected(status);
                    },
                    Err(e) => {
                        app.guest_tools_state_mut().step = InstallStep::Failed(e.to_string());
                    },
                }
            } else {
                app.guest_tools_state_mut().step =
                    InstallStep::Failed(t!("gtools.no-vm").to_string());
            }
        }

        if ui.button(t!("gtools.close")).clicked() {
            app.guest_tools_state_mut().close();
        }
    });
}

fn render_detected(app: &mut LibreVmmApp, ui: &mut egui::Ui, status: &GuestToolsStatus) {
    ui.add_space(theme::Spacing::XS);

    // OS info header
    let os_label = match &status.os_family {
        GuestOsFamily::Linux { distro } => {
            t!("gtools.os-linux", distro = distro_name(distro)).to_string()
        },
        GuestOsFamily::Windows => t!("gtools.os-windows").to_string(),
        GuestOsFamily::Unknown => t!("gtools.os-unknown").to_string(),
    };
    ui.label(
        egui::RichText::new(t!("gtools.detected-os", os = &os_label).to_string())
            .size(14.0)
            .strong()
            .color(AppColors::TEXT),
    );
    ui.add_space(theme::Spacing::SM);

    // Current status indicators
    egui::Grid::new("guest_tools_status")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            status_row(
                ui,
                &t!("gtools.qemu-agent"),
                status.agent_installed,
                status.agent_running,
            );
            status_row(
                ui,
                &t!("gtools.spice-agent"),
                status.spice_agent_installed,
                false,
            );
        });

    ui.add_space(theme::Spacing::MD);
    ui.separator();
    ui.add_space(theme::Spacing::SM);

    // OS-specific content
    match &status.os_family {
        GuestOsFamily::Linux { distro } => {
            render_linux_install(app, ui, distro);
        },
        GuestOsFamily::Windows => {
            render_windows_mount(app, ui);
        },
        GuestOsFamily::Unknown => {
            ui.label(
                egui::RichText::new(t!("gtools.unknown-os").to_string())
                    .size(theme::FontSize::BODY)
                    .color(AppColors::WARNING),
            );
            ui.add_space(theme::Spacing::SM);

            // Manual OS selection fallback
            ui.label(
                egui::RichText::new(t!("gtools.manual-select").to_string())
                    .size(theme::FontSize::BODY)
                    .strong()
                    .color(AppColors::TEXT),
            );
            ui.add_space(theme::Spacing::XS);

            const MANUAL_OPTIONS: &[(&str, &str)] = &[
                ("linux-debian", "Linux (Debian/Ubuntu)"),
                ("linux-redhat", "Linux (Fedora/RHEL)"),
                ("linux-arch", "Linux (Arch)"),
                ("linux-suse", "Linux (openSUSE/SLES)"),
                ("linux-other", "Linux (Other)"),
                ("windows", "Windows"),
            ];

            let current_idx = app.guest_tools_state().manual_os_index;
            let current_label = MANUAL_OPTIONS
                .get(current_idx)
                .map(|o| o.1)
                .unwrap_or("---");

            egui::ComboBox::from_id_salt("manual_os_select")
                .selected_text(current_label)
                .width(250.0)
                .show_ui(ui, |ui| {
                    for (idx, (_key, label)) in MANUAL_OPTIONS.iter().enumerate() {
                        if ui.selectable_label(idx == current_idx, *label).clicked() {
                            app.guest_tools_state_mut().manual_os_index = idx;
                        }
                    }
                });

            ui.add_space(theme::Spacing::SM);

            let apply_btn = egui::Button::new(
                egui::RichText::new(t!("gtools.apply-manual").to_string())
                    .color(egui::Color32::WHITE),
            )
            .fill(AppColors::PRIMARY)
            .rounding(theme::ThemeRounding::BUTTON)
            .min_size(egui::vec2(160.0, 30.0));

            if ui.add(apply_btn).clicked() {
                let idx = app.guest_tools_state().manual_os_index;
                let os_family = match idx {
                    0 => GuestOsFamily::Linux {
                        distro: LinuxDistro::Debian,
                    },
                    1 => GuestOsFamily::Linux {
                        distro: LinuxDistro::RedHat,
                    },
                    2 => GuestOsFamily::Linux {
                        distro: LinuxDistro::Arch,
                    },
                    3 => GuestOsFamily::Linux {
                        distro: LinuxDistro::Suse,
                    },
                    4 => GuestOsFamily::Linux {
                        distro: LinuxDistro::Other("linux".into()),
                    },
                    5 => GuestOsFamily::Windows,
                    _ => GuestOsFamily::Unknown,
                };
                let new_status = GuestToolsStatus {
                    agent_installed: status.agent_installed,
                    agent_running: status.agent_running,
                    spice_agent_installed: status.spice_agent_installed,
                    os_family,
                };
                app.guest_tools_state_mut().status = Some(new_status.clone());
                app.guest_tools_state_mut().step = InstallStep::Detected(new_status);
            }
            ui.add_space(theme::Spacing::XS);
        },
    }

    ui.add_space(theme::Spacing::SM);

    // Bottom buttons
    ui.horizontal(|ui| {
        if ui.button(t!("gtools.re-detect")).clicked() {
            app.guest_tools_state_mut().reset();
        }
        if ui.button(t!("gtools.close")).clicked() {
            app.guest_tools_state_mut().close();
        }
    });
}

fn render_linux_install(app: &mut LibreVmmApp, ui: &mut egui::Ui, distro: &LinuxDistro) {
    ui.label(
        egui::RichText::new(t!("gtools.packages").to_string())
            .size(theme::FontSize::BODY)
            .color(AppColors::TEXT),
    );
    ui.add_space(theme::Spacing::XS);

    let packages = match distro {
        LinuxDistro::Debian => "qemu-guest-agent, spice-vdagent",
        LinuxDistro::RedHat => "qemu-guest-agent, spice-vdagent",
        LinuxDistro::Arch => "qemu-guest-agent, spice-vdagent",
        LinuxDistro::Suse => "qemu-guest-agent, spice-vdagent",
        LinuxDistro::Other(_) => "qemu-guest-agent, spice-vdagent (best effort)",
    };
    ui.label(
        egui::RichText::new(packages)
            .size(12.0)
            .color(AppColors::TEXT_DIM),
    );
    ui.add_space(theme::Spacing::SM);

    let install_btn = egui::Button::new(
        egui::RichText::new(t!("gtools.install").to_string()).color(egui::Color32::WHITE),
    )
    .fill(AppColors::SUCCESS)
    .rounding(theme::ThemeRounding::BUTTON)
    .min_size(egui::vec2(160.0, 30.0));

    if ui.add(install_btn).clicked() {
        app.guest_tools_state_mut().step = InstallStep::Installing;
        if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
            if let Some(ref status) = app.guest_tools_state().status.clone() {
                if let GuestOsFamily::Linux { ref distro } = status.os_family {
                    match vmm_core::guest_tools::install_linux_tools(&name, distro) {
                        Ok(output) => {
                            app.guest_tools_state_mut().install_output = Some(output);
                            app.guest_tools_state_mut().step = InstallStep::InstallComplete;
                        },
                        Err(e) => {
                            app.guest_tools_state_mut().step = InstallStep::Failed(e.to_string());
                        },
                    }
                }
            }
        }
    }
}

fn render_windows_mount(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    // Auto-fill bundled ISO path if field is empty
    if app.guest_tools_state().virtio_iso_path.is_empty() {
        if let Some(bundled) = vmm_core::guest_tools::find_bundled_virtio_win_iso() {
            app.guest_tools_state_mut().virtio_iso_path = bundled;
        }
    }

    ui.label(
        egui::RichText::new(t!("gtools.windows-mount").to_string())
            .size(theme::FontSize::BODY)
            .color(AppColors::TEXT),
    );
    ui.add_space(theme::Spacing::SM);

    ui.horizontal(|ui| {
        ui.label(t!("gtools.iso-path"));
        let state = app.guest_tools_state_mut();
        ui.add(
            egui::TextEdit::singleline(&mut state.virtio_iso_path)
                .desired_width(260.0)
                .hint_text("/path/to/virtio-win.iso"),
        );
        if ui.button(t!("hostguest.browse")).clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("ISO Image", &["iso"])
                .pick_file()
            {
                app.guest_tools_state_mut().virtio_iso_path = path.to_string_lossy().to_string();
            }
        }
    });
    ui.add_space(theme::Spacing::SM);

    let iso_path_empty = app.guest_tools_state().virtio_iso_path.is_empty();
    let mount_btn = egui::Button::new(
        egui::RichText::new(t!("gtools.mount-virtio").to_string()).color(egui::Color32::WHITE),
    )
    .fill(if iso_path_empty {
        AppColors::MUTED
    } else {
        AppColors::SUCCESS
    })
    .rounding(theme::ThemeRounding::BUTTON)
    .min_size(egui::vec2(180.0, 30.0));

    if ui.add_enabled(!iso_path_empty, mount_btn).clicked() {
        let iso_path = app.guest_tools_state().virtio_iso_path.clone();
        if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
            // Windows VMs use SATA: disk=sda, install ISO=sdb, drivers=sdc
            match vmm_core::guest_tools::mount_virtio_win_iso(&name, &iso_path, "sdc") {
                Ok(()) => {
                    app.guest_tools_state_mut().step = InstallStep::InstallComplete;
                    app.guest_tools_state_mut().install_output =
                        Some(t!("gtools.mount-ok").to_string());
                },
                Err(e) => {
                    app.guest_tools_state_mut().step = InstallStep::Failed(e.to_string());
                },
            }
        }
    }
}

fn render_installing(ui: &mut egui::Ui) {
    ui.add_space(theme::Spacing::LG);
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(
            egui::RichText::new(t!("gtools.installing").to_string())
                .size(14.0)
                .color(AppColors::WARNING),
        );
    });
    ui.add_space(theme::Spacing::LG);
}

fn render_complete(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.add_space(theme::Spacing::SM);
    ui.label(
        egui::RichText::new(t!("gtools.install-ok").to_string())
            .size(14.0)
            .strong()
            .color(AppColors::SUCCESS),
    );
    ui.add_space(theme::Spacing::SM);

    // Show install output in a scrollable area
    if let Some(ref output) = app.guest_tools_state().install_output.clone() {
        ui.label(
            egui::RichText::new(t!("gtools.install-output").to_string())
                .size(12.0)
                .color(AppColors::TEXT_DIM),
        );
        ui.add_space(theme::Spacing::XS);
        egui::ScrollArea::vertical()
            .max_height(180.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(output)
                        .size(theme::FontSize::SMALL)
                        .color(AppColors::TEXT_DIM)
                        .monospace(),
                );
            });
    }

    ui.add_space(theme::Spacing::MD);
    if ui.button("Close").clicked() {
        app.guest_tools_state_mut().close();
    }
}

fn render_failed(app: &mut LibreVmmApp, ui: &mut egui::Ui, msg: &str) {
    ui.add_space(theme::Spacing::SM);
    ui.label(
        egui::RichText::new(t!("gtools.install-fail").to_string())
            .size(14.0)
            .strong()
            .color(AppColors::DANGER),
    );
    ui.add_space(theme::Spacing::XS);
    ui.label(egui::RichText::new(msg).size(12.0).color(AppColors::DANGER));
    ui.add_space(theme::Spacing::MD);

    ui.horizontal(|ui| {
        if ui.button(t!("gtools.retry")).clicked() {
            app.guest_tools_state_mut().reset();
        }
        if ui.button(t!("gtools.close")).clicked() {
            app.guest_tools_state_mut().close();
        }
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn distro_name(distro: &LinuxDistro) -> &str {
    match distro {
        LinuxDistro::Debian => "Debian/Ubuntu",
        LinuxDistro::RedHat => "Fedora/RHEL",
        LinuxDistro::Arch => "Arch",
        LinuxDistro::Suse => "openSUSE/SLES",
        LinuxDistro::Other(name) => name.as_str(),
    }
}

fn status_row(ui: &mut egui::Ui, label: &str, installed: bool, running: bool) {
    ui.label(
        egui::RichText::new(label)
            .size(12.0)
            .color(AppColors::TEXT_DIM),
    );
    let (text, color) = if installed && running {
        (
            t!("gtools.installed-running").to_string(),
            AppColors::SUCCESS,
        )
    } else if installed {
        (
            t!("gtools.installed-not-running").to_string(),
            AppColors::WARNING,
        )
    } else {
        (t!("gtools.not-installed").to_string(), AppColors::DANGER)
    };
    ui.label(egui::RichText::new(&text).size(12.0).color(color));
    ui.end_row();
}
