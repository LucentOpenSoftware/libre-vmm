//! Remote Hosts — dialog for managing and connecting to remote hypervisors.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::AppColors;
use eframe::egui;
use rust_i18n::t;
use vmm_core::remote::{RemoteHost, RemoteHostsConfig};

/// State for the remote hosts dialog.
#[derive(Debug)]
pub struct RemoteHostsState {
    pub visible: bool,
    pub config: RemoteHostsConfig,
    pub adding: bool,
    pub new_name: String,
    pub new_hostname: String,
    pub new_username: String,
    pub new_ssh_port: String,
    pub new_use_system: bool,
    pub test_result: Option<String>,
    pub test_error: bool,
    pub error: Option<String>,
}

impl Default for RemoteHostsState {
    fn default() -> Self {
        Self {
            visible: false,
            config: RemoteHostsConfig::load(),
            adding: false,
            new_name: String::new(),
            new_hostname: String::new(),
            new_username: String::new(),
            new_ssh_port: "22".to_string(),
            new_use_system: true,
            test_result: None,
            test_error: false,
            error: None,
        }
    }
}

impl RemoteHostsState {
    pub fn open(&mut self) {
        self.visible = true;
        self.adding = false;
        self.config = RemoteHostsConfig::load();
        self.error = None;
        self.test_result = None;
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.reset_form();
    }

    fn reset_form(&mut self) {
        self.adding = false;
        self.new_name.clear();
        self.new_hostname.clear();
        self.new_username.clear();
        self.new_ssh_port = "22".to_string();
        self.new_use_system = true;
        self.test_result = None;
        self.test_error = false;
    }
}

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let visible = app.remote_hosts_state().visible;
    if !visible {
        return;
    }

    let mut open = true;
    egui::Window::new(t!("remote.window-title"))
        .open(&mut open)
        .resizable(true)
        .default_size([480.0, 380.0])
        .show(ctx, |ui| {
            render_inner(app, ui);
        });

    if !open {
        app.remote_hosts_state_mut().close();
    }
}

fn render_inner(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Error display
            if let Some(error) = app.remote_hosts_state().error.clone() {
                ui.label(
                    egui::RichText::new(&error)
                        .color(AppColors::DANGER)
                        .size(12.0),
                );
                ui.add_space(theme::Spacing::XS);
            }

            let adding = app.remote_hosts_state().adding;

            if adding {
                render_add_form(app, ui);
            } else {
                render_host_list(app, ui);
            }
        }); // ScrollArea
}

fn render_host_list(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    // Local connection info
    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(theme::ThemeRounding::BUTTON)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (dot_rect, _) =
                    ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                let color = if app.is_connected() {
                    AppColors::RUNNING
                } else {
                    AppColors::CRASHED
                };
                ui.painter().circle_filled(dot_rect.center(), 4.0, color);

                ui.label(
                    egui::RichText::new(t!("remote.local-machine"))
                        .size(theme::FontSize::BODY)
                        .strong()
                        .color(AppColors::TEXT),
                );
                ui.label(
                    egui::RichText::new(if app.is_connected() {
                        t!("remote.connected")
                    } else {
                        t!("remote.disconnected")
                    })
                    .size(theme::FontSize::SMALL)
                    .color(if app.is_connected() {
                        AppColors::RUNNING
                    } else {
                        AppColors::CRASHED
                    }),
                );
            });
        });

    ui.add_space(theme::Spacing::SM);

    // Remote hosts
    let hosts = app.remote_hosts_state().config.hosts.clone();

    if hosts.is_empty() {
        ui.label(
            egui::RichText::new(t!("remote.no-hosts"))
                .size(12.0)
                .color(AppColors::TEXT_DIM),
        );
    } else {
        let mut remove_idx: Option<usize> = None;
        let mut connect_idx: Option<usize> = None;

        for (i, host) in hosts.iter().enumerate() {
            egui::Frame::none()
                .fill(AppColors::BG_CARD)
                .rounding(theme::ThemeRounding::BUTTON)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(&host.name)
                                    .size(theme::FontSize::BODY)
                                    .strong()
                                    .color(AppColors::TEXT),
                            );
                            let user_str = if host.username.is_empty() {
                                host.hostname.clone()
                            } else {
                                format!("{}@{}", host.username, host.hostname)
                            };
                            ui.label(
                                egui::RichText::new(format!("{}:{}", user_str, host.ssh_port))
                                    .size(theme::FontSize::SMALL)
                                    .color(AppColors::TEXT_DIM),
                            );
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button(
                                    egui::RichText::new(t!("remote.remove"))
                                        .color(AppColors::DANGER),
                                )
                                .clicked()
                            {
                                remove_idx = Some(i);
                            }
                            if ui.small_button(t!("remote.connect")).clicked() {
                                connect_idx = Some(i);
                            }
                        });
                    });
                });
            ui.add_space(theme::Spacing::XS);
        }

        if let Some(idx) = remove_idx {
            app.remote_hosts_state_mut().config.remove_host(idx);
            let _ = app.remote_hosts_state().config.save();
        }

        if let Some(idx) = connect_idx {
            if let Some(host) = hosts.get(idx) {
                let uri = host.connection_uri();
                app.action_connect_remote(&uri, &host.name);
            }
        }
    }

    ui.add_space(theme::Spacing::MD);

    // Add host button
    let btn = egui::Button::new(
        egui::RichText::new(t!("remote.add-host-btn")).color(egui::Color32::WHITE),
    )
    .fill(AppColors::PRIMARY)
    .rounding(theme::ThemeRounding::BUTTON);
    if ui.add(btn).clicked() {
        app.remote_hosts_state_mut().adding = true;
    }
}

fn render_add_form(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.heading(t!("remote.add-heading"));
    ui.add_space(theme::Spacing::SM);

    egui::Grid::new("add_remote_host")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t!("remote.display-name"));
            let mut name = app.remote_hosts_state().new_name.clone();
            ui.add(
                egui::TextEdit::singleline(&mut name)
                    .desired_width(250.0)
                    .hint_text(t!("remote.display-name-hint")),
            );
            app.remote_hosts_state_mut().new_name = name;
            ui.end_row();

            ui.label(t!("remote.hostname"));
            let mut hostname = app.remote_hosts_state().new_hostname.clone();
            ui.add(
                egui::TextEdit::singleline(&mut hostname)
                    .desired_width(250.0)
                    .hint_text(t!("remote.hostname-hint")),
            );
            app.remote_hosts_state_mut().new_hostname = hostname;
            ui.end_row();

            ui.label(t!("remote.ssh-username"));
            let mut username = app.remote_hosts_state().new_username.clone();
            ui.add(
                egui::TextEdit::singleline(&mut username)
                    .desired_width(250.0)
                    .hint_text(t!("remote.ssh-username-hint")),
            );
            app.remote_hosts_state_mut().new_username = username;
            ui.end_row();

            ui.label(t!("remote.ssh-port"));
            let mut port = app.remote_hosts_state().new_ssh_port.clone();
            ui.add(egui::TextEdit::singleline(&mut port).desired_width(80.0));
            app.remote_hosts_state_mut().new_ssh_port = port;
            ui.end_row();

            ui.label(t!("remote.connection"));
            let mut use_system = app.remote_hosts_state().new_use_system;
            ui.horizontal(|ui| {
                ui.radio_value(&mut use_system, true, t!("remote.conn-system"));
                ui.radio_value(&mut use_system, false, t!("remote.conn-session"));
            });
            app.remote_hosts_state_mut().new_use_system = use_system;
            ui.end_row();
        });

    // Preview URI
    let host = build_host_from_form(app);
    ui.add_space(theme::Spacing::XS);
    ui.label(
        egui::RichText::new(t!("remote.uri-preview", uri = host.connection_uri()))
            .size(theme::FontSize::SMALL)
            .color(AppColors::MUTED),
    );

    // Test result
    if let Some(result) = app.remote_hosts_state().test_result.clone() {
        let color = if app.remote_hosts_state().test_error {
            AppColors::DANGER
        } else {
            AppColors::RUNNING
        };
        ui.label(
            egui::RichText::new(&result)
                .size(theme::FontSize::SMALL)
                .color(color),
        );
    }

    ui.add_space(theme::Spacing::MD);

    // Buttons
    ui.horizontal(|ui| {
        if ui.button(t!("remote.test-connection")).clicked() {
            // SECURITY (CWE-78, CWE-88): Validate inputs before executing SSH/virsh commands
            match validate_remote_host_form(app) {
                Ok(()) => {
                    let host = build_host_from_form(app);
                    match host.test_libvirt() {
                        Ok(hostname) => {
                            app.remote_hosts_state_mut().test_result =
                                Some(t!("remote.connected-to", host = hostname).into_owned());
                            app.remote_hosts_state_mut().test_error = false;
                        },
                        Err(e) => {
                            app.remote_hosts_state_mut().test_result = Some(e.to_string());
                            app.remote_hosts_state_mut().test_error = true;
                        },
                    }
                },
                Err(e) => {
                    app.remote_hosts_state_mut().test_result = Some(e);
                    app.remote_hosts_state_mut().test_error = true;
                },
            }
        }

        let can_save = !app.remote_hosts_state().new_name.is_empty()
            && !app.remote_hosts_state().new_hostname.is_empty();

        if ui
            .add_enabled(
                can_save,
                egui::Button::new(
                    egui::RichText::new(t!("remote.save-host")).color(egui::Color32::WHITE),
                )
                .fill(AppColors::SUCCESS)
                .rounding(theme::ThemeRounding::BUTTON),
            )
            .clicked()
        {
            // SECURITY (CWE-78, CWE-88, CWE-93): Validate before persisting to disk.
            // Without this, malicious hostnames/usernames could be saved and later
            // used in SSH commands when the user clicks "Connect" from the host list.
            match validate_remote_host_form(app) {
                Ok(()) => {
                    let host = build_host_from_form(app);
                    app.remote_hosts_state_mut().config.add_host(host);
                    let _ = app.remote_hosts_state().config.save();
                    app.remote_hosts_state_mut().reset_form();
                },
                Err(e) => {
                    app.remote_hosts_state_mut().error = Some(e);
                },
            }
        }

        if ui.button(t!("remote.cancel")).clicked() {
            app.remote_hosts_state_mut().reset_form();
        }
    });
}

/// Maximum length for remote host display name (CWE-400).
const MAX_HOST_NAME_LEN: usize = 128;

/// Validate remote host form inputs at the GUI layer.
/// Defense-in-depth: core validates in test_ssh()/test_libvirt(), but the GUI must
/// validate BEFORE saving to config to prevent persisting malicious values that
/// could later be used in SSH commands or libvirt URIs without re-validation.
///
/// Prevents: CWE-78 (OS command injection via SSH args), CWE-93 (CRLF injection),
/// CWE-918 (SSRF via crafted libvirt URI), CWE-88 (argument injection).
fn validate_remote_host_form(app: &LibreVmmApp) -> Result<(), String> {
    let state = app.remote_hosts_state();

    // Display name validation
    if state.new_name.is_empty() {
        return Err(t!("remote.err-name-empty").into_owned());
    }
    if state.new_name.len() > MAX_HOST_NAME_LEN {
        return Err(t!("remote.err-name-too-long", max = MAX_HOST_NAME_LEN).into_owned());
    }

    // Hostname validation (CWE-78, CWE-93)
    let hostname = &state.new_hostname;
    if hostname.is_empty() {
        return Err(t!("remote.err-hostname-empty").into_owned());
    }
    if hostname.len() > 253 {
        return Err(t!("remote.err-hostname-too-long").into_owned());
    }
    if hostname.starts_with('-') {
        return Err(t!("remote.err-hostname-dash").into_owned());
    }
    // Only allow safe hostname characters: alphanumeric, hyphens, dots, colons (IPv6), brackets
    if !hostname
        .chars()
        .all(|c| c.is_alphanumeric() || ".-:[]".contains(c))
    {
        return Err(t!("remote.err-hostname-invalid").into_owned());
    }

    // Username validation (CWE-78, CWE-93)
    let username = &state.new_username;
    if !username.is_empty() {
        if username.len() > 64 {
            return Err(t!("remote.err-username-too-long").into_owned());
        }
        if username.starts_with('-') {
            return Err(t!("remote.err-username-dash").into_owned());
        }
        if !username
            .chars()
            .all(|c| c.is_alphanumeric() || "-_.".contains(c))
        {
            return Err(t!("remote.err-username-invalid").into_owned());
        }
    }

    // SSH port validation (CWE-20)
    let port: u16 = state
        .new_ssh_port
        .parse()
        .map_err(|_| t!("remote.err-port-invalid").into_owned())?;
    if port == 0 {
        return Err(t!("remote.err-port-range").into_owned());
    }

    Ok(())
}

fn build_host_from_form(app: &LibreVmmApp) -> RemoteHost {
    let state = app.remote_hosts_state();
    RemoteHost {
        name: state.new_name.clone(),
        hostname: state.new_hostname.clone(),
        username: state.new_username.clone(),
        ssh_port: state.new_ssh_port.parse().unwrap_or(22),
        uri: String::new(),
        use_system: state.new_use_system,
    }
}
