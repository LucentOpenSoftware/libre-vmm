//! Create VM wizard — step-by-step, beginner-friendly.

use crate::app::{LibreVmmApp, Screen, WizardStep};
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding, GRID_SPACING};
use crate::views::iso_library;
use eframe::egui;
use rust_i18n::t;
use vmm_core::config::NetworkMode;
use vmm_core::iso_detect;
use vmm_core::template::{builtin_templates, templates_by_category, OsCategory};

/// Maximum length for VM description to prevent XML/memory abuse (CWE-400).
const MAX_DESCRIPTION_LEN: usize = 4096;

/// Validate an ISO path for safety.
/// Rejects path traversal, null bytes, and non-absolute paths when non-empty (CWE-22, CWE-73).
fn validate_iso_path(path: &str) -> Option<&'static str> {
    if path.is_empty() {
        return None; // empty is OK (no ISO)
    }
    // CWE-20: Reject null bytes which could truncate paths in C libraries
    if path.contains('\0') {
        return Some("ISO path contains null bytes");
    }
    // CWE-22: Reject path traversal components
    if path.contains("..") {
        return Some("ISO path must not contain '..' (path traversal)");
    }
    // CWE-73: Must be an absolute path to avoid CWD-dependent resolution
    if !path.starts_with('/') {
        return Some("ISO path must be an absolute path (start with '/')");
    }
    // Reject shell metacharacters that could cause issues in subprocess calls (CWE-78)
    if path.chars().any(|c| ";|&`$\\\"'<>!{}".contains(c)) {
        return Some("ISO path contains unsafe shell characters");
    }
    None
}

pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui, step: &WizardStep) {
    // Progress indicator
    ui.horizontal(|ui| {
        let steps = [
            (t!("wizard.step1").to_string(), WizardStep::ChooseTemplate),
            (t!("wizard.step2").to_string(), WizardStep::Configure),
            (t!("wizard.step3").to_string(), WizardStep::Review),
        ];
        for (label, s) in &steps {
            let active = step == s;
            let color = if active {
                AppColors::PRIMARY
            } else {
                AppColors::TEXT_DIM
            };
            ui.label(egui::RichText::new(label.as_str()).color(color).strong());
            ui.label(
                egui::RichText::new(t!("wizard.step-separator").to_string())
                    .color(AppColors::TEXT_DIM),
            );
        }
    });

    ui.add_space(Spacing::MD);
    ui.separator();
    ui.add_space(Spacing::MD);

    match step {
        WizardStep::ChooseTemplate => render_step_template(app, ui),
        WizardStep::Configure => render_step_configure(app, ui),
        WizardStep::Review => render_step_review(app, ui),
    }
}

fn render_step_template(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.heading(t!("wizard.choose-os").to_string());
    ui.label(
        egui::RichText::new(t!("wizard.choose-os-sub").to_string()).color(AppColors::TEXT_DIM),
    );
    ui.add_space(Spacing::SM);

    // ── Search / filter bar ────────────────────────────────────────
    let search = app.wizard_template_search_mut();
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("\u{1F50D}") // magnifying glass
                .size(FontSize::SUBHEADING),
        );
        ui.add(
            egui::TextEdit::singleline(search)
                .hint_text(t!("wizard.search-hint").to_string())
                .desired_width(300.0),
        );
        if !search.is_empty() {
            if ui
                .small_button("\u{2715}")
                .on_hover_text(t!("tooltip.clear-search").to_string())
                .clicked()
            {
                // ✕ clear
                *search = String::new();
            }
        }
    });

    let search_lower = app.wizard_template_search().to_lowercase();
    let selected_idx = app.wizard_template_idx();

    ui.add_space(Spacing::SM);

    // Reserve space at bottom for nav buttons, then fill remaining with scroll.
    let avail = ui.available_height();
    let nav_height = 44.0;
    let scroll_height = (avail - nav_height - Spacing::SM).max(100.0);

    // ── Categorized scrollable list ────────────────────────────────
    egui::ScrollArea::vertical()
        .id_salt("template_scroll")
        .max_height(scroll_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for cat in OsCategory::ALL {
                let entries = templates_by_category(*cat);

                // Filter by search query
                let filtered: Vec<_> = if search_lower.is_empty() {
                    entries
                } else {
                    entries
                        .into_iter()
                        .filter(|(_, t)| {
                            t.label.to_lowercase().contains(&search_lower)
                                || t.description.to_lowercase().contains(&search_lower)
                                || t.id.contains(&search_lower)
                        })
                        .collect()
                };

                if filtered.is_empty() {
                    continue;
                }

                // Category header
                ui.add_space(Spacing::SM);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(cat.icon()).size(FontSize::HEADING));
                    ui.label(
                        egui::RichText::new(t!(cat.label_key()).to_string())
                            .size(FontSize::SUBHEADING)
                            .strong()
                            .color(AppColors::TEXT),
                    );
                    ui.label(
                        egui::RichText::new(format!("({})", filtered.len()))
                            .size(FontSize::SMALL)
                            .color(AppColors::MUTED),
                    );
                });
                ui.add_space(Spacing::XS);
                ui.separator();
                ui.add_space(Spacing::XS);

                // Template cards in 3-column grid
                egui::Grid::new(format!("template_grid_{:?}", cat))
                    .num_columns(3)
                    .spacing(GRID_SPACING)
                    .show(ui, |ui| {
                        for (col, (global_idx, template)) in filtered.iter().enumerate() {
                            let is_selected = *global_idx == selected_idx;
                            let fill = if is_selected {
                                AppColors::PRIMARY.linear_multiply(0.2)
                            } else {
                                AppColors::BG_CARD
                            };
                            let stroke = if is_selected {
                                egui::Stroke::new(1.5, AppColors::PRIMARY)
                            } else {
                                egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE)
                            };

                            let frame = egui::Frame::none()
                                .fill(fill)
                                .rounding(ThemeRounding::CARD)
                                .stroke(stroke)
                                .inner_margin(Spacing::SM);

                            frame.show(ui, |ui| {
                                ui.set_min_width(200.0);
                                ui.set_max_width(260.0);
                                let response = ui
                                    .vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(template.label)
                                                .size(FontSize::BODY)
                                                .strong()
                                                .color(if is_selected {
                                                    AppColors::PRIMARY
                                                } else {
                                                    AppColors::TEXT
                                                }),
                                        );
                                        ui.label(
                                            egui::RichText::new(template.description)
                                                .size(FontSize::SMALL)
                                                .color(AppColors::TEXT_DIM),
                                        );
                                        ui.label(
                                            egui::RichText::new(
                                                t!(
                                                    "wizard.specs-format",
                                                    cpus = template.recommended_cpus,
                                                    memory = template.recommended_memory_mib,
                                                    disk = template.recommended_disk_gib,
                                                )
                                                .to_string(),
                                            )
                                            .size(FontSize::SMALL)
                                            .color(AppColors::MUTED),
                                        );
                                    })
                                    .response;

                                if response.interact(egui::Sense::click()).clicked() {
                                    app.action_apply_template(*global_idx);
                                }
                            });

                            if col % 3 == 2 {
                                ui.end_row();
                            }
                        }
                        // Close last incomplete row
                        ui.end_row();
                    });

                ui.add_space(Spacing::SM);
            }
        });

    // ── Navigation bar (always visible at bottom) ──────────────────
    ui.add_space(Spacing::SM);
    ui.separator();
    ui.add_space(Spacing::XS);
    ui.horizontal(|ui| {
        if ui.button(t!("wizard.cancel").to_string()).clicked() {
            app.set_screen(Screen::Home);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let next = egui::Button::new(
                egui::RichText::new(t!("wizard.next").to_string()).color(egui::Color32::WHITE),
            )
            .fill(AppColors::PRIMARY)
            .min_size(egui::vec2(140.0, 32.0));
            if ui.add(next).clicked() {
                app.set_screen(Screen::CreateWizard(WizardStep::Configure));
            }
        });
    });
}

fn render_step_configure(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.heading(t!("wizard.configure").to_string());
    ui.add_space(Spacing::SM);

    egui::Grid::new("config_grid")
        .num_columns(2)
        .spacing([Spacing::LG, 10.0])
        .show(ui, |ui| {
            // Name
            ui.label(t!("wizard.name").to_string());
            let name = app.wizard_name_mut();
            ui.add(
                egui::TextEdit::singleline(name)
                    .hint_text(t!("wizard.name-hint").to_string())
                    .desired_width(300.0),
            );
            ui.end_row();

            // ISO
            ui.label(t!("wizard.iso").to_string());
            ui.horizontal(|ui| {
                let iso = app.wizard_iso_mut();
                ui.add(
                    egui::TextEdit::singleline(iso)
                        .hint_text(t!("wizard.iso-hint").to_string())
                        .desired_width(200.0),
                );
                if ui.button(t!("wizard.browse").to_string()).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ISO Images", &["iso", "img"])
                        .pick_file()
                    {
                        let path_str = path.display().to_string();
                        *iso = path_str.clone();
                        // Auto-detect OS from ISO and select matching template
                        auto_detect_and_apply_template(app, &path_str);
                    }
                }
                // ISO Library toggle
                let picker_label = if app.show_iso_picker() {
                    t!("wizard.hide-library").to_string()
                } else {
                    t!("wizard.iso-library").to_string()
                };
                if ui.button(picker_label).clicked() {
                    let current = app.show_iso_picker();
                    app.set_show_iso_picker(!current);
                }
            });
            ui.end_row();
        });

    // ISO Library picker (shown inline below the grid when toggled)
    if app.show_iso_picker() {
        ui.add_space(Spacing::SM);
        egui::Frame::none()
            .fill(AppColors::BG_CARD)
            .rounding(ThemeRounding::CARD)
            .inner_margin(Spacing::MD)
            .stroke(egui::Stroke::new(0.5, AppColors::PRIMARY))
            .show(ui, |ui| {
                let state = app.iso_library_state_mut();
                if let Some(path) = iso_library::render_picker(state, ui) {
                    // ISO was selected from library — set it
                    *app.wizard_iso_mut() = path.clone();
                    app.set_show_iso_picker(false);
                    // Auto-detect OS from ISO and select matching template
                    auto_detect_and_apply_template(app, &path);
                }
            });
    }

    // Continue config grid
    egui::Grid::new("config_grid_2")
        .num_columns(2)
        .spacing([Spacing::LG, 10.0])
        .show(ui, |ui| {
            // CPUs
            ui.label(t!("wizard.cpus").to_string());
            // SECURITY: CWE-681 — Clamp before cast to prevent sign flip on corrupted config.
            let mut cpus = (app.wizard_cpus().min(16)) as i32;
            ui.add(egui::Slider::new(&mut cpus, 1..=16).text(t!("wizard.cores").to_string()));
            app.set_wizard_cpus(cpus.max(1) as u32);
            ui.end_row();

            // Memory
            ui.label(t!("wizard.memory").to_string());
            // SECURITY: CWE-681 — u64→i32 truncation. Clamp to slider range before cast.
            let mut mem = (app.wizard_memory_mib().min(32768)) as i32;
            ui.add(
                egui::Slider::new(&mut mem, 512..=32768)
                    .text(t!("wizard.mib").to_string())
                    .step_by(512.0),
            );
            app.set_wizard_memory_mib(mem.max(512) as u64);
            ui.end_row();

            // Disk
            ui.label(t!("wizard.disk-size").to_string());
            // SECURITY: CWE-681 — u64→i32 truncation. Clamp to slider range before cast.
            let mut disk = (app.wizard_disk_gib().min(500)) as i32;
            ui.add(
                egui::Slider::new(&mut disk, 5..=500)
                    .text(t!("wizard.gib").to_string())
                    .step_by(5.0),
            );
            app.set_wizard_disk_gib(disk.max(5) as u64);
            ui.end_row();

            // Network
            ui.label(t!("wizard.network").to_string());
            let current = app.wizard_network().clone();
            egui::ComboBox::from_id_salt("network_mode")
                .selected_text(match &current {
                    NetworkMode::Nat => t!("wizard.net-nat").to_string(),
                    NetworkMode::Bridged => t!("wizard.net-bridged").to_string(),
                    NetworkMode::HostOnly => t!("wizard.net-host-only").to_string(),
                    NetworkMode::LanSegment(name) => format!("LAN: {}", name),
                    NetworkMode::None => t!("wizard.net-none").to_string(),
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            current == NetworkMode::Nat,
                            t!("wizard.net-nat").to_string(),
                        )
                        .clicked()
                    {
                        app.set_wizard_network(NetworkMode::Nat);
                    }
                    if ui
                        .selectable_label(
                            current == NetworkMode::Bridged,
                            t!("wizard.net-bridged").to_string(),
                        )
                        .clicked()
                    {
                        app.set_wizard_network(NetworkMode::Bridged);
                    }
                    if ui
                        .selectable_label(
                            current == NetworkMode::HostOnly,
                            t!("wizard.net-host-only").to_string(),
                        )
                        .clicked()
                    {
                        app.set_wizard_network(NetworkMode::HostOnly);
                    }
                    if ui
                        .selectable_label(
                            current == NetworkMode::None,
                            t!("wizard.net-none").to_string(),
                        )
                        .clicked()
                    {
                        app.set_wizard_network(NetworkMode::None);
                    }
                });
            ui.end_row();

            // UEFI
            ui.label(t!("wizard.uefi-boot").to_string());
            let mut uefi = app.wizard_uefi();
            ui.checkbox(&mut uefi, t!("wizard.uefi-enable").to_string());
            app.set_wizard_uefi(uefi);
            ui.end_row();

            // Description
            ui.label(t!("wizard.notes").to_string());
            let desc = app.wizard_description_mut();
            ui.add(
                egui::TextEdit::multiline(desc)
                    .hint_text(t!("wizard.notes-hint").to_string())
                    .desired_width(300.0)
                    .desired_rows(2),
            );
            ui.end_row();
        });

    ui.add_space(Spacing::LG);

    // Navigation
    ui.horizontal(|ui| {
        if ui.button(t!("wizard.back").to_string()).clicked() {
            app.set_screen(Screen::CreateWizard(WizardStep::ChooseTemplate));
        }
        if ui.button(t!("wizard.cancel").to_string()).clicked() {
            app.set_screen(Screen::Home);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_proceed = !app.wizard_name().is_empty();
            let next = egui::Button::new(
                egui::RichText::new(t!("wizard.next-review").to_string())
                    .color(egui::Color32::WHITE),
            )
            .fill(if can_proceed {
                AppColors::PRIMARY
            } else {
                AppColors::MUTED
            });
            if ui.add_enabled(can_proceed, next).clicked() {
                app.set_screen(Screen::CreateWizard(WizardStep::Review));
            }
        });
    });
}

fn render_step_review(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.heading(t!("wizard.review-create").to_string());
    ui.add_space(Spacing::SM);

    let templates = builtin_templates();
    let template = &templates[app.wizard_template_idx()];

    // Pre-compute translated strings to avoid temporary reference issues.
    let lbl_name = t!("wizard.review-name").to_string();
    let lbl_template = t!("wizard.review-template").to_string();
    let lbl_iso = t!("wizard.review-iso").to_string();
    let lbl_cpus = t!("wizard.review-cpus").to_string();
    let lbl_memory = t!("wizard.review-memory").to_string();
    let lbl_disk = t!("wizard.review-disk").to_string();
    let lbl_network = t!("wizard.review-network").to_string();
    let lbl_uefi = t!("wizard.review-uefi").to_string();

    let val_iso = if app.wizard_iso().is_empty() {
        t!("wizard.iso-none").to_string()
    } else {
        app.wizard_iso().to_string()
    };
    let val_cpus = app.wizard_cpus().to_string();
    let val_memory = t!(
        "wizard.memory-format",
        mib = app.wizard_memory_mib(),
        gib = format!("{:.1}", app.wizard_memory_mib() as f64 / 1024.0),
    )
    .to_string();
    let val_disk = t!("wizard.disk-format", gib = app.wizard_disk_gib()).to_string();
    let val_network = match app.wizard_network() {
        NetworkMode::Nat => t!("wizard.net-nat-short").to_string(),
        NetworkMode::Bridged => t!("wizard.net-bridged").to_string(),
        NetworkMode::HostOnly => t!("wizard.net-host-only").to_string(),
        NetworkMode::LanSegment(name) => format!("LAN: {}", name),
        NetworkMode::None => t!("wizard.net-none").to_string(),
    };
    let val_uefi = if app.wizard_uefi() {
        t!("common.yes").to_string()
    } else {
        t!("common.no").to_string()
    };

    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::CARD)
        .inner_margin(Spacing::LG)
        .show(ui, |ui| {
            egui::Grid::new("review_grid")
                .num_columns(2)
                .spacing([Spacing::XL, Spacing::SM])
                .show(ui, |ui| {
                    review_row(ui, &lbl_name, app.wizard_name());
                    review_row(ui, &lbl_template, template.label);
                    review_row(ui, &lbl_iso, &val_iso);
                    review_row(ui, &lbl_cpus, &val_cpus);
                    review_row(ui, &lbl_memory, &val_memory);
                    review_row(ui, &lbl_disk, &val_disk);
                    review_row(ui, &lbl_network, &val_network);
                    review_row(ui, &lbl_uefi, &val_uefi);
                });
        });

    ui.add_space(Spacing::LG);

    ui.horizontal(|ui| {
        if ui.button(t!("wizard.back").to_string()).clicked() {
            app.set_screen(Screen::CreateWizard(WizardStep::Configure));
        }
        if ui.button(t!("wizard.cancel").to_string()).clicked() {
            app.set_screen(Screen::Home);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let create = egui::Button::new(
                egui::RichText::new(t!("wizard.create-vm").to_string())
                    .color(egui::Color32::WHITE)
                    .size(FontSize::SUBHEADING + 1.0),
            )
            .fill(AppColors::SUCCESS)
            .min_size(egui::vec2(200.0, 36.0))
            .rounding(ThemeRounding::CARD);

            if ui.add(create).clicked() {
                // SECURITY: GUI-layer validation before passing to core.
                // Defense-in-depth: core also validates, but catching here gives
                // better UX and blocks malformed data earlier.

                // CWE-20: Validate VM name
                if let Some(err) = vmm_core::config::validate_vm_name(app.wizard_name()) {
                    app.report_validation_error(t!("wizard.invalid-name", err = err).to_string());
                } else if let Some(err) = validate_iso_path(app.wizard_iso()) {
                    // CWE-22/CWE-73: Validate ISO path
                    app.report_validation_error(t!("wizard.invalid-iso", err = err).to_string());
                } else if app.wizard_description().len() > MAX_DESCRIPTION_LEN {
                    // CWE-400: Prevent oversized description
                    app.report_validation_error(
                        t!(
                            "wizard.desc-too-long",
                            len = app.wizard_description().len(),
                            max = MAX_DESCRIPTION_LEN,
                        )
                        .to_string(),
                    );
                } else {
                    app.action_create();
                }
            }
        });
    });
}

/// Auto-detect OS from an ISO path and apply the matching template.
fn auto_detect_and_apply_template(app: &mut LibreVmmApp, iso_path: &str) {
    if let Some(detected) = iso_detect::detect_os_from_iso(iso_path) {
        let templates = builtin_templates();
        let hint = detected.template_hint.to_lowercase();
        // Map detected OS hint to template ID
        let target_id = match hint.as_str() {
            h if h.contains("ubuntu") || h.contains("pop") => Some("ubuntu-desktop"),
            h if h.contains("mint") => Some("linux-mint"),
            h if h.contains("debian") => Some("debian-desktop"),
            h if h.contains("fedora") => Some("fedora-workstation"),
            h if h.contains("arch") || h.contains("manjaro") || h.contains("endeavour") => {
                Some("arch-linux")
            },
            h if h.contains("windows 11") || h.contains("windows server") => Some("windows-11"),
            h if h.contains("windows") => Some("windows-10"),
            h if h.contains("freebsd") => Some("freebsd"),
            h if h.contains("centos")
                || h.contains("rocky")
                || h.contains("alma")
                || h.contains("suse")
                || h.contains("nix") =>
            {
                Some("linux-server")
            },
            _ => None,
        };
        if let Some(id) = target_id {
            if let Some(idx) = templates.iter().position(|t| t.id == id) {
                app.action_apply_template(idx);
            }
        }
    }
}

fn review_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(
        egui::RichText::new(label)
            .color(AppColors::TEXT_DIM)
            .size(FontSize::BODY),
    );
    ui.label(
        egui::RichText::new(value)
            .color(AppColors::TEXT)
            .size(FontSize::BODY)
            .strong(),
    );
    ui.end_row();
}
