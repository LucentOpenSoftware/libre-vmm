//! Virtual Network Editor — create, manage, and delete virtual networks.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::{AppColors, Spacing, ThemeRounding, GRID_SPACING};
use eframe::egui;
use rust_i18n::t;
use vmm_core::network::NetworkInfo;
use vmm_core::network_editor::{NetworkConfig, NetworkMode};

/// State for the network editor dialog.
#[derive(Default)]
pub struct NetworkEditorState {
    pub open: bool,
    pub networks: Vec<NetworkInfo>,
    pub adding: bool,
    pub config: NetworkConfig,
    pub error: Option<String>,
    pub success: Option<String>,
}

impl NetworkEditorState {
    pub fn open(&mut self) {
        self.open = true;
        self.adding = false;
        self.error = None;
        self.success = None;
    }
}

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    if !app.network_editor_state().open {
        return;
    }

    // Check Esc to close the dialog (CWE-1216 — UX accessibility)
    if theme::escape_pressed(ctx) {
        app.network_editor_state_mut().open = false;
        return;
    }

    let mut open = true;
    egui::Window::new(t!("neteditor.title"))
        .open(&mut open)
        .resizable(true)
        .default_size([550.0, 450.0])
        .show(ctx, |ui| {
            render_inner(app, ui);
        });

    if !open {
        app.network_editor_state_mut().open = false;
    }
}

fn render_inner(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Error/success messages
            if let Some(ref err) = app.network_editor_state().error.clone() {
                ui.label(egui::RichText::new(err).color(AppColors::DANGER).size(12.0));
                ui.add_space(Spacing::XS);
            }
            if let Some(ref msg) = app.network_editor_state().success.clone() {
                ui.label(
                    egui::RichText::new(msg)
                        .color(AppColors::SUCCESS)
                        .size(12.0),
                );
                ui.add_space(Spacing::XS);
            }

            let adding = app.network_editor_state().adding;
            if adding {
                render_add_form(app, ui);
            } else {
                render_network_list(app, ui);
            }
        }); // ScrollArea
}

fn render_network_list(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    // Refresh button
    ui.horizontal(|ui| {
        if ui.button(t!("neteditor.refresh")).clicked() {
            app.action_refresh_networks();
        }
        ui.label(
            egui::RichText::new(t!(
                "neteditor.network-count",
                count = app.network_editor_state().networks.len()
            ))
            .size(12.0)
            .color(AppColors::TEXT_DIM),
        );
    });
    ui.add_space(Spacing::SM);

    let networks = app.network_editor_state().networks.clone();

    if networks.is_empty() {
        ui.label(egui::RichText::new(t!("neteditor.no-networks")).color(AppColors::TEXT_DIM));
    } else {
        let mut action_start: Option<String> = None;
        let mut action_stop: Option<String> = None;
        let mut action_delete: Option<String> = None;

        for net in &networks {
            egui::Frame::none()
                .fill(AppColors::BG_CARD)
                .rounding(ThemeRounding::BUTTON)
                .inner_margin(Spacing::SM)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Status dot
                        let color = if net.active {
                            AppColors::RUNNING
                        } else {
                            AppColors::OFF
                        };
                        let (dot_rect, _) =
                            ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().circle_filled(dot_rect.center(), 4.0, color);

                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(&net.name)
                                    .size(theme::FontSize::BODY)
                                    .strong()
                                    .color(AppColors::TEXT),
                            );
                            let bridge_str = if net.bridge.is_empty() {
                                t!("neteditor.none").to_string()
                            } else {
                                net.bridge.clone()
                            };
                            let status_str = if net.active {
                                t!("neteditor.active").to_string()
                            } else {
                                t!("neteditor.inactive").to_string()
                            };
                            let autostart_str = if net.autostart {
                                t!("neteditor.autostart-yes").to_string()
                            } else {
                                t!("neteditor.autostart-no").to_string()
                            };
                            let info = t!(
                                "neteditor.info",
                                bridge = bridge_str,
                                status = status_str,
                                autostart = autostart_str
                            );
                            ui.label(
                                egui::RichText::new(info)
                                    .size(theme::FontSize::SMALL)
                                    .color(AppColors::TEXT_DIM),
                            );
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Don't allow deleting the default network
                            if net.name != "default" {
                                if ui
                                    .small_button(
                                        egui::RichText::new(t!("neteditor.delete"))
                                            .color(AppColors::DANGER),
                                    )
                                    .clicked()
                                {
                                    action_delete = Some(net.name.clone());
                                }
                            }
                            if net.active {
                                if ui.small_button(t!("neteditor.stop")).clicked() {
                                    action_stop = Some(net.name.clone());
                                }
                            } else {
                                if ui.small_button(t!("neteditor.start")).clicked() {
                                    action_start = Some(net.name.clone());
                                }
                            }
                        });
                    });
                });
            ui.add_space(Spacing::XS);
        }

        // Process actions
        if let Some(name) = action_start {
            app.action_network_start(&name);
        }
        if let Some(name) = action_stop {
            app.action_network_stop(&name);
        }
        if let Some(name) = action_delete {
            app.action_network_delete(&name);
        }
    }

    ui.add_space(Spacing::MD);

    let btn = egui::Button::new(
        egui::RichText::new(t!("neteditor.add-network")).color(egui::Color32::WHITE),
    )
    .fill(AppColors::PRIMARY)
    .rounding(ThemeRounding::BUTTON);
    if ui.add(btn).clicked() {
        app.network_editor_state_mut().adding = true;
        app.network_editor_state_mut().config = NetworkConfig::default();
    }
}

fn render_add_form(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.heading(t!("neteditor.create-title"));
    ui.add_space(Spacing::SM);

    egui::Grid::new("net_editor_form")
        .num_columns(2)
        .spacing(GRID_SPACING)
        .show(ui, |ui| {
            ui.label(t!("neteditor.name"));
            let mut name = app.network_editor_state().config.name.clone();
            ui.add(
                egui::TextEdit::singleline(&mut name)
                    .desired_width(200.0)
                    .hint_text(t!("neteditor.name-hint")),
            );
            app.network_editor_state_mut().config.name = name;
            ui.end_row();

            ui.label(t!("neteditor.mode"));
            let current_mode = app.network_editor_state().config.mode.clone();
            egui::ComboBox::from_id_salt("net_mode")
                .selected_text(current_mode.to_string())
                .show_ui(ui, |ui| {
                    for mode in [
                        NetworkMode::Nat,
                        NetworkMode::Bridged,
                        NetworkMode::Isolated,
                    ] {
                        if ui
                            .selectable_label(current_mode == mode, mode.to_string())
                            .clicked()
                        {
                            app.network_editor_state_mut().config.mode = mode;
                        }
                    }
                });
            ui.end_row();

            ui.label(t!("neteditor.subnet"));
            let mut subnet = app.network_editor_state().config.subnet.clone();
            ui.add(
                egui::TextEdit::singleline(&mut subnet)
                    .desired_width(150.0)
                    .hint_text("192.168.100"),
            );
            app.network_editor_state_mut().config.subnet = subnet;
            ui.end_row();

            ui.label(t!("neteditor.netmask"));
            let mut netmask = app.network_editor_state().config.netmask.clone();
            ui.add(egui::TextEdit::singleline(&mut netmask).desired_width(150.0));
            app.network_editor_state_mut().config.netmask = netmask;
            ui.end_row();

            ui.label(t!("neteditor.dhcp-start"));
            let mut start = app.network_editor_state().config.dhcp_start.clone();
            ui.add(egui::TextEdit::singleline(&mut start).desired_width(150.0));
            app.network_editor_state_mut().config.dhcp_start = start;
            ui.end_row();

            ui.label(t!("neteditor.dhcp-end"));
            let mut end = app.network_editor_state().config.dhcp_end.clone();
            ui.add(egui::TextEdit::singleline(&mut end).desired_width(150.0));
            app.network_editor_state_mut().config.dhcp_end = end;
            ui.end_row();

            ui.label(t!("neteditor.dns"));
            let mut dns = app.network_editor_state().config.dns_enabled;
            ui.checkbox(&mut dns, t!("neteditor.enable-dns"));
            app.network_editor_state_mut().config.dns_enabled = dns;
            ui.end_row();

            ui.label(t!("neteditor.autostart"));
            let mut autostart = app.network_editor_state().config.autostart;
            ui.checkbox(&mut autostart, t!("neteditor.start-on-boot"));
            app.network_editor_state_mut().config.autostart = autostart;
            ui.end_row();
        });

    ui.add_space(Spacing::MD);

    ui.horizontal(|ui| {
        let can_create = !app.network_editor_state().config.name.is_empty();
        if ui
            .add_enabled(
                can_create,
                egui::Button::new(
                    egui::RichText::new(t!("neteditor.create-network")).color(egui::Color32::WHITE),
                )
                .fill(AppColors::SUCCESS)
                .rounding(ThemeRounding::BUTTON),
            )
            .clicked()
        {
            app.action_create_network();
        }

        if ui.button(t!("neteditor.cancel")).clicked() {
            app.network_editor_state_mut().adding = false;
        }
    });
}
