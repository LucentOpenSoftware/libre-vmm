//! Home screen — the first impression page.
//!
//! Shown when no VM is selected. Provides:
//!   * Brand header with hypervisor info
//!   * Quick action cards (new VM, import, ISO library)
//!   * Recent VMs grid (favorites first, then alphabetical)
//!   * Empty / error states

use crate::app::{LibreVmmApp, Screen};
use crate::theme::{AppColors, BoxColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;
use vmm_core::domain::VmState;

/// Maximum number of recent VMs shown in the grid.
const MAX_RECENT: usize = 6;
/// Quick action card dimensions.
const QUICK_CARD_W: f32 = 220.0;
const QUICK_CARD_H: f32 = 96.0;
/// Recent-VM card dimensions.
const RECENT_CARD_W: f32 = 220.0;
const RECENT_CARD_H: f32 = 78.0;

pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // 1. Connection error banner (sticks at the top)
            render_connection_banner(app, ui);

            ui.add_space(Spacing::XL);

            // 2. Brand header
            render_brand_header(app, ui);

            ui.add_space(Spacing::XL);

            // If we are disconnected we still show actions but skip Recent /
            // empty state since the VM list is meaningless.
            let has_conn_err = app.connection_error().is_some();

            // 3. Quick actions row
            render_quick_actions(app, ui);

            ui.add_space(Spacing::XL);

            if has_conn_err {
                return;
            }

            // 4. Recent VMs OR empty state
            if app.vms().is_empty() {
                render_empty_state(app, ui);
            } else {
                render_recent_vms(app, ui);
                ui.add_space(Spacing::LG);
                render_stats(app, ui);
            }
        });
}

// =========================================================================
//  Connection error banner
// =========================================================================

fn render_connection_banner(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let Some(err) = app.connection_error().map(|s| s.to_string()) else {
        return;
    };

    egui::Frame::none()
        .fill(AppColors::DANGER.linear_multiply(0.18))
        .stroke(egui::Stroke::new(1.0, AppColors::DANGER))
        .rounding(ThemeRounding::CARD)
        .inner_margin(egui::Margin::symmetric(Spacing::LG, Spacing::MD))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(t!("home.connection-failed"))
                            .strong()
                            .size(FontSize::HEADING)
                            .color(AppColors::DANGER),
                    );
                    ui.label(
                        egui::RichText::new(err)
                            .size(FontSize::BODY)
                            .color(AppColors::TEXT),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn = egui::Button::new(
                        egui::RichText::new(t!("home.reconnect"))
                            .color(egui::Color32::WHITE)
                            .size(FontSize::BODY),
                    )
                    .fill(AppColors::DANGER)
                    .rounding(ThemeRounding::BUTTON);
                    if ui.add(btn).clicked() {
                        app.action_connect_local();
                    }
                });
            });
        });
}

// =========================================================================
//  Brand header
// =========================================================================

fn render_brand_header(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let accent = BoxColors::primary(app.active_box_type());
    let hyp_info = app.hypervisor_info();
    let kvm = app.kvm_available();

    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(t!("app.brand"))
                .size(FontSize::BRAND)
                .strong()
                .color(accent),
        );

        ui.add_space(Spacing::XS);

        ui.label(
            egui::RichText::new(t!("home.tagline"))
                .size(FontSize::SUBHEADING)
                .color(AppColors::TEXT_DIM),
        );

        if let Some(info) = hyp_info {
            ui.add_space(Spacing::SM);
            ui.horizontal(|ui| {
                // Center the row of badges
                ui.with_layout(
                    egui::Layout::left_to_right(egui::Align::Center)
                        .with_main_align(egui::Align::Center)
                        .with_main_justify(true),
                    |ui| {
                        ui.add_space(0.0);
                        badge(ui, &info, AppColors::PRIMARY);
                        let (label, color) = if kvm {
                            (t!("home.kvm-available").to_string(), AppColors::SUCCESS)
                        } else {
                            (t!("home.kvm-unavailable").to_string(), AppColors::WARNING)
                        };
                        badge(ui, &label, color);
                    },
                );
            });
        }
    });
}

fn badge(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::none()
        .fill(color.linear_multiply(0.18))
        .stroke(egui::Stroke::new(1.0, color))
        .rounding(ThemeRounding::BUTTON_SMALL)
        .inner_margin(egui::Margin::symmetric(Spacing::SM, 2.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(FontSize::LABEL).color(color));
        });
}

// =========================================================================
//  Quick actions row
// =========================================================================

fn render_quick_actions(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let accent = BoxColors::primary(app.active_box_type());

    // Centered horizontal row
    ui.vertical_centered(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width().min(720.0), QUICK_CARD_H),
            egui::Layout::left_to_right(egui::Align::Center)
                .with_main_align(egui::Align::Center)
                .with_main_justify(true),
            |ui| {
                if quick_card(
                    ui,
                    "\u{2795}",
                    &t!("home.quick-create").to_string(),
                    &t!("home.quick-create-sub").to_string(),
                    accent,
                ) {
                    app.set_screen(Screen::BoxSelector);
                }

                if quick_card(
                    ui,
                    "\u{1F4E5}",
                    &t!("home.quick-import").to_string(),
                    &t!("home.quick-import-sub").to_string(),
                    accent,
                ) {
                    app.import_export_state_mut().open_import();
                }

                if quick_card(
                    ui,
                    "\u{1F4BF}",
                    &t!("home.quick-iso").to_string(),
                    &t!("home.quick-iso-sub").to_string(),
                    accent,
                ) {
                    app.set_show_iso_picker(true);
                }
            },
        );
    });
}

/// Draw a single quick-action card. Returns true if clicked.
fn quick_card(
    ui: &mut egui::Ui,
    icon: &str,
    title: &str,
    subtitle: &str,
    accent: egui::Color32,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(QUICK_CARD_W, QUICK_CARD_H), egui::Sense::click());

    let fill = if response.hovered() {
        AppColors::BG_HOVER
    } else {
        AppColors::BG_CARD
    };
    let stroke_color = if response.hovered() {
        accent
    } else {
        AppColors::STROKE_SUBTLE
    };

    ui.painter().rect(
        rect,
        egui::Rounding::same(ThemeRounding::CARD),
        fill,
        egui::Stroke::new(1.0, stroke_color),
    );

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(Spacing::MD, Spacing::SM)))
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    );
    child.label(egui::RichText::new(icon).size(22.0).color(accent));
    child.add_space(Spacing::XS);
    child.label(
        egui::RichText::new(title)
            .size(FontSize::HEADING)
            .strong()
            .color(AppColors::TEXT),
    );
    child.label(
        egui::RichText::new(subtitle)
            .size(FontSize::LABEL)
            .color(AppColors::TEXT_DIM),
    );

    response.clicked()
}

// =========================================================================
//  Recent VMs
// =========================================================================

fn render_recent_vms(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    // Pre-compute the ordered list of recent VM entries.
    let entries = collect_recent_entries(app, MAX_RECENT);
    if entries.is_empty() {
        return;
    }

    ui.vertical_centered(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width().min(740.0), 0.0),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.label(
                    egui::RichText::new(t!("home.recent-title"))
                        .size(FontSize::HEADING)
                        .strong()
                        .color(AppColors::TEXT),
                );
                ui.add_space(Spacing::SM);

                // Lay out as a wrapping grid of cards
                let mut clicked: Option<String> = None;
                ui.horizontal_wrapped(|ui| {
                    for e in &entries {
                        if recent_card(ui, e) {
                            clicked = Some(e.name.clone());
                        }
                    }
                });

                if let Some(name) = clicked {
                    app.set_selected_vm(Some(name));
                    app.set_screen(Screen::Home);
                }
            },
        );
    });
}

struct RecentEntry {
    name: String,
    state: VmState,
    os_label: Option<String>,
    favorite: bool,
}

fn collect_recent_entries(app: &LibreVmmApp, limit: usize) -> Vec<RecentEntry> {
    let configs = app.vm_configs();

    // Build name -> config lookup for O(1) access.
    let cfg_map: std::collections::HashMap<&str, &vmm_core::config::VmConfig> =
        configs.iter().map(|c| (c.name.as_str(), c)).collect();

    let mut entries: Vec<RecentEntry> = app
        .vms()
        .iter()
        .map(|vm| {
            let cfg = cfg_map.get(vm.name.as_str()).copied();
            RecentEntry {
                name: vm.name.clone(),
                state: vm.state.clone(),
                os_label: cfg.map(|c| format!("{:?}", c.os_type)),
                favorite: cfg.map(|c| c.favorite).unwrap_or(false),
            }
        })
        .collect();

    // Sort: favorites first, then alphabetical (case-insensitive).
    entries.sort_by(|a, b| {
        b.favorite
            .cmp(&a.favorite)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    entries.truncate(limit);
    entries
}

/// Draw a single recent-VM card. Returns true if clicked.
fn recent_card(ui: &mut egui::Ui, entry: &RecentEntry) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(RECENT_CARD_W, RECENT_CARD_H),
        egui::Sense::click(),
    );

    let fill = if response.hovered() {
        AppColors::BG_HOVER
    } else {
        AppColors::BG_CARD
    };

    let state_color = match entry.state {
        VmState::Running => AppColors::RUNNING,
        VmState::Paused => AppColors::PAUSED,
        VmState::Crashed => AppColors::CRASHED,
        _ => AppColors::OFF,
    };

    let stroke_color = if response.hovered() {
        state_color
    } else {
        AppColors::STROKE_SUBTLE
    };

    {
        let painter = ui.painter();
        painter.rect(
            rect,
            egui::Rounding::same(ThemeRounding::CARD),
            fill,
            egui::Stroke::new(1.0, stroke_color),
        );

        // State dot in top-left
        let dot_center = rect.left_top() + egui::vec2(Spacing::MD, Spacing::MD);
        painter.circle_filled(dot_center, 5.0, state_color);

        // Favorite star in top-right
        if entry.favorite {
            painter.text(
                rect.right_top() + egui::vec2(-Spacing::MD, Spacing::MD),
                egui::Align2::RIGHT_TOP,
                "\u{2605}",
                egui::FontId::proportional(FontSize::LABEL),
                AppColors::STAR_COLOR,
            );
        }
    }

    // Text content (offset down so it doesn't collide with the dot)
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(Spacing::MD + 10.0, Spacing::SM)))
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    );
    child.label(
        egui::RichText::new(&entry.name)
            .size(FontSize::HEADING)
            .strong()
            .color(AppColors::TEXT),
    );
    child.label(
        egui::RichText::new(entry.state.as_str())
            .size(FontSize::LABEL)
            .color(state_color),
    );
    if let Some(ref os) = entry.os_label {
        child.label(
            egui::RichText::new(os)
                .size(FontSize::SMALL)
                .color(AppColors::TEXT_DIM),
        );
    }

    response.clicked()
}

// =========================================================================
//  Empty state
// =========================================================================

fn render_empty_state(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let accent = BoxColors::primary(app.active_box_type());

    ui.vertical_centered(|ui| {
        ui.add_space(Spacing::XL);

        ui.label(
            egui::RichText::new(t!("home.empty-title"))
                .size(FontSize::PAGE_TITLE)
                .strong()
                .color(AppColors::TEXT),
        );
        ui.add_space(Spacing::SM);
        ui.label(
            egui::RichText::new(t!("home.empty-sub"))
                .size(FontSize::SUBHEADING)
                .color(AppColors::TEXT_DIM),
        );
        ui.add_space(Spacing::LG);

        let btn = egui::Button::new(
            egui::RichText::new(t!("home.empty-create"))
                .color(egui::Color32::WHITE)
                .size(FontSize::HEADING)
                .strong(),
        )
        .fill(accent)
        .min_size(egui::vec2(260.0, 44.0))
        .rounding(ThemeRounding::CARD);
        if ui.add(btn).clicked() {
            app.set_screen(Screen::BoxSelector);
        }
    });
}

// =========================================================================
//  Stats line (unchanged behaviour, just moved under recent grid)
// =========================================================================

fn render_stats(app: &LibreVmmApp, ui: &mut egui::Ui) {
    let vms = app.vms();
    if vms.is_empty() {
        return;
    }
    let running = vms.iter().filter(|v| v.state == VmState::Running).count();
    let paused = vms.iter().filter(|v| v.state == VmState::Paused).count();
    let total = vms.len();

    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(
                t!(
                    "home.stats",
                    total = total,
                    s = if total != 1 { "s" } else { "" },
                    running = running,
                    paused = paused
                )
                .to_string(),
            )
            .size(FontSize::BODY)
            .color(AppColors::TEXT_DIM),
        );
    });
}
