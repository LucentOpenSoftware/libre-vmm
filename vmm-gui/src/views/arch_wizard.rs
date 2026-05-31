//! Architecture wizard — Box 2 (Hardware Lab) VM creation wizard.
//!
//! A multi-step wizard for creating VMs on any QEMU-supported architecture:
//! 1. Choose Architecture (x86_64, ARM64, RISC-V, MIPS, PPC, etc.)
//! 2. Choose Machine Type + CPU Model
//! 3. Configure Hardware (CPUs, RAM, disk, network)
//! 4. Review & Create

use crate::app::{ArchWizardStep, LibreVmmApp, Screen};
use crate::theme;
use crate::theme::{AppColors, BoxColors};
use eframe::egui;
use rust_i18n::t;
// Wave 16.A1: pure types come from vmm-types; the QemuArchIo extension trait
// (Linux-only filesystem helpers) still comes from vmm-core.
use vmm_core::qemu_archs::QemuArchIo;
use vmm_types::config::CpuTopology;
use vmm_types::qemu_archs::{BoxType, QemuArch};

/// Render the architecture wizard (Box 2: Hardware Lab).
pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui, step: &ArchWizardStep) {
    let accent = BoxColors::primary(&BoxType::HardwareLab);

    // Top accent stripe
    let stripe = ui.allocate_space(egui::vec2(ui.available_width(), 3.0));
    ui.painter().rect_filled(stripe.1, 0.0, accent);

    ui.add_space(theme::Spacing::MD);

    match step {
        ArchWizardStep::ChooseArch => render_choose_arch(app, ui, accent),
        ArchWizardStep::ChooseMachine => render_choose_machine(app, ui, accent),
        ArchWizardStep::Configure => render_configure(app, ui, accent),
        ArchWizardStep::Review => render_review(app, ui, accent),
    }
}

/// Step 1: Choose target architecture.
fn render_choose_arch(app: &mut LibreVmmApp, ui: &mut egui::Ui, accent: egui::Color32) {
    ui.horizontal(|ui| {
        ui.heading(egui::RichText::new(t!("arch.hardware-lab")).color(accent));
        ui.label(egui::RichText::new(t!("arch.choose-arch-sub")).color(AppColors::TEXT_DIM));
    });
    ui.add_space(theme::Spacing::SM);

    // Detect installed emulators
    let installed = vmm_core::qemu_archs::detect_installed_architectures();

    ui.label(egui::RichText::new(t!("arch.choose-arch-desc")).color(AppColors::TEXT_DIM));
    ui.add_space(theme::Spacing::XS);

    // Show filter: All / Installed Only
    ui.horizontal(|ui| {
        ui.label(t!("arch.show"));
        let show_all = app.arch_wizard_show_all();
        if ui
            .selectable_label(!show_all, t!("arch.installed-filter"))
            .clicked()
        {
            app.set_arch_wizard_show_all(false);
        }
        if ui
            .selectable_label(show_all, t!("arch.all-archs"))
            .clicked()
        {
            app.set_arch_wizard_show_all(true);
        }
    });
    ui.add_space(theme::Spacing::SM);

    // Architecture grid
    let nav_height = 48.0;
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - nav_height)
        .show(ui, |ui| {
            // Group by category
            let archs: Vec<QemuArch> = if app.arch_wizard_show_all() {
                QemuArch::all()
            } else {
                installed.clone()
            };

            let mut current_category = "";

            for arch in &archs {
                let cat = arch.category();
                if cat != current_category {
                    current_category = cat;
                    ui.add_space(theme::Spacing::SM);
                    ui.label(
                        egui::RichText::new(cat)
                            .color(accent)
                            .size(theme::FontSize::BODY),
                    );
                    ui.separator();
                }

                let is_installed = installed.iter().any(|a| a == arch);
                let is_selected = app.arch_wizard_arch() == Some(arch);

                let bg = if is_selected {
                    accent.linear_multiply(0.15)
                } else {
                    AppColors::BG_CARD
                };

                let frame = egui::Frame::none()
                    .fill(bg)
                    .rounding(egui::Rounding::same(theme::ThemeRounding::BUTTON))
                    .inner_margin(egui::Margin::same(10.0));

                let resp = frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Selection indicator
                        let dot_color = if is_selected {
                            accent
                        } else {
                            AppColors::MUTED
                        };
                        let (dot_rect, _) =
                            ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter()
                            .circle_filled(dot_rect.center(), 5.0, dot_color);

                        // Architecture name
                        ui.label(egui::RichText::new(arch.display_name()).strong().color(
                            if is_installed {
                                AppColors::TEXT
                            } else {
                                AppColors::TEXT_DIM
                            },
                        ));

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Bits badge
                            ui.label(
                                egui::RichText::new(t!("arch.bits", n = arch.bits()))
                                    .color(AppColors::MUTED)
                                    .size(theme::FontSize::SMALL),
                            );

                            // Installation status
                            if is_installed {
                                ui.label(
                                    egui::RichText::new(t!("arch.installed"))
                                        .color(AppColors::SUCCESS)
                                        .size(theme::FontSize::SMALL),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new(t!("arch.not-installed"))
                                        .color(AppColors::WARNING)
                                        .size(theme::FontSize::SMALL),
                                );
                            }

                            // KVM badge for same-arch
                            if arch.can_use_kvm_on_x86() {
                                ui.label(
                                    egui::RichText::new("KVM")
                                        .color(AppColors::SUCCESS)
                                        .size(theme::FontSize::SMALL)
                                        .strong(),
                                );
                            }
                        });
                    });
                });

                if resp.response.interact(egui::Sense::click()).clicked() {
                    app.set_arch_wizard_arch(arch.clone());
                }
            }
        });

    ui.add_space(theme::Spacing::MD);

    // Navigation buttons
    ui.horizontal(|ui| {
        if ui.button(t!("arch.cancel")).clicked() {
            app.set_screen(Screen::Home);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let has_selection = app.arch_wizard_arch().is_some();
            let next_btn = ui.add_enabled(
                has_selection,
                egui::Button::new(egui::RichText::new(t!("arch.next-machine")).color(
                    if has_selection {
                        egui::Color32::WHITE
                    } else {
                        AppColors::MUTED
                    },
                ))
                .fill(if has_selection {
                    accent
                } else {
                    AppColors::BG_CARD
                }),
            );
            if next_btn.clicked() {
                app.set_screen(Screen::ArchWizard(ArchWizardStep::ChooseMachine));
            }
        });
    });
}

/// Step 2: Choose machine type and CPU model.
fn render_choose_machine(app: &mut LibreVmmApp, ui: &mut egui::Ui, accent: egui::Color32) {
    let arch = match app.arch_wizard_arch() {
        Some(a) => a.clone(),
        None => {
            app.set_screen(Screen::ArchWizard(ArchWizardStep::ChooseArch));
            return;
        },
    };

    ui.horizontal(|ui| {
        ui.heading(egui::RichText::new(t!("arch.hardware-lab")).color(accent));
        ui.label(
            egui::RichText::new(t!("arch.machine-cpu-sub", arch = arch.display_name()))
                .color(AppColors::TEXT_DIM),
        );
    });
    ui.add_space(theme::Spacing::MD);

    let nav_height = 48.0;
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - nav_height)
        .show(ui, |ui| {
            // Machine types
            ui.label(
                egui::RichText::new(t!("arch.machine-type"))
                    .color(accent)
                    .size(theme::FontSize::SUBHEADING)
                    .strong(),
            );
            ui.add_space(theme::Spacing::XS);
            ui.label(
                egui::RichText::new(t!("arch.machine-type-desc"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::LABEL),
            );
            ui.add_space(theme::Spacing::SM);

            let machines = arch.machine_types();
            for machine in &machines {
                let is_selected = app.arch_wizard_machine() == machine.id;
                let bg = if is_selected {
                    accent.linear_multiply(0.15)
                } else {
                    AppColors::BG_CARD
                };

                let frame = egui::Frame::none()
                    .fill(bg)
                    .rounding(egui::Rounding::same(theme::ThemeRounding::BUTTON))
                    .inner_margin(egui::Margin::same(8.0));

                let resp = frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let dot_color = if is_selected {
                            accent
                        } else {
                            AppColors::MUTED
                        };
                        let (dot_rect, _) =
                            ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter()
                            .circle_filled(dot_rect.center(), 5.0, dot_color);

                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&machine.id)
                                        .strong()
                                        .color(AppColors::TEXT)
                                        .family(egui::FontFamily::Monospace),
                                );
                                if machine.is_default {
                                    ui.label(
                                        egui::RichText::new(t!("arch.recommended"))
                                            .color(accent)
                                            .size(theme::FontSize::SMALL),
                                    );
                                }
                            });
                            ui.label(
                                egui::RichText::new(&machine.description)
                                    .color(AppColors::TEXT_DIM)
                                    .size(theme::FontSize::LABEL),
                            );
                        });
                    });
                });

                if resp.response.interact(egui::Sense::click()).clicked() {
                    app.set_arch_wizard_machine(machine.id.clone());
                }
            }

            ui.add_space(theme::Spacing::LG);

            // CPU model
            ui.label(
                egui::RichText::new(t!("arch.cpu-model"))
                    .color(accent)
                    .size(theme::FontSize::SUBHEADING)
                    .strong(),
            );
            ui.add_space(theme::Spacing::XS);
            ui.label(
                egui::RichText::new(t!("arch.cpu-model-desc"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::LABEL),
            );
            ui.add_space(theme::Spacing::SM);

            let cpus = arch.cpu_models();
            for cpu in &cpus {
                let is_selected = app.arch_wizard_cpu() == cpu.id;
                let bg = if is_selected {
                    accent.linear_multiply(0.15)
                } else {
                    AppColors::BG_CARD
                };

                let frame = egui::Frame::none()
                    .fill(bg)
                    .rounding(egui::Rounding::same(theme::ThemeRounding::BUTTON))
                    .inner_margin(egui::Margin::same(8.0));

                let resp = frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let dot_color = if is_selected {
                            accent
                        } else {
                            AppColors::MUTED
                        };
                        let (dot_rect, _) =
                            ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter()
                            .circle_filled(dot_rect.center(), 5.0, dot_color);

                        ui.label(
                            egui::RichText::new(&cpu.id)
                                .strong()
                                .color(AppColors::TEXT)
                                .family(egui::FontFamily::Monospace),
                        );
                        ui.label(
                            egui::RichText::new(&format!("  {}", cpu.description))
                                .color(AppColors::TEXT_DIM)
                                .size(theme::FontSize::LABEL),
                        );
                    });
                });

                if resp.response.interact(egui::Sense::click()).clicked() {
                    app.set_arch_wizard_cpu(cpu.id.clone());
                }
            }

            ui.add_space(theme::Spacing::MD);

            // KVM toggle (only for same-arch)
            if arch.can_use_kvm_on_x86() {
                ui.horizontal(|ui| {
                    let mut use_kvm = app.arch_wizard_use_kvm();
                    ui.checkbox(&mut use_kvm, t!("arch.use-kvm"));
                    app.set_arch_wizard_use_kvm(use_kvm);
                });
                ui.label(
                    egui::RichText::new(t!("arch.kvm-desc"))
                        .color(AppColors::TEXT_DIM)
                        .size(theme::FontSize::SMALL),
                );
            } else {
                ui.label(
                    egui::RichText::new(t!("arch.tcg-warning", arch = arch.display_name()))
                        .color(AppColors::WARNING)
                        .size(theme::FontSize::LABEL),
                );
            }
        });

    ui.add_space(theme::Spacing::MD);

    // Navigation
    ui.horizontal(|ui| {
        if ui.button(t!("arch.back")).clicked() {
            app.set_screen(Screen::ArchWizard(ArchWizardStep::ChooseArch));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let next_btn = ui.add(
                egui::Button::new(
                    egui::RichText::new(t!("arch.next-hardware")).color(egui::Color32::WHITE),
                )
                .fill(accent),
            );
            if next_btn.clicked() {
                // Apply architecture defaults
                app.apply_arch_defaults();
                app.set_screen(Screen::ArchWizard(ArchWizardStep::Configure));
            }
        });
    });
}

/// Section header helper.
fn section_header(ui: &mut egui::Ui, title: impl Into<String>, accent: egui::Color32) {
    ui.add_space(theme::Spacing::XS);
    ui.label(
        egui::RichText::new(title)
            .color(accent)
            .size(theme::FontSize::SUBHEADING)
            .strong(),
    );
    ui.add_space(theme::Spacing::XS);
}

/// Step 3: Configure hardware (CPUs, RAM, disk, network, topology, features).
fn render_configure(app: &mut LibreVmmApp, ui: &mut egui::Ui, accent: egui::Color32) {
    let arch = match app.arch_wizard_arch() {
        Some(a) => a.clone(),
        None => {
            app.set_screen(Screen::ArchWizard(ArchWizardStep::ChooseArch));
            return;
        },
    };

    ui.horizontal(|ui| {
        ui.heading(egui::RichText::new(t!("arch.hardware-lab")).color(accent));
        ui.label(
            egui::RichText::new(t!("arch.configure-sub", arch = arch.display_name()))
                .color(AppColors::TEXT_DIM),
        );
    });
    ui.add_space(theme::Spacing::MD);

    let nav_height = 48.0;
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - nav_height)
        .show(ui, |ui| {
            // VM Name
            section_header(ui, t!("arch.vm-name"), accent);
            ui.text_edit_singleline(app.wizard_name_mut());
            ui.add_space(theme::Spacing::SM);

            // ── CPU Configuration ────────────────────────────────────────
            section_header(ui, t!("arch.processor"), accent);

            // vCPU count
            let max_cpus = arch.max_cpus();
            ui.label(
                egui::RichText::new(t!("arch.vcpus-max", max = max_cpus))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::LABEL),
            );
            let mut cpus = app.wizard_cpus() as f64;
            ui.add(egui::Slider::new(&mut cpus, 1.0..=(max_cpus as f64)).integer());
            // SECURITY: CWE-681 — Clamp f64 before narrowing to u32.
            app.set_wizard_cpus(cpus.clamp(1.0, max_cpus as f64) as u32);
            ui.add_space(6.0);

            // CPU Topology (sockets × cores × threads)
            if arch.supports_smp_topology() && max_cpus > 1 {
                let has_topology = app.arch_cpu_topology().is_some();
                let mut enable_topology = has_topology;
                ui.checkbox(&mut enable_topology, t!("arch.custom-topology"));
                ui.label(
                    egui::RichText::new(t!("arch.custom-topology-desc"))
                        .color(AppColors::TEXT_DIM)
                        .size(theme::FontSize::SMALL),
                );

                if enable_topology && !has_topology {
                    // Initialize with a sensible default
                    let vcpus = app.wizard_cpus();
                    let topo = if vcpus >= 4 {
                        CpuTopology {
                            sockets: 1,
                            cores: vcpus,
                            threads: 1,
                        }
                    } else {
                        CpuTopology {
                            sockets: 1,
                            cores: vcpus.max(1),
                            threads: 1,
                        }
                    };
                    app.set_arch_cpu_topology(Some(topo));
                } else if !enable_topology && has_topology {
                    app.set_arch_cpu_topology(None);
                }

                if enable_topology {
                    ui.add_space(theme::Spacing::XS);

                    // Topology presets
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(t!("arch.presets"))
                                .color(AppColors::TEXT_DIM)
                                .size(theme::FontSize::LABEL),
                        );
                        for (label, preset) in CpuTopology::presets() {
                            if preset.total_vcpus() <= max_cpus {
                                let btn = ui.small_button(label);
                                if btn.clicked() {
                                    app.set_wizard_cpus(preset.total_vcpus());
                                    app.set_arch_cpu_topology(Some(preset));
                                }
                            }
                        }
                    });

                    ui.add_space(theme::Spacing::XS);

                    // Manual sliders
                    let topo = app.arch_cpu_topology().cloned().unwrap_or(CpuTopology {
                        sockets: 1,
                        cores: app.wizard_cpus(),
                        threads: 1,
                    });
                    let mut sockets = topo.sockets as f64;
                    let mut cores = topo.cores as f64;
                    let mut threads = topo.threads as f64;

                    egui::Grid::new("arch_topo_grid")
                        .num_columns(2)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(t!("arch.sockets"))
                                    .color(AppColors::TEXT_DIM)
                                    .size(theme::FontSize::LABEL),
                            );
                            ui.add(egui::Slider::new(&mut sockets, 1.0..=8.0).integer());
                            ui.end_row();

                            ui.label(
                                egui::RichText::new(t!("arch.cores-per-socket"))
                                    .color(AppColors::TEXT_DIM)
                                    .size(theme::FontSize::LABEL),
                            );
                            ui.add(egui::Slider::new(&mut cores, 1.0..=64.0).integer());
                            ui.end_row();

                            ui.label(
                                egui::RichText::new(t!("arch.threads-per-core"))
                                    .color(AppColors::TEXT_DIM)
                                    .size(theme::FontSize::LABEL),
                            );
                            ui.add(egui::Slider::new(&mut threads, 1.0..=4.0).integer());
                            ui.end_row();
                        });

                    // SECURITY: CWE-681 — Clamp f64 slider values before narrowing to u32.
                    let new_topo = CpuTopology {
                        sockets: sockets.clamp(1.0, 8.0) as u32,
                        cores: cores.clamp(1.0, 64.0) as u32,
                        threads: threads.clamp(1.0, 4.0) as u32,
                    };
                    let total = new_topo.total_vcpus();

                    // Show topology total
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(t!(
                                "arch.topo-total",
                                total = total,
                                s = new_topo.sockets,
                                c = new_topo.cores,
                                t = new_topo.threads
                            ))
                            .color(if total <= max_cpus {
                                AppColors::TEXT
                            } else {
                                AppColors::DANGER
                            })
                            .size(theme::FontSize::LABEL),
                        );
                    });

                    if total > max_cpus {
                        ui.label(
                            egui::RichText::new(t!(
                                "arch.topo-exceeds",
                                total = total,
                                max = max_cpus,
                                arch = arch.display_name()
                            ))
                            .color(AppColors::WARNING)
                            .size(theme::FontSize::SMALL),
                        );
                    }

                    // Sync vCPU count from topology
                    if total != app.wizard_cpus() && total <= max_cpus {
                        app.set_wizard_cpus(total);
                    }
                    app.set_arch_cpu_topology(Some(new_topo));
                }
            }

            ui.add_space(theme::Spacing::SM);

            // ── CPU Feature Flags ────────────────────────────────────────
            let features = arch.cpu_features();
            if !features.is_empty() {
                section_header(ui, t!("arch.cpu-features"), accent);
                ui.label(
                    egui::RichText::new(t!("arch.cpu-features-desc"))
                        .color(AppColors::TEXT_DIM)
                        .size(theme::FontSize::SMALL),
                );
                ui.add_space(theme::Spacing::XS);

                // Group features by category
                let mut groups: Vec<String> = Vec::new();
                for f in &features {
                    if !groups.contains(&f.group) {
                        groups.push(f.group.clone());
                    }
                }

                for group in &groups {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(group)
                                .color(accent.linear_multiply(0.8))
                                .size(theme::FontSize::LABEL)
                                .strong(),
                        );
                    });

                    let group_features: Vec<_> =
                        features.iter().filter(|f| &f.group == group).collect();

                    // Render features in a compact grid (2 columns)
                    egui::Grid::new(format!("cpu_feat_{}", group))
                        .num_columns(2)
                        .spacing([16.0, 2.0])
                        .show(ui, |ui| {
                            for (i, feat) in group_features.iter().enumerate() {
                                let mut enabled = app.has_arch_cpu_feature(&feat.name);
                                let label = if feat.default_on {
                                    format!("{} *", feat.name)
                                } else {
                                    feat.name.clone()
                                };
                                if ui.checkbox(&mut enabled, "").changed() {
                                    app.toggle_arch_cpu_feature(&feat.name);
                                }
                                ui.label(
                                    egui::RichText::new(&label)
                                        .color(AppColors::TEXT)
                                        .size(theme::FontSize::LABEL)
                                        .family(egui::FontFamily::Monospace),
                                )
                                .on_hover_text(&feat.description);
                                if i % 2 == 1 {
                                    ui.end_row();
                                }
                            }
                            // Close the last row if odd number of features
                            if group_features.len() % 2 == 1 {
                                ui.label(""); // fill empty cell
                                ui.end_row();
                            }
                        });

                    ui.add_space(theme::Spacing::XS);
                }

                // Show enabled count
                let enabled_count = app.arch_cpu_features().len();
                if enabled_count > 0 {
                    ui.label(
                        egui::RichText::new(t!("arch.features-enabled", n = enabled_count))
                            .color(accent)
                            .size(theme::FontSize::SMALL),
                    );
                }
            }

            ui.add_space(theme::Spacing::SM);

            // ── Memory ───────────────────────────────────────────────────
            section_header(ui, t!("arch.memory"), accent);
            let mut mem = app.wizard_memory_mib() as f64;
            let max_mem = if arch.bits() >= 64 { 65536.0 } else { 4096.0 };
            ui.add(
                egui::Slider::new(&mut mem, 64.0..=max_mem)
                    .integer()
                    .suffix(" MiB"),
            );
            app.set_wizard_memory_mib(mem as u64);
            ui.add_space(theme::Spacing::SM);

            // ── Storage ──────────────────────────────────────────────────
            section_header(ui, t!("arch.storage"), accent);
            ui.label(
                egui::RichText::new(t!("arch.disk-size"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::LABEL),
            );
            let mut disk = app.wizard_disk_gib() as f64;
            ui.add(
                egui::Slider::new(&mut disk, 1.0..=500.0)
                    .integer()
                    .suffix(" GiB"),
            );
            app.set_wizard_disk_gib(disk as u64);
            ui.add_space(theme::Spacing::XS);

            // Boot ISO
            ui.label(
                egui::RichText::new(t!("arch.boot-iso"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::LABEL),
            );
            ui.horizontal(|ui| {
                ui.text_edit_singleline(app.wizard_iso_mut());
                if ui.button(t!("arch.browse")).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ISO Images", &["iso", "img"])
                        .pick_file()
                    {
                        *app.wizard_iso_mut() = path.display().to_string();
                    }
                }
            });
            ui.add_space(theme::Spacing::SM);

            // ── Network ──────────────────────────────────────────────────
            section_header(ui, t!("arch.network"), accent);
            let mut net_idx = match app.wizard_network() {
                vmm_core::config::NetworkMode::Nat => 0,
                vmm_core::config::NetworkMode::Bridged => 1,
                vmm_core::config::NetworkMode::HostOnly => 2,
                vmm_core::config::NetworkMode::LanSegment(_) => 4,
                vmm_core::config::NetworkMode::None => 3,
            };
            egui::ComboBox::from_id_salt("arch_wizard_network")
                .selected_text(match net_idx {
                    0 => t!("wizard.net-nat-short"),
                    1 => t!("wizard.net-bridged"),
                    2 => t!("wizard.net-host-only"),
                    _ => t!("wizard.net-none"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut net_idx, 0, t!("wizard.net-nat-short"));
                    ui.selectable_value(&mut net_idx, 1, t!("wizard.net-bridged"));
                    ui.selectable_value(&mut net_idx, 2, t!("wizard.net-host-only"));
                    ui.selectable_value(&mut net_idx, 3, t!("wizard.net-none"));
                });
            app.set_wizard_network(match net_idx {
                0 => vmm_core::config::NetworkMode::Nat,
                1 => vmm_core::config::NetworkMode::Bridged,
                2 => vmm_core::config::NetworkMode::HostOnly,
                _ => vmm_core::config::NetworkMode::None,
            });
            ui.add_space(theme::Spacing::SM);

            // ── Architecture Capabilities ────────────────────────────────
            ui.separator();
            section_header(ui, t!("arch.capabilities"), accent);

            let capabilities = [
                (t!("arch.cap-uefi"), arch.has_uefi_support()),
                (t!("arch.cap-virtio"), arch.has_virtio_support()),
                (t!("arch.cap-usb"), arch.has_usb_support()),
                (t!("arch.cap-audio"), arch.has_audio_support()),
                (t!("arch.cap-spice"), arch.has_spice_support()),
                (t!("arch.cap-kvm"), arch.can_use_kvm_on_x86()),
                (t!("arch.cap-smp"), arch.supports_smp_topology()),
                (t!("arch.cap-cpu-features"), arch.has_cpu_features()),
            ];

            egui::Grid::new("arch_caps").num_columns(2).show(ui, |ui| {
                for (name, supported) in &capabilities {
                    ui.label(
                        egui::RichText::new(name.as_ref())
                            .color(AppColors::TEXT_DIM)
                            .size(theme::FontSize::LABEL),
                    );
                    if *supported {
                        ui.label(
                            egui::RichText::new(t!("arch.yes"))
                                .color(AppColors::SUCCESS)
                                .size(theme::FontSize::LABEL),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new(t!("arch.no"))
                                .color(AppColors::MUTED)
                                .size(theme::FontSize::LABEL),
                        );
                    }
                    ui.end_row();
                }
            });

            // Description
            ui.add_space(theme::Spacing::SM);
            section_header(ui, t!("arch.description-optional"), accent);
            ui.text_edit_multiline(app.wizard_description_mut());
        });

    ui.add_space(theme::Spacing::MD);

    // Navigation
    ui.horizontal(|ui| {
        if ui.button(t!("arch.back")).clicked() {
            app.set_screen(Screen::ArchWizard(ArchWizardStep::ChooseMachine));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_create = !app.wizard_name().is_empty();
            let next_btn = ui.add_enabled(
                can_create,
                egui::Button::new(egui::RichText::new(t!("arch.next-review")).color(
                    if can_create {
                        egui::Color32::WHITE
                    } else {
                        AppColors::MUTED
                    },
                ))
                .fill(if can_create {
                    accent
                } else {
                    AppColors::BG_CARD
                }),
            );
            if next_btn.clicked() {
                app.set_screen(Screen::ArchWizard(ArchWizardStep::Review));
            }
        });
    });
}

/// Step 4: Review and create.
fn render_review(app: &mut LibreVmmApp, ui: &mut egui::Ui, accent: egui::Color32) {
    let arch = match app.arch_wizard_arch() {
        Some(a) => a.clone(),
        None => {
            app.set_screen(Screen::ArchWizard(ArchWizardStep::ChooseArch));
            return;
        },
    };

    ui.horizontal(|ui| {
        ui.heading(egui::RichText::new(t!("arch.hardware-lab")).color(accent));
        ui.label(egui::RichText::new(t!("arch.review-create-sub")).color(AppColors::TEXT_DIM));
    });
    ui.add_space(theme::Spacing::MD);

    let nav_height = 48.0;
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - nav_height)
        .show(ui, |ui| {
            // Summary card
            let frame = egui::Frame::none()
                .fill(AppColors::BG_CARD)
                .rounding(egui::Rounding::same(theme::ThemeRounding::CARD))
                .inner_margin(egui::Margin::same(16.0));

            frame.show(ui, |ui| {
                ui.heading(egui::RichText::new(app.wizard_name()).color(AppColors::TEXT));
                ui.add_space(theme::Spacing::SM);

                egui::Grid::new("review_grid")
                    .num_columns(2)
                    .spacing([20.0, 6.0])
                    .show(ui, |ui| {
                        review_row(ui, t!("arch.review-box"), t!("arch.hardware-lab"), accent);
                        review_row(
                            ui,
                            t!("arch.review-arch"),
                            arch.display_name().to_string(),
                            accent,
                        );
                        review_row(
                            ui,
                            t!("arch.review-machine"),
                            app.arch_wizard_machine().to_string(),
                            accent,
                        );
                        review_row(
                            ui,
                            t!("arch.review-cpu"),
                            app.arch_wizard_cpu().to_string(),
                            accent,
                        );
                        review_row(
                            ui,
                            t!("arch.review-emulation"),
                            if app.arch_wizard_use_kvm() {
                                t!("summary.kvm-hardware").into_owned()
                            } else {
                                t!("summary.tcg-software").into_owned()
                            },
                            accent,
                        );
                        review_row(
                            ui,
                            t!("arch.review-vcpus"),
                            app.wizard_cpus().to_string(),
                            accent,
                        );

                        // CPU Topology
                        if let Some(topo) = app.arch_cpu_topology() {
                            review_row(ui, t!("arch.review-topology"), format!("{}", topo), accent);
                        }

                        // CPU Features
                        let features = app.arch_cpu_features();
                        if !features.is_empty() {
                            let feat_str = features.join(", ");
                            review_row(ui, t!("arch.review-features"), feat_str, accent);
                        }

                        review_row(
                            ui,
                            t!("arch.review-memory"),
                            format!("{} MiB", app.wizard_memory_mib()),
                            accent,
                        );
                        review_row(
                            ui,
                            t!("arch.review-disk"),
                            format!("{} GiB", app.wizard_disk_gib()),
                            accent,
                        );
                        review_row(
                            ui,
                            t!("arch.review-network"),
                            app.wizard_network().to_string(),
                            accent,
                        );

                        if !app.wizard_iso().is_empty() {
                            review_row(
                                ui,
                                t!("arch.review-iso"),
                                app.wizard_iso().to_string(),
                                accent,
                            );
                        }

                        review_row(
                            ui,
                            t!("arch.cap-virtio"),
                            if arch.has_virtio_support() {
                                t!("arch.yes").into_owned()
                            } else {
                                t!("arch.no").into_owned()
                            },
                            accent,
                        );
                        review_row(
                            ui,
                            t!("arch.cap-usb"),
                            if arch.has_usb_support() {
                                t!("arch.yes").into_owned()
                            } else {
                                t!("arch.no").into_owned()
                            },
                            accent,
                        );
                        review_row(
                            ui,
                            t!("arch.cap-uefi"),
                            if arch.has_uefi_support() {
                                t!("arch.available").into_owned()
                            } else {
                                t!("arch.na").into_owned()
                            },
                            accent,
                        );
                    });

                if !app.wizard_description().is_empty() {
                    ui.add_space(theme::Spacing::SM);
                    ui.label(
                        egui::RichText::new(t!("arch.description-label"))
                            .color(AppColors::TEXT_DIM),
                    );
                    ui.label(app.wizard_description());
                }
            });

            // QEMU binary path
            ui.add_space(theme::Spacing::SM);
            ui.label(
                egui::RichText::new(t!("arch.qemu-binary", bin = arch.qemu_binary()))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::SMALL)
                    .family(egui::FontFamily::Monospace),
            );

            if !arch.is_binary_available() {
                ui.add_space(theme::Spacing::XS);
                let pkg = if arch.qemu_suffix().contains("arm")
                    || arch.qemu_suffix().contains("aarch64")
                {
                    "arm"
                } else {
                    arch.qemu_suffix()
                };
                ui.label(
                    egui::RichText::new(t!(
                        "arch.qemu-not-installed",
                        bin = arch.qemu_binary(),
                        pkg = pkg
                    ))
                    .color(AppColors::WARNING)
                    .size(theme::FontSize::LABEL),
                );
            }
        });

    ui.add_space(theme::Spacing::MD);

    // Navigation
    ui.horizontal(|ui| {
        if ui.button(t!("arch.back")).clicked() {
            app.set_screen(Screen::ArchWizard(ArchWizardStep::Configure));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let create_btn = ui.add(
                egui::Button::new(
                    egui::RichText::new(t!("arch.create-vm"))
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(accent)
                .min_size(egui::vec2(120.0, 32.0)),
            );
            if create_btn.clicked() {
                app.action_create_arch_vm();
            }
        });
    });
}

/// Helper to render a review row.
fn review_row(
    ui: &mut egui::Ui,
    label: impl Into<String>,
    value: impl Into<String>,
    accent: egui::Color32,
) {
    ui.label(
        egui::RichText::new(label)
            .color(AppColors::TEXT_DIM)
            .size(theme::FontSize::BODY),
    );
    ui.label(
        egui::RichText::new(value)
            .color(AppColors::TEXT)
            .size(theme::FontSize::BODY),
    );
    let _ = accent; // Silence unused warning
    ui.end_row();
}
