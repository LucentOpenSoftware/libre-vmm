//! Power User wizard — Box 3 VM creation with advanced hardware controls.
//!
//! A multi-step wizard for creating performance-tuned VMs:
//! 1. Choose Template (same OS templates + Power User defaults)
//! 2. CPU & Memory (topology, pinning, hugepages)
//! 3. Storage & Passthrough (disk cache, I/O threads, VFIO devices)
//! 4. Network & Extras (multi-NIC, port forwards, custom QEMU args)
//! 5. Review & Create

use crate::app::{LibreVmmApp, PowerWizardStep, Screen};
use crate::theme;
use crate::theme::{AppColors, BoxColors};
use eframe::egui;
use rust_i18n::t;
use vmm_core::config::{CpuTopology, NetworkMode, VfioDeviceConfig};
use vmm_core::qemu_archs::BoxType;
use vmm_core::resource_limits;
use vmm_core::template::builtin_templates;

/// Render the Power User wizard.
pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui, step: &PowerWizardStep) {
    let accent = BoxColors::primary(&BoxType::PowerUser);

    // Top accent stripe
    let stripe = ui.allocate_space(egui::vec2(ui.available_width(), 3.0));
    ui.painter().rect_filled(stripe.1, 0.0, accent);

    ui.add_space(theme::Spacing::SM);

    // Step indicator
    render_step_indicator(ui, step, accent);

    ui.add_space(theme::Spacing::SM);
    ui.separator();
    ui.add_space(theme::Spacing::SM);

    match step {
        PowerWizardStep::ChooseTemplate => render_step_template(app, ui, accent),
        PowerWizardStep::CpuMemory => render_step_cpu_memory(app, ui, accent),
        PowerWizardStep::StoragePassthrough => render_step_storage(app, ui, accent),
        PowerWizardStep::NetworkExtras => render_step_network(app, ui, accent),
        PowerWizardStep::Review => render_step_review(app, ui, accent),
    }
}

fn render_step_indicator(ui: &mut egui::Ui, current: &PowerWizardStep, accent: egui::Color32) {
    let steps = [
        (t!("power-wizard.step1"), PowerWizardStep::ChooseTemplate),
        (t!("power-wizard.step2"), PowerWizardStep::CpuMemory),
        (
            t!("power-wizard.step3"),
            PowerWizardStep::StoragePassthrough,
        ),
        (t!("power-wizard.step4"), PowerWizardStep::NetworkExtras),
        (t!("power-wizard.step5"), PowerWizardStep::Review),
    ];

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t!("power-wizard.title"))
                .color(accent)
                .strong()
                .size(theme::FontSize::SUBHEADING),
        );
        ui.label(egui::RichText::new("  |  ").color(AppColors::MUTED));

        for (i, (label, step)) in steps.iter().enumerate() {
            let is_active = current == step;
            let color = if is_active {
                accent
            } else {
                AppColors::TEXT_DIM
            };
            ui.label(egui::RichText::new(label.as_ref()).color(color).strong());
            if i < steps.len() - 1 {
                ui.label(egui::RichText::new(" › ").color(AppColors::MUTED));
            }
        }
    });
}

// ===================== Step 1: Choose Template =====================

fn render_step_template(app: &mut LibreVmmApp, ui: &mut egui::Ui, accent: egui::Color32) {
    ui.heading(t!("power-wizard.choose-os"));
    ui.label(egui::RichText::new(t!("power-wizard.choose-os-desc")).color(AppColors::TEXT_DIM));
    ui.add_space(theme::Spacing::SM);

    let templates = builtin_templates();
    let selected_idx = app.wizard_template_idx();

    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 60.0)
        .show(ui, |ui| {
            egui::Grid::new("power_template_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    for (i, template) in templates.iter().enumerate() {
                        let is_selected = i == selected_idx;
                        let fill = if is_selected {
                            accent.linear_multiply(0.2)
                        } else {
                            AppColors::BG_CARD
                        };
                        let stroke = if is_selected {
                            egui::Stroke::new(1.5, accent)
                        } else {
                            egui::Stroke::new(0.5, egui::Color32::from_rgb(55, 60, 75))
                        };

                        let frame = egui::Frame::none()
                            .fill(fill)
                            .rounding(theme::ThemeRounding::CARD)
                            .stroke(stroke)
                            .inner_margin(theme::Spacing::MD);

                        frame.show(ui, |ui| {
                            ui.set_min_width(280.0);
                            let response = ui
                                .vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(template.label)
                                            .size(theme::FontSize::SUBHEADING)
                                            .strong()
                                            .color(AppColors::TEXT),
                                    );
                                    ui.label(
                                        egui::RichText::new(template.description)
                                            .size(theme::FontSize::LABEL)
                                            .color(AppColors::TEXT_DIM),
                                    );
                                    ui.label(
                                        egui::RichText::new(t!(
                                            "wizard.specs-format",
                                            cpus = template.recommended_cpus,
                                            memory = template.recommended_memory_mib,
                                            disk = template.recommended_disk_gib,
                                        ))
                                        .size(theme::FontSize::SMALL)
                                        .color(AppColors::MUTED),
                                    );
                                })
                                .response;

                            if response.interact(egui::Sense::click()).clicked() {
                                app.action_apply_template(i);
                            }
                        });

                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
        });

    ui.add_space(theme::Spacing::MD);

    // Navigation
    ui.horizontal(|ui| {
        if ui.button(t!("power-wizard.cancel")).clicked() {
            app.set_screen(Screen::Home);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let next = egui::Button::new(
                egui::RichText::new(t!("power-wizard.next-cpu-memory")).color(egui::Color32::WHITE),
            )
            .fill(accent);
            if ui.add(next).clicked() {
                app.set_screen(Screen::PowerWizard(PowerWizardStep::CpuMemory));
            }
        });
    });
}

// ===================== Step 2: CPU & Memory =====================

fn render_step_cpu_memory(app: &mut LibreVmmApp, ui: &mut egui::Ui, accent: egui::Color32) {
    let host_cpus = resource_limits::host_cpu_count();

    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 60.0)
        .show(ui, |ui| {
            // VM Name
            ui.label(
                egui::RichText::new(t!("power-wizard.vm-name"))
                    .color(accent)
                    .strong(),
            );
            ui.text_edit_singleline(app.wizard_name_mut());
            ui.add_space(theme::Spacing::MD);

            // === CPU Section ===
            section_header(ui, t!("power-wizard.cpu-config"), accent);

            // CPU count
            ui.label(
                egui::RichText::new(t!("power-wizard.vcpus-host", n = host_cpus))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::LABEL),
            );
            let mut cpus = app.wizard_cpus() as f64;
            ui.add(egui::Slider::new(&mut cpus, 1.0..=(host_cpus as f64).min(128.0)).integer());
            // SECURITY: CWE-681 — Clamp f64 before narrowing to u32 to prevent UB on out-of-range values.
            app.set_wizard_cpus(cpus.clamp(1.0, 128.0) as u32);
            ui.add_space(theme::Spacing::SM);

            // CPU Topology
            ui.label(
                egui::RichText::new(t!("power-wizard.cpu-topology"))
                    .color(accent)
                    .strong(),
            );
            ui.label(
                egui::RichText::new(t!("power-wizard.cpu-topology-desc"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::LABEL),
            );

            let has_topology = app.power_cpu_topology().is_some();
            let mut enable_topo = has_topology;
            ui.checkbox(&mut enable_topo, t!("power-wizard.custom-topology"));

            if enable_topo && !has_topology {
                // Initialize with a sensible default
                let vcpus = app.wizard_cpus();
                app.set_power_cpu_topology(Some(CpuTopology {
                    sockets: 1,
                    cores: vcpus,
                    threads: 1,
                }));
            } else if !enable_topo && has_topology {
                app.set_power_cpu_topology(None);
            }

            if enable_topo {
                if let Some(mut topo) = app.power_cpu_topology().cloned() {
                    egui::Grid::new("cpu_topo_grid")
                        .num_columns(2)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            // SECURITY: CWE-681 — Clamp f64→u32 casts to prevent UB on edge values.
                            ui.label(t!("power-wizard.sockets"));
                            let mut s = topo.sockets as f64;
                            ui.add(egui::Slider::new(&mut s, 1.0..=8.0).integer());
                            topo.sockets = s.clamp(1.0, 8.0) as u32;
                            ui.end_row();

                            ui.label(t!("power-wizard.cores-per-socket"));
                            let mut c = topo.cores as f64;
                            ui.add(egui::Slider::new(&mut c, 1.0..=64.0).integer());
                            topo.cores = c.clamp(1.0, 64.0) as u32;
                            ui.end_row();

                            ui.label(t!("power-wizard.threads-per-core"));
                            let mut t = topo.threads as f64;
                            ui.add(egui::Slider::new(&mut t, 1.0..=4.0).integer());
                            topo.threads = t.clamp(1.0, 4.0) as u32;
                            ui.end_row();
                        });

                    let total = topo.total_vcpus();
                    ui.label(
                        egui::RichText::new(t!(
                            "power-wizard.topo-total",
                            total = total,
                            topo = topo.to_string()
                        ))
                        .color(if total == app.wizard_cpus() {
                            AppColors::SUCCESS
                        } else {
                            AppColors::WARNING
                        })
                        .size(theme::FontSize::LABEL),
                    );
                    if total != app.wizard_cpus() {
                        ui.label(
                            egui::RichText::new(t!(
                                "power-wizard.topo-mismatch",
                                slider = app.wizard_cpus(),
                                topo = total
                            ))
                            .color(AppColors::WARNING)
                            .size(theme::FontSize::SMALL),
                        );
                    }

                    app.set_power_cpu_topology(Some(topo));
                }

                // Quick presets
                ui.add_space(theme::Spacing::XS);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(t!("power-wizard.presets"))
                            .color(AppColors::TEXT_DIM)
                            .size(theme::FontSize::LABEL),
                    );
                    for (label, preset) in CpuTopology::presets() {
                        if ui.small_button(label).clicked() {
                            app.set_wizard_cpus(preset.total_vcpus());
                            app.set_power_cpu_topology(Some(preset));
                        }
                    }
                });
            }

            ui.add_space(theme::Spacing::MD);

            // === Memory Section ===
            section_header(ui, t!("power-wizard.memory-config"), accent);

            let mut mem = app.wizard_memory_mib() as f64;
            ui.add(
                egui::Slider::new(&mut mem, 512.0..=131072.0)
                    .integer()
                    .suffix(" MiB")
                    .step_by(512.0),
            );
            app.set_wizard_memory_mib(mem as u64);
            ui.label(
                egui::RichText::new(format!("{:.1} GiB", mem / 1024.0))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::LABEL),
            );
            ui.add_space(theme::Spacing::SM);

            // Hugepages
            let mut hugepages = app.power_hugepages();
            ui.checkbox(&mut hugepages, t!("power-wizard.hugepages"));
            app.set_power_hugepages(hugepages);
            ui.label(
                egui::RichText::new(t!("power-wizard.hugepages-desc"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::SMALL),
            );

            ui.add_space(theme::Spacing::SM);

            // Description
            ui.label(
                egui::RichText::new(t!("power-wizard.description-optional"))
                    .color(accent)
                    .strong(),
            );
            ui.text_edit_multiline(app.wizard_description_mut());
        });

    ui.add_space(theme::Spacing::MD);

    // Navigation
    ui.horizontal(|ui| {
        if ui.button(t!("power-wizard.back")).clicked() {
            app.set_screen(Screen::PowerWizard(PowerWizardStep::ChooseTemplate));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_next = !app.wizard_name().is_empty();
            let next = egui::Button::new(
                egui::RichText::new(t!("power-wizard.next-storage")).color(if can_next {
                    egui::Color32::WHITE
                } else {
                    AppColors::MUTED
                }),
            )
            .fill(if can_next { accent } else { AppColors::BG_CARD });
            if ui.add_enabled(can_next, next).clicked() {
                app.set_screen(Screen::PowerWizard(PowerWizardStep::StoragePassthrough));
            }
        });
    });
}

// ===================== Step 3: Storage & Passthrough =====================

fn render_step_storage(app: &mut LibreVmmApp, ui: &mut egui::Ui, accent: egui::Color32) {
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 60.0)
        .show(ui, |ui| {
            // === Disk Section ===
            section_header(ui, t!("power-wizard.primary-disk"), accent);

            let mut disk = app.wizard_disk_gib() as f64;
            ui.add(
                egui::Slider::new(&mut disk, 5.0..=2000.0)
                    .integer()
                    .suffix(" GiB"),
            );
            app.set_wizard_disk_gib(disk as u64);
            ui.add_space(theme::Spacing::SM);

            // Disk cache mode
            ui.label(
                egui::RichText::new(t!("power-wizard.disk-cache-mode"))
                    .color(accent)
                    .strong(),
            );
            let cache_modes: [(&str, std::borrow::Cow<'static, str>); 5] = [
                ("none", t!("power-wizard.cache-none")),
                ("writeback", t!("power-wizard.cache-writeback")),
                ("writethrough", t!("power-wizard.cache-writethrough")),
                ("unsafe", t!("power-wizard.cache-unsafe")),
                ("directsync", t!("power-wizard.cache-directsync")),
            ];

            let current_cache = app.power_disk_cache().to_string();
            egui::ComboBox::from_id_salt("disk_cache_mode")
                .selected_text(&current_cache)
                .show_ui(ui, |ui| {
                    for (mode, desc) in &cache_modes {
                        let selected = current_cache == *mode;
                        if ui
                            .selectable_label(
                                selected,
                                format!("{} — {}", mode, desc.split('—').next().unwrap_or("")),
                            )
                            .clicked()
                        {
                            app.set_power_disk_cache(mode.to_string());
                        }
                    }
                });

            if let Some((_, desc)) = cache_modes
                .iter()
                .find(|(m, _)| *m == current_cache.as_str())
            {
                ui.label(
                    egui::RichText::new(desc.as_ref())
                        .color(AppColors::TEXT_DIM)
                        .size(theme::FontSize::SMALL),
                );
            }
            ui.add_space(theme::Spacing::SM);

            // I/O mode
            ui.label(
                egui::RichText::new(t!("power-wizard.io-mode"))
                    .color(accent)
                    .strong(),
            );
            let mut io_mode_idx = if app.power_disk_io_mode() == "native" {
                0
            } else {
                1
            };
            ui.horizontal(|ui| {
                ui.selectable_value(&mut io_mode_idx, 0, t!("power-wizard.io-native"));
                ui.selectable_value(&mut io_mode_idx, 1, t!("power-wizard.io-threads"));
            });
            app.set_power_disk_io_mode(if io_mode_idx == 0 {
                "native".to_string()
            } else {
                "threads".to_string()
            });
            ui.add_space(theme::Spacing::SM);

            // I/O threads
            ui.label(
                egui::RichText::new(t!("power-wizard.io-threads-label"))
                    .color(accent)
                    .strong(),
            );
            let mut io_threads = app.power_io_threads() as f64;
            ui.add(egui::Slider::new(&mut io_threads, 0.0..=8.0).integer());
            // SECURITY: CWE-681 — Clamp f64 before cast to u32.
            app.set_power_io_threads(io_threads.clamp(0.0, 8.0) as u32);
            ui.label(
                egui::RichText::new(t!("power-wizard.io-threads-desc"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::SMALL),
            );

            ui.add_space(theme::Spacing::SM);

            // Boot ISO
            ui.label(
                egui::RichText::new(t!("power-wizard.boot-iso"))
                    .color(accent)
                    .strong(),
            );
            ui.horizontal(|ui| {
                ui.text_edit_singleline(app.wizard_iso_mut());
                if ui.button(t!("power-wizard.browse")).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ISO Images", &["iso", "img"])
                        .pick_file()
                    {
                        *app.wizard_iso_mut() = path.display().to_string();
                    }
                }
            });

            ui.add_space(theme::Spacing::LG);

            // === VFIO Passthrough ===
            section_header(ui, t!("power-wizard.vfio-passthrough"), accent);
            ui.label(
                egui::RichText::new(t!("power-wizard.vfio-desc"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::LABEL),
            );
            ui.label(
                egui::RichText::new(t!("power-wizard.vfio-iommu-req"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::SMALL),
            );
            ui.add_space(theme::Spacing::XS);

            // List current VFIO devices
            let devices = app.power_vfio_devices().to_vec();
            if devices.is_empty() {
                ui.label(
                    egui::RichText::new(t!("power-wizard.no-pci-devices"))
                        .color(AppColors::MUTED)
                        .size(theme::FontSize::LABEL),
                );
            } else {
                for (i, dev) in devices.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&dev.pci_address)
                                .family(egui::FontFamily::Monospace)
                                .color(AppColors::TEXT),
                        );
                        if !dev.description.is_empty() {
                            ui.label(
                                egui::RichText::new(&dev.description)
                                    .color(AppColors::TEXT_DIM)
                                    .size(theme::FontSize::LABEL),
                            );
                        }
                        if ui.small_button(t!("power-wizard.remove")).clicked() {
                            app.power_remove_vfio_device(i);
                        }
                    });
                }
            }

            // Add new VFIO device
            ui.horizontal(|ui| {
                ui.label(t!("power-wizard.pci-address"));
                let pci_addr = app.power_vfio_input_mut();
                ui.add(
                    egui::TextEdit::singleline(pci_addr)
                        .hint_text("0000:01:00.0")
                        .desired_width(160.0)
                        .font(egui::FontId::monospace(13.0)),
                );
                if ui.button(t!("power-wizard.add-device")).clicked() {
                    let addr = app.power_vfio_input().to_string();
                    if !addr.is_empty() {
                        app.power_add_vfio_device(VfioDeviceConfig {
                            pci_address: addr,
                            description: String::new(),
                            rom_bar: true,
                        });
                    }
                }
            });

            // Scan for VFIO devices
            if ui.button(t!("power-wizard.scan-vfio")).clicked() {
                let caps = vmm_core::gpu::detect_gpu_capabilities();
                for dev in &caps.vfio_devices {
                    if dev.vfio_bound {
                        let already_added = app
                            .power_vfio_devices()
                            .iter()
                            .any(|d| d.pci_address == dev.pci_address);
                        if !already_added {
                            app.power_add_vfio_device(VfioDeviceConfig {
                                pci_address: dev.pci_address.clone(),
                                description: dev.description.clone(),
                                rom_bar: true,
                            });
                        }
                    }
                }
            }
        });

    ui.add_space(theme::Spacing::MD);

    // Navigation
    ui.horizontal(|ui| {
        if ui.button(t!("power-wizard.back")).clicked() {
            app.set_screen(Screen::PowerWizard(PowerWizardStep::CpuMemory));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let next = egui::Button::new(
                egui::RichText::new(t!("power-wizard.next-network")).color(egui::Color32::WHITE),
            )
            .fill(accent);
            if ui.add(next).clicked() {
                app.set_screen(Screen::PowerWizard(PowerWizardStep::NetworkExtras));
            }
        });
    });
}

// ===================== Step 4: Network & Extras =====================

fn render_step_network(app: &mut LibreVmmApp, ui: &mut egui::Ui, accent: egui::Color32) {
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 60.0)
        .show(ui, |ui| {
            // === Network ===
            section_header(ui, t!("power-wizard.network-config"), accent);

            // Primary network mode (for simplicity, keep the single-NIC model for the wizard)
            ui.label(
                egui::RichText::new(t!("power-wizard.primary-network"))
                    .color(accent)
                    .strong(),
            );
            let current = app.wizard_network().clone();
            egui::ComboBox::from_id_salt("power_network_mode")
                .selected_text(match &current {
                    NetworkMode::Nat => t!("wizard.net-nat").into_owned(),
                    NetworkMode::Bridged => t!("wizard.net-bridged").into_owned(),
                    NetworkMode::HostOnly => t!("wizard.net-host-only").into_owned(),
                    NetworkMode::LanSegment(name) => format!("LAN: {}", name),
                    NetworkMode::None => t!("wizard.net-none").into_owned(),
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(current == NetworkMode::Nat, t!("wizard.net-nat"))
                        .clicked()
                    {
                        app.set_wizard_network(NetworkMode::Nat);
                    }
                    if ui
                        .selectable_label(current == NetworkMode::Bridged, t!("wizard.net-bridged"))
                        .clicked()
                    {
                        app.set_wizard_network(NetworkMode::Bridged);
                    }
                    if ui
                        .selectable_label(
                            current == NetworkMode::HostOnly,
                            t!("wizard.net-host-only"),
                        )
                        .clicked()
                    {
                        app.set_wizard_network(NetworkMode::HostOnly);
                    }
                    if ui
                        .selectable_label(current == NetworkMode::None, t!("wizard.net-none"))
                        .clicked()
                    {
                        app.set_wizard_network(NetworkMode::None);
                    }
                });
            ui.add_space(theme::Spacing::SM);

            // NIC model
            ui.label(
                egui::RichText::new(t!("power-wizard.nic-model"))
                    .color(accent)
                    .strong(),
            );
            let nic_models = ["virtio", "e1000e", "rtl8139"];
            let current_nic = app.power_nic_model().to_string();
            egui::ComboBox::from_id_salt("power_nic_model")
                .selected_text(&current_nic)
                .show_ui(ui, |ui| {
                    for model in &nic_models {
                        if ui.selectable_label(current_nic == *model, *model).clicked() {
                            app.set_power_nic_model(model.to_string());
                        }
                    }
                });
            ui.label(
                egui::RichText::new(t!("power-wizard.nic-model-desc"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::SMALL),
            );

            ui.add_space(theme::Spacing::LG);

            // === Boot Options ===
            section_header(ui, t!("power-wizard.boot-options"), accent);

            let mut uefi = app.wizard_uefi();
            ui.checkbox(&mut uefi, t!("power-wizard.uefi-boot"));
            app.set_wizard_uefi(uefi);

            let mut tpm = app.power_tpm_enabled();
            ui.checkbox(&mut tpm, t!("power-wizard.tpm-emulation"));
            app.set_power_tpm_enabled(tpm);
            ui.add_space(theme::Spacing::SM);

            let mut gpu = app.power_gpu_accel();
            ui.checkbox(&mut gpu, t!("power-wizard.gpu-accel"));
            app.set_power_gpu_accel(gpu);

            ui.horizontal(|ui| {
                ui.label(t!("power-wizard.display-protocol"));
                let current = app.power_display_protocol();
                egui::ComboBox::from_id_salt("power_display_proto")
                    .selected_text(current.to_string())
                    .show_ui(ui, |ui| {
                        for &proto in vmm_core::config::DisplayProtocol::ALL {
                            let label = format!("{} — {}", proto, proto.description());
                            if ui.selectable_label(current == proto, label).clicked() {
                                app.set_power_display_protocol(proto);
                            }
                        }
                    });
            });

            ui.add_space(theme::Spacing::LG);

            // === Custom QEMU Arguments ===
            section_header(ui, t!("power-wizard.custom-qemu-args"), accent);
            ui.label(
                egui::RichText::new(t!("power-wizard.custom-qemu-desc"))
                    .color(AppColors::WARNING)
                    .size(theme::FontSize::LABEL),
            );
            ui.add_space(theme::Spacing::XS);

            // Show existing args
            let args = app.power_custom_args().to_vec();
            let mut to_remove = None;
            for (i, arg) in args.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(arg)
                            .family(egui::FontFamily::Monospace)
                            .color(AppColors::TEXT)
                            .size(theme::FontSize::BODY),
                    );
                    if ui.small_button("✕").clicked() {
                        to_remove = Some(i);
                    }
                });
            }
            if let Some(idx) = to_remove {
                app.power_remove_custom_arg(idx);
            }

            // Add new arg
            ui.horizontal(|ui| {
                let input = app.power_custom_arg_input_mut();
                ui.add(
                    egui::TextEdit::singleline(input)
                        .hint_text("-device virtio-balloon-pci")
                        .desired_width(300.0)
                        .font(egui::FontId::monospace(13.0)),
                );
                if ui.button(t!("power-wizard.add-arg")).clicked() {
                    let arg = app.power_custom_arg_input().to_string();
                    if !arg.is_empty() {
                        app.power_add_custom_arg(arg);
                    }
                }
            });
        });

    ui.add_space(theme::Spacing::MD);

    // Navigation
    ui.horizontal(|ui| {
        if ui.button(t!("power-wizard.back")).clicked() {
            app.set_screen(Screen::PowerWizard(PowerWizardStep::StoragePassthrough));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let next = egui::Button::new(
                egui::RichText::new(t!("power-wizard.next-review")).color(egui::Color32::WHITE),
            )
            .fill(accent);
            if ui.add(next).clicked() {
                app.set_screen(Screen::PowerWizard(PowerWizardStep::Review));
            }
        });
    });
}

// ===================== Step 5: Review & Create =====================

fn render_step_review(app: &mut LibreVmmApp, ui: &mut egui::Ui, accent: egui::Color32) {
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 60.0)
        .show(ui, |ui| {
            let frame = egui::Frame::none()
                .fill(AppColors::BG_CARD)
                .rounding(egui::Rounding::same(theme::ThemeRounding::CARD))
                .inner_margin(egui::Margin::same(16.0));

            frame.show(ui, |ui| {
                ui.heading(egui::RichText::new(app.wizard_name()).color(AppColors::TEXT));
                ui.label(
                    egui::RichText::new(t!("power-wizard.power-user-vm"))
                        .color(accent)
                        .size(theme::FontSize::LABEL),
                );
                ui.add_space(theme::Spacing::SM);

                egui::Grid::new("power_review_grid")
                    .num_columns(2)
                    .spacing([20.0, 6.0])
                    .show(ui, |ui| {
                        review_row(
                            ui,
                            t!("power-wizard.review-box"),
                            t!("power-wizard.title"),
                            accent,
                        );

                        let templates = builtin_templates();
                        if let Some(t) = templates.get(app.wizard_template_idx()) {
                            review_row(
                                ui,
                                t!("power-wizard.review-template"),
                                t.label.to_string(),
                                accent,
                            );
                        }

                        review_row(
                            ui,
                            t!("power-wizard.review-vcpus"),
                            app.wizard_cpus().to_string(),
                            accent,
                        );

                        if let Some(topo) = app.power_cpu_topology() {
                            review_row(
                                ui,
                                t!("power-wizard.review-topology"),
                                topo.to_string(),
                                accent,
                            );
                        }

                        review_row(
                            ui,
                            t!("power-wizard.review-memory"),
                            format!(
                                "{} MiB ({:.1} GiB)",
                                app.wizard_memory_mib(),
                                app.wizard_memory_mib() as f64 / 1024.0
                            ),
                            accent,
                        );
                        review_row(
                            ui,
                            t!("power-wizard.review-hugepages"),
                            if app.power_hugepages() {
                                t!("arch.yes").into_owned()
                            } else {
                                t!("arch.no").into_owned()
                            },
                            accent,
                        );
                        review_row(
                            ui,
                            t!("power-wizard.review-disk"),
                            format!("{} GiB", app.wizard_disk_gib()),
                            accent,
                        );
                        review_row(
                            ui,
                            t!("power-wizard.review-disk-cache"),
                            app.power_disk_cache().to_string(),
                            accent,
                        );
                        review_row(
                            ui,
                            t!("power-wizard.review-io-mode"),
                            app.power_disk_io_mode().to_string(),
                            accent,
                        );

                        if app.power_io_threads() > 0 {
                            review_row(
                                ui,
                                t!("power-wizard.review-io-threads"),
                                app.power_io_threads().to_string(),
                                accent,
                            );
                        }

                        review_row(
                            ui,
                            t!("power-wizard.review-network"),
                            app.wizard_network().to_string(),
                            accent,
                        );
                        review_row(
                            ui,
                            t!("power-wizard.review-nic-model"),
                            app.power_nic_model().to_string(),
                            accent,
                        );
                        review_row(
                            ui,
                            t!("power-wizard.review-uefi"),
                            if app.wizard_uefi() {
                                t!("arch.yes").into_owned()
                            } else {
                                t!("arch.no").into_owned()
                            },
                            accent,
                        );
                        review_row(
                            ui,
                            t!("power-wizard.review-tpm"),
                            if app.power_tpm_enabled() {
                                t!("arch.yes").into_owned()
                            } else {
                                t!("arch.no").into_owned()
                            },
                            accent,
                        );
                        review_row(
                            ui,
                            t!("power-wizard.review-gpu-accel"),
                            if app.power_gpu_accel() {
                                "VirGL".to_string()
                            } else {
                                t!("arch.no").into_owned()
                            },
                            accent,
                        );
                        review_row(
                            ui,
                            t!("power-wizard.review-display"),
                            app.power_display_protocol().to_string(),
                            accent,
                        );

                        if !app.wizard_iso().is_empty() {
                            review_row(
                                ui,
                                t!("power-wizard.review-iso"),
                                app.wizard_iso().to_string(),
                                accent,
                            );
                        }

                        let vfio = app.power_vfio_devices();
                        if !vfio.is_empty() {
                            review_row(
                                ui,
                                t!("power-wizard.review-vfio"),
                                t!("power-wizard.devices-count", n = vfio.len()),
                                accent,
                            );
                            for dev in vfio {
                                review_row(
                                    ui,
                                    "",
                                    format!("  {} {}", dev.pci_address, dev.description),
                                    accent,
                                );
                            }
                        }

                        let args = app.power_custom_args();
                        if !args.is_empty() {
                            review_row(
                                ui,
                                t!("power-wizard.review-custom-args"),
                                t!("power-wizard.args-count", n = args.len()),
                                accent,
                            );
                        }
                    });
            });

            if !app.wizard_description().is_empty() {
                ui.add_space(theme::Spacing::SM);
                ui.label(
                    egui::RichText::new(t!("power-wizard.description-label"))
                        .color(AppColors::TEXT_DIM),
                );
                ui.label(app.wizard_description());
            }

            // Custom args preview
            let args = app.power_custom_args();
            if !args.is_empty() {
                ui.add_space(theme::Spacing::SM);
                ui.label(
                    egui::RichText::new(t!("power-wizard.custom-args-preview"))
                        .color(accent)
                        .size(theme::FontSize::LABEL),
                );
                for arg in args {
                    ui.label(
                        egui::RichText::new(arg)
                            .family(egui::FontFamily::Monospace)
                            .color(AppColors::TEXT_DIM)
                            .size(theme::FontSize::SMALL),
                    );
                }
            }
        });

    ui.add_space(theme::Spacing::MD);

    // Navigation
    ui.horizontal(|ui| {
        if ui.button(t!("power-wizard.back")).clicked() {
            app.set_screen(Screen::PowerWizard(PowerWizardStep::NetworkExtras));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let create_btn = ui.add(
                egui::Button::new(
                    egui::RichText::new(t!("power-wizard.create-power-vm"))
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(accent)
                .min_size(egui::vec2(140.0, 32.0)),
            );
            if create_btn.clicked() {
                app.action_create_power_vm();
            }
        });
    });
}

// ===================== Helpers =====================

fn section_header(ui: &mut egui::Ui, title: impl Into<String>, accent: egui::Color32) {
    ui.label(
        egui::RichText::new(title)
            .color(accent)
            .size(theme::FontSize::HEADING)
            .strong(),
    );
    ui.separator();
    ui.add_space(theme::Spacing::XS);
}

fn review_row(
    ui: &mut egui::Ui,
    label: impl Into<String>,
    value: impl Into<String>,
    _accent: egui::Color32,
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
    ui.end_row();
}
