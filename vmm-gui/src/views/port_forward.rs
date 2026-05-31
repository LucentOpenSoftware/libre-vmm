//! Port forwarding rules editor — VirtualBox-style NAT port forward UI.
//! Renders as a section in VM Settings or as a floating dialog.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::{AppColors, Spacing};
use eframe::egui;
use rust_i18n::t;
use vmm_core::config::{PortForwardRule, PortProtocol};

/// State for the port forwarding editor.
#[derive(Debug, Clone, Default)]
pub struct PortForwardState {
    /// Whether the editor dialog is open.
    pub open: bool,
    /// Rules being edited (copy of config.port_forwards).
    pub rules: Vec<PortForwardRule>,
    /// New rule being composed.
    pub new_protocol: PortProtocol,
    pub new_host_port: String,
    pub new_guest_port: String,
    pub new_description: String,
    /// Error message for validation.
    pub error: Option<String>,
}

/// Render the port forwarding editor as a floating window.
pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    if !app.port_forward_state().open {
        return;
    }

    // Check Esc to close the dialog (CWE-1216 — UX accessibility)
    if theme::escape_pressed(ctx) {
        app.port_forward_state_mut().open = false;
        return;
    }

    let mut open = true;

    egui::Window::new(t!("portfwd.title"))
        .open(&mut open)
        .default_width(550.0)
        .default_height(350.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            render_inner(app, ui);
        });

    if !open {
        app.port_forward_state_mut().open = false;
    }
}

fn render_inner(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.label(egui::RichText::new(t!("portfwd.nat-title").to_string()).strong());
            ui.label(
                egui::RichText::new(t!("portfwd.nat-desc").to_string())
                    .color(AppColors::TEXT_DIM)
                    .small(),
            );
            ui.add_space(Spacing::SM);

            // Existing rules table
            let rules = app.port_forward_state().rules.clone();
            let mut to_remove: Option<usize> = None;

            if rules.is_empty() {
                ui.colored_label(AppColors::TEXT_DIM, t!("portfwd.no-rules"));
            } else {
                egui::Grid::new("port_forward_rules")
                    .num_columns(5)
                    .spacing([Spacing::MD, Spacing::XS])
                    .striped(true)
                    .show(ui, |ui| {
                        // Header
                        ui.label(egui::RichText::new(t!("portfwd.protocol").to_string()).strong());
                        ui.label(egui::RichText::new(t!("portfwd.host-port").to_string()).strong());
                        ui.label(
                            egui::RichText::new(t!("portfwd.guest-port").to_string()).strong(),
                        );
                        ui.label(
                            egui::RichText::new(t!("portfwd.description").to_string()).strong(),
                        );
                        ui.label(""); // action column
                        ui.end_row();

                        for (i, rule) in rules.iter().enumerate() {
                            ui.label(format!("{}", rule.protocol));
                            ui.label(format!("{}", rule.host_port));
                            ui.label(format!("{}", rule.guest_port));
                            ui.label(if rule.description.is_empty() {
                                "—".to_string()
                            } else {
                                rule.description.clone()
                            });
                            if ui
                                .small_button(egui::RichText::new("✕").color(AppColors::DANGER))
                                .on_hover_text(t!("portfwd.remove-tooltip").to_string())
                                .clicked()
                            {
                                to_remove = Some(i);
                            }
                            ui.end_row();
                        }
                    });
            }

            // Remove rule if requested
            if let Some(idx) = to_remove {
                app.port_forward_state_mut().rules.remove(idx);
            }

            ui.add_space(Spacing::MD);
            ui.separator();
            ui.add_space(Spacing::XS);

            // Add new rule form
            ui.label(egui::RichText::new(t!("portfwd.add-rule").to_string()).strong());

            ui.horizontal(|ui| {
                // Protocol selector
                ui.label(t!("portfwd.protocol-label"));
                let current_proto = app.port_forward_state().new_protocol.clone();
                egui::ComboBox::from_id_salt("pf_proto")
                    .selected_text(format!("{}", current_proto))
                    .width(70.0)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_value(
                                &mut app.port_forward_state_mut().new_protocol,
                                PortProtocol::Tcp,
                                "TCP",
                            )
                            .changed()
                        {}
                        if ui
                            .selectable_value(
                                &mut app.port_forward_state_mut().new_protocol,
                                PortProtocol::Udp,
                                "UDP",
                            )
                            .changed()
                        {}
                    });
            });

            ui.horizontal(|ui| {
                ui.label(t!("portfwd.host-port-label"));
                ui.add(
                    egui::TextEdit::singleline(app.port_forward_state_mut().new_host_port_mut())
                        .desired_width(60.0)
                        .hint_text("2222"),
                );
                ui.label(t!("portfwd.arrow"));
                ui.add(
                    egui::TextEdit::singleline(app.port_forward_state_mut().new_guest_port_mut())
                        .desired_width(60.0)
                        .hint_text("22"),
                );
            });

            ui.horizontal(|ui| {
                ui.label(t!("portfwd.description-label"));
                ui.add(
                    egui::TextEdit::singleline(app.port_forward_state_mut().new_description_mut())
                        .desired_width(200.0)
                        .hint_text("SSH"),
                );
            });

            // Error message
            if let Some(ref err) = app.port_forward_state().error.clone() {
                ui.colored_label(AppColors::DANGER, err);
            }

            ui.add_space(Spacing::XS);
            ui.horizontal(|ui| {
                if ui
                    .button(format!("\u{2795} {}", t!("portfwd.add-btn")))
                    .clicked()
                {
                    let state = app.port_forward_state();
                    let host_port: Result<u16, _> = state.new_host_port.parse();
                    let guest_port: Result<u16, _> = state.new_guest_port.parse();

                    match (host_port, guest_port) {
                        (Ok(hp), Ok(gp)) if hp > 0 && gp > 0 => {
                            // SECURITY (CWE-78, CWE-91): Validate description before storing.
                            // Descriptions may end up in iptables comments, libvirt XML, or
                            // QEMU -netdev hostfwd arguments in future implementations.
                            if let Err(e) = validate_pf_description(&state.new_description) {
                                app.port_forward_state_mut().error =
                                    Some(t!("portfwd.invalid-desc", err = e).to_string());
                            } else {
                                let rule = PortForwardRule {
                                    protocol: state.new_protocol.clone(),
                                    host_port: hp,
                                    guest_port: gp,
                                    description: state.new_description.clone(),
                                };
                                let state = app.port_forward_state_mut();
                                state.rules.push(rule);
                                state.new_host_port.clear();
                                state.new_guest_port.clear();
                                state.new_description.clear();
                                state.error = None;
                            }
                        },
                        _ => {
                            app.port_forward_state_mut().error =
                                Some(t!("portfwd.invalid-ports").to_string());
                        },
                    }
                }

                // Common presets
                ui.menu_button(t!("portfwd.quick-add"), |ui| {
                    if ui.button("SSH (2222 → 22)").clicked() {
                        app.port_forward_state_mut().rules.push(PortForwardRule {
                            protocol: PortProtocol::Tcp,
                            host_port: 2222,
                            guest_port: 22,
                            description: "SSH".to_string(),
                        });
                        ui.close_menu();
                    }
                    if ui.button("HTTP (8080 → 80)").clicked() {
                        app.port_forward_state_mut().rules.push(PortForwardRule {
                            protocol: PortProtocol::Tcp,
                            host_port: 8080,
                            guest_port: 80,
                            description: "HTTP".to_string(),
                        });
                        ui.close_menu();
                    }
                    if ui.button("HTTPS (8443 → 443)").clicked() {
                        app.port_forward_state_mut().rules.push(PortForwardRule {
                            protocol: PortProtocol::Tcp,
                            host_port: 8443,
                            guest_port: 443,
                            description: "HTTPS".to_string(),
                        });
                        ui.close_menu();
                    }
                    if ui.button("RDP (3390 → 3389)").clicked() {
                        app.port_forward_state_mut().rules.push(PortForwardRule {
                            protocol: PortProtocol::Tcp,
                            host_port: 3390,
                            guest_port: 3389,
                            description: "RDP".to_string(),
                        });
                        ui.close_menu();
                    }
                });
            });

            ui.add_space(Spacing::SM);

            // Save / Apply buttons
            ui.horizontal(|ui| {
                if ui
                    .button(format!("\u{1F4BE} {}", t!("portfwd.save")))
                    .clicked()
                {
                    app.action_save_port_forwards();
                }
            });
        }); // ScrollArea
}

/// Maximum length for port forward rule descriptions (CWE-400).
const MAX_PF_DESCRIPTION_LEN: usize = 128;

/// Validate a port forward rule description.
/// Prevents injection if description is later used in iptables comments,
/// libvirt XML, or shell commands (CWE-78, CWE-91).
fn validate_pf_description(desc: &str) -> Result<(), String> {
    if desc.len() > MAX_PF_DESCRIPTION_LEN {
        return Err(format!(
            "Description too long ({} chars, max {})",
            desc.len(),
            MAX_PF_DESCRIPTION_LEN
        ));
    }
    // Reject characters that could enable iptables comment injection (CWE-78),
    // XML injection (CWE-91), or shell metachar injection (CWE-78).
    if desc.chars().any(|c| ";|&`$\\\"'<>!{}#\n\r\t\0".contains(c)) {
        return Err("Description contains unsafe characters".to_string());
    }
    Ok(())
}

impl PortForwardState {
    pub fn new_host_port_mut(&mut self) -> &mut String {
        &mut self.new_host_port
    }
    pub fn new_guest_port_mut(&mut self) -> &mut String {
        &mut self.new_guest_port
    }
    pub fn new_description_mut(&mut self) -> &mut String {
        &mut self.new_description
    }
}
