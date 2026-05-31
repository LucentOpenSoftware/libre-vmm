//! Network Conditioner GUI — simulate network conditions (latency, loss, bandwidth).
//! Uses Linux `tc` (traffic control) under the hood.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::AppColors;
use eframe::egui;
use vmm_core::domain::VmState;

/// Network Conditioner UI state.
pub struct NetCondState {
    pub open: bool,
    pub active_preset: Option<String>,
    pub custom_delay_ms: u32,
    pub custom_jitter_ms: u32,
    pub custom_loss_pct: f32,
    pub custom_bandwidth_kbps: u32,
    pub status_msg: Option<String>,
    pub is_error: bool,
}

impl Default for NetCondState {
    fn default() -> Self {
        Self {
            open: false,
            active_preset: None,
            custom_delay_ms: 100,
            custom_jitter_ms: 20,
            custom_loss_pct: 0.0,
            custom_bandwidth_kbps: 1000,
            status_msg: None,
            is_error: false,
        }
    }
}

/// Built-in presets for network simulation.
struct Preset {
    name: &'static str,
    description: &'static str,
    delay_ms: u32,
    jitter_ms: u32,
    loss_pct: f32,
    bandwidth_kbps: Option<u32>,
}

const PRESETS: &[Preset] = &[
    Preset {
        name: "No Limit",
        description: "Normal network — no conditions applied",
        delay_ms: 0,
        jitter_ms: 0,
        loss_pct: 0.0,
        bandwidth_kbps: None,
    },
    Preset {
        name: "WiFi (Good)",
        description: "20ms latency, 0.1% loss",
        delay_ms: 20,
        jitter_ms: 5,
        loss_pct: 0.1,
        bandwidth_kbps: Some(50_000),
    },
    Preset {
        name: "WiFi (Poor)",
        description: "80ms latency, 2% loss",
        delay_ms: 80,
        jitter_ms: 30,
        loss_pct: 2.0,
        bandwidth_kbps: Some(5_000),
    },
    Preset {
        name: "4G/LTE",
        description: "50ms latency, 0.5% loss, 20 Mbps",
        delay_ms: 50,
        jitter_ms: 15,
        loss_pct: 0.5,
        bandwidth_kbps: Some(20_000),
    },
    Preset {
        name: "3G",
        description: "200ms latency, 1% loss, 1 Mbps",
        delay_ms: 200,
        jitter_ms: 50,
        loss_pct: 1.0,
        bandwidth_kbps: Some(1_000),
    },
    Preset {
        name: "Satellite",
        description: "600ms latency, 2% loss",
        delay_ms: 600,
        jitter_ms: 100,
        loss_pct: 2.0,
        bandwidth_kbps: Some(5_000),
    },
    Preset {
        name: "High Latency",
        description: "500ms latency (VPN/tunnels)",
        delay_ms: 500,
        jitter_ms: 50,
        loss_pct: 0.0,
        bandwidth_kbps: None,
    },
    Preset {
        name: "Lossy Network",
        description: "5% packet loss",
        delay_ms: 50,
        jitter_ms: 10,
        loss_pct: 5.0,
        bandwidth_kbps: None,
    },
    Preset {
        name: "Terrible",
        description: "1000ms, 10% loss, 256 Kbps",
        delay_ms: 1000,
        jitter_ms: 200,
        loss_pct: 10.0,
        bandwidth_kbps: Some(256),
    },
];

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    if !app.net_cond_state().open {
        return;
    }

    // Check Esc to close the dialog (CWE-1216 — UX accessibility)
    if theme::escape_pressed(ctx) {
        app.net_cond_state_mut().open = false;
        return;
    }

    let Some(vm_name) = app.selected_vm().map(|s| s.to_string()) else {
        app.net_cond_state_mut().open = false;
        return;
    };

    let vm_state = app.selected_vm_state().unwrap_or(VmState::Off);
    let is_running = matches!(vm_state, VmState::Running);

    let mut open = true;
    egui::Window::new("Network Conditioner")
        .id(egui::Id::new("net_cond_dialog"))
        .default_size([480.0, 500.0])
        .resizable(true)
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.heading(
                egui::RichText::new(format!("Network: {}", vm_name))
                    .size(theme::FontSize::HEADING)
                    .color(AppColors::TEXT),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(
                    "Simulate network conditions for testing. Uses Linux traffic control (tc).",
                )
                .size(theme::FontSize::SMALL)
                .color(AppColors::TEXT_DIM),
            );

            if !is_running {
                ui.add_space(theme::Spacing::SM);
                ui.label(
                    egui::RichText::new("⚠ VM must be running to apply network conditions")
                        .size(12.0)
                        .color(AppColors::WARNING),
                );
            }

            ui.add_space(theme::Spacing::SM);

            // Current state
            let active = app.net_cond_state().active_preset.clone();
            egui::Frame::none()
                .fill(AppColors::BG_CARD)
                .rounding(theme::ThemeRounding::BUTTON)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Active:")
                                .size(12.0)
                                .color(AppColors::TEXT_DIM),
                        );
                        ui.label(
                            egui::RichText::new(active.as_deref().unwrap_or("No Limit"))
                                .size(theme::FontSize::BODY)
                                .strong()
                                .color(
                                    if active.is_some() && active.as_deref() != Some("No Limit") {
                                        AppColors::WARNING
                                    } else {
                                        AppColors::SUCCESS
                                    },
                                ),
                        );

                        if active.is_some() && active.as_deref() != Some("No Limit") {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled(is_running, egui::Button::new("Clear"))
                                        .clicked()
                                    {
                                        app.action_clear_network_condition(&vm_name);
                                    }
                                },
                            );
                        }
                    });
                });

            ui.add_space(theme::Spacing::SM);

            // Preset buttons
            ui.label(
                egui::RichText::new("Presets")
                    .size(14.0)
                    .strong()
                    .color(AppColors::TEXT),
            );
            ui.add_space(theme::Spacing::XS);

            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    for preset in PRESETS {
                        let is_active = active.as_deref() == Some(preset.name);

                        egui::Frame::none()
                            .fill(if is_active {
                                AppColors::BG_HOVER
                            } else {
                                AppColors::BG_CARD
                            })
                            .rounding(theme::ThemeRounding::BUTTON_SMALL)
                            .inner_margin(theme::Spacing::SM)
                            .stroke(if is_active {
                                egui::Stroke::new(1.0, AppColors::PRIMARY)
                            } else {
                                egui::Stroke::NONE
                            })
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(preset.name)
                                                .size(theme::FontSize::BODY)
                                                .strong()
                                                .color(AppColors::TEXT),
                                        );
                                        ui.label(
                                            egui::RichText::new(preset.description)
                                                .size(theme::FontSize::SMALL)
                                                .color(AppColors::TEXT_DIM),
                                        );
                                    });

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if !is_active {
                                                if ui
                                                    .add_enabled(
                                                        is_running,
                                                        egui::Button::new("Apply"),
                                                    )
                                                    .clicked()
                                                {
                                                    app.action_apply_network_preset(
                                                        &vm_name,
                                                        preset.name,
                                                        preset.delay_ms,
                                                        preset.jitter_ms,
                                                        preset.loss_pct,
                                                        preset.bandwidth_kbps,
                                                    );
                                                }
                                            } else {
                                                ui.label(
                                                    egui::RichText::new("\u{2714} Active")
                                                        .size(theme::FontSize::SMALL)
                                                        .color(AppColors::SUCCESS),
                                                );
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(2.0);
                    }
                });

            ui.add_space(theme::Spacing::MD);

            // Custom configuration
            ui.label(
                egui::RichText::new("Custom")
                    .size(14.0)
                    .strong()
                    .color(AppColors::TEXT),
            );
            ui.add_space(theme::Spacing::XS);

            egui::Frame::none()
                .fill(AppColors::BG_CARD)
                .rounding(theme::ThemeRounding::BUTTON)
                .inner_margin(theme::Spacing::MD)
                .show(ui, |ui| {
                    egui::Grid::new("custom_net_cond")
                        .num_columns(2)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            // SECURITY: CWE-681 — Clamp u32 to slider range before i32 cast,
                            // and clamp result before casting back to u32 to prevent sign flip.
                            ui.label("Latency:");
                            let mut delay = (app.net_cond_state().custom_delay_ms.min(2000)) as i32;
                            if ui
                                .add(egui::Slider::new(&mut delay, 0..=2000).text("ms"))
                                .changed()
                            {
                                app.net_cond_state_mut().custom_delay_ms = delay.max(0) as u32;
                            }
                            ui.end_row();

                            ui.label("Jitter:");
                            let mut jitter =
                                (app.net_cond_state().custom_jitter_ms.min(500)) as i32;
                            if ui
                                .add(egui::Slider::new(&mut jitter, 0..=500).text("ms"))
                                .changed()
                            {
                                app.net_cond_state_mut().custom_jitter_ms = jitter.max(0) as u32;
                            }
                            ui.end_row();

                            ui.label("Packet Loss:");
                            let mut loss = app.net_cond_state().custom_loss_pct;
                            if ui
                                .add(
                                    egui::Slider::new(&mut loss, 0.0..=50.0)
                                        .text("%")
                                        .step_by(0.5),
                                )
                                .changed()
                            {
                                app.net_cond_state_mut().custom_loss_pct = loss;
                            }
                            ui.end_row();

                            ui.label("Bandwidth:");
                            let mut bw =
                                (app.net_cond_state().custom_bandwidth_kbps.min(100_000)) as i32;
                            if ui
                                .add(
                                    egui::Slider::new(&mut bw, 64..=100_000)
                                        .text("Kbps")
                                        .logarithmic(true),
                                )
                                .changed()
                            {
                                app.net_cond_state_mut().custom_bandwidth_kbps = bw.max(64) as u32;
                            }
                            ui.end_row();
                        });

                    ui.add_space(theme::Spacing::SM);
                    if ui
                        .add_enabled(is_running, egui::Button::new("Apply Custom"))
                        .clicked()
                    {
                        let s = app.net_cond_state();
                        app.action_apply_network_preset(
                            &vm_name,
                            "Custom",
                            s.custom_delay_ms,
                            s.custom_jitter_ms,
                            s.custom_loss_pct,
                            Some(s.custom_bandwidth_kbps),
                        );
                    }
                });

            // Status message
            let ncs = app.net_cond_state();
            if let Some(ref msg) = ncs.status_msg {
                ui.add_space(theme::Spacing::SM);
                ui.label(egui::RichText::new(msg).size(12.0).color(if ncs.is_error {
                    AppColors::DANGER
                } else {
                    AppColors::SUCCESS
                }));
            }
        });

    if !open {
        app.net_cond_state_mut().open = false;
    }
}
