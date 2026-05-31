//! VM Summary — VMware-style hardware device list + info.

use crate::app::{LibreVmmApp, Screen};
use crate::theme;
use crate::theme::{AppColors, BoxColors};
use eframe::egui;
use rust_i18n::t;
use std::fmt::Write;
use vmm_core::config::OsType;
use vmm_core::domain::VmState;
use vmm_core::qemu_archs::BoxType;

pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui, name: &str) {
    let name = name.to_string();

    let vm = app.vms().iter().find(|v| v.name == name).cloned();
    let Some(vm) = vm else {
        ui.label(t!("summary.vm-not-found").as_ref());
        return;
    };

    let config = app.selected_vm_config().cloned();

    egui::ScrollArea::vertical().show(ui, |ui| {
        // VM title + state
        ui.horizontal(|ui| {
            let state_color = match vm.state {
                VmState::Running => AppColors::RUNNING,
                VmState::Paused => AppColors::PAUSED,
                VmState::Crashed => AppColors::CRASHED,
                _ => AppColors::OFF,
            };

            let (dot_rect, _) =
                ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(dot_rect.center(), 6.0, state_color);

            ui.heading(
                egui::RichText::new(&vm.name)
                    .color(AppColors::TEXT)
                    .strong(),
            );
            ui.label(
                egui::RichText::new(format!("({})", vm.state))
                    .color(state_color)
                    .size(theme::FontSize::HEADING),
            );
        });

        // OS type label + Box type badge + Architecture info
        if let Some(ref cfg) = config {
            ui.horizontal(|ui| {
                let os_label = match cfg.os_type {
                    OsType::Linux => "Linux",
                    OsType::Windows => "Windows",
                    OsType::MacOS => "macOS",
                    OsType::FreeBSD => "FreeBSD",
                    OsType::Other => "Other OS",
                };
                ui.label(
                    egui::RichText::new(os_label)
                        .size(theme::FontSize::BODY)
                        .color(AppColors::TEXT_DIM),
                );

                // Box type badge
                let box_color = BoxColors::primary(&cfg.box_type);
                ui.label(
                    egui::RichText::new(format!(" | {}", cfg.box_type.display_name()))
                        .size(theme::FontSize::SMALL)
                        .color(box_color),
                );

                // Architecture badge (only show if not default x86_64)
                if cfg.qemu_arch != vmm_core::qemu_archs::QemuArch::X86_64 {
                    ui.label(
                        egui::RichText::new(format!(" | {}", cfg.qemu_arch.display_name()))
                            .size(theme::FontSize::SMALL)
                            .color(box_color),
                    );
                }

                if cfg.autostart {
                    ui.label(
                        egui::RichText::new(format!(" | {}", t!("summary.autostart")))
                            .size(theme::FontSize::SMALL)
                            .color(AppColors::PRIMARY),
                    );
                }
            });
        }

        ui.add_space(theme::Spacing::SM);
        ui.separator();
        ui.add_space(theme::Spacing::SM);

        // ===== Hardware Device List =====
        egui::Frame::none()
            .fill(AppColors::BG_CARD)
            .rounding(theme::ThemeRounding::CARD)
            .inner_margin(theme::Spacing::LG)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(t!("summary.hardware").as_ref())
                        .size(15.0)
                        .strong()
                        .color(AppColors::TEXT),
                );
                ui.add_space(theme::Spacing::SM);

                egui::Grid::new("hw_summary")
                    .num_columns(3)
                    .spacing([12.0, 6.0])
                    .min_col_width(30.0)
                    .show(ui, |ui| {
                        // Architecture & Machine (for non-default configs)
                        if let Some(ref cfg) = config {
                            if cfg.box_type == BoxType::HardwareLab
                                || cfg.qemu_arch != vmm_core::qemu_archs::QemuArch::X86_64
                            {
                                hw_row(
                                    ui,
                                    "\u{1F9EC}",
                                    &t!("summary.architecture"),
                                    cfg.qemu_arch.display_name(),
                                );
                                hw_row(
                                    ui,
                                    "\u{2699}",
                                    &t!("summary.machine-type"),
                                    &cfg.machine_type,
                                );
                                if !cfg.cpu_model.is_empty() {
                                    hw_row(
                                        ui,
                                        "\u{1F4A0}",
                                        &t!("summary.cpu-model"),
                                        &cfg.cpu_model,
                                    );
                                }
                                let emulation_val = if cfg.use_kvm {
                                    t!("summary.kvm-hardware")
                                } else {
                                    t!("summary.tcg-software")
                                };
                                hw_row(ui, "\u{26A1}", &t!("summary.emulation"), &emulation_val);
                            }
                        }

                        // Memory
                        hw_row(
                            ui,
                            "\u{1F4BB}",
                            &t!("summary.memory"),
                            &format!(
                                "{} MiB ({:.1} GiB)",
                                vm.memory_mib,
                                vm.memory_mib as f64 / 1024.0
                            ),
                        );

                        // Processors
                        hw_row(
                            ui,
                            "\u{2699}",
                            &t!("summary.processors"),
                            &format!("{} vCPU{}", vm.vcpus, if vm.vcpus > 1 { "s" } else { "" }),
                        );

                        // Hard Disk — show configured size only (no per-frame I/O)
                        if let Some(ref cfg) = config {
                            let disk_str = format!("{} GiB", cfg.disk_size_gib);
                            hw_row(ui, "\u{1F4BE}", &t!("summary.hard-disk"), &disk_str);
                        }

                        // CD/DVD (ISO)
                        if let Some(ref cfg) = config {
                            let empty_label = t!("summary.empty");
                            let iso_str = cfg.iso_path.as_deref().unwrap_or(&empty_label);
                            let iso_display = if iso_str.len() > 40 {
                                let start = iso_str
                                    .char_indices()
                                    .rev()
                                    .nth(36)
                                    .map(|(i, _)| i)
                                    .unwrap_or(0);
                                format!("...{}", &iso_str[start..])
                            } else {
                                iso_str.to_string()
                            };
                            hw_row(ui, "\u{1F4BF}", &t!("summary.cd-dvd"), &iso_display);
                        }

                        // Network Adapters — show effective NICs
                        if let Some(ref cfg) = config {
                            let nics = cfg.effective_nics();
                            if nics.is_empty() {
                                hw_row(
                                    ui,
                                    "\u{1F310}",
                                    &t!("summary.network"),
                                    &t!("summary.disconnected"),
                                );
                            } else if nics.len() == 1 {
                                let nic = &nics[0];
                                hw_row(
                                    ui,
                                    "\u{1F310}",
                                    &t!("summary.network-adapter"),
                                    &format!("{} ({})", nic.mode, nic.model),
                                );
                            } else {
                                for (i, nic) in nics.iter().enumerate() {
                                    hw_row(
                                        ui,
                                        "\u{1F310}",
                                        &format!("{} {}", t!("summary.network-adapter"), i + 1),
                                        &format!("{} ({})", nic.mode, nic.model),
                                    );
                                }
                            }
                        }

                        // Boot Order
                        if let Some(ref cfg) = config {
                            let mut boot_str = String::new();
                            for (i, d) in cfg.boot_order.iter().enumerate() {
                                if i > 0 {
                                    boot_str.push_str(" \u{2192} ");
                                }
                                let _ = write!(boot_str, "{}", d);
                            }
                            hw_row(ui, "\u{1F4BB}", &t!("summary.boot-order"), &boot_str);
                        }

                        // Display
                        if let Some(ref cfg) = config {
                            let protocol = cfg.display_protocol.to_string();
                            let display_str = if cfg.display_count > 1 {
                                format!("{} ({} heads)", protocol, cfg.display_count)
                            } else {
                                protocol.to_string()
                            };
                            hw_row(ui, "\u{1F5B5}", &t!("summary.display"), &display_str);
                        }

                        // Sound Card
                        if let Some(ref cfg) = config {
                            if cfg.audio {
                                hw_row(ui, "\u{1F50A}", &t!("summary.sound-card"), "ich9 (HDA)");
                            }
                        }

                        // USB Controller
                        if let Some(ref cfg) = config {
                            if cfg.usb_support {
                                hw_row(
                                    ui,
                                    "\u{2328}",
                                    &t!("summary.usb-controller"),
                                    "USB 3.0 (xhci)",
                                );
                            }
                        }

                        // Shared folder
                        if let Some(ref cfg) = config {
                            if let Some(ref folder) = cfg.shared_folder {
                                let folder_display = if folder.len() > 40 {
                                    let start = folder
                                        .char_indices()
                                        .rev()
                                        .nth(36)
                                        .map(|(i, _)| i)
                                        .unwrap_or(0);
                                    format!("...{}", &folder[start..])
                                } else {
                                    folder.clone()
                                };
                                hw_row(
                                    ui,
                                    "\u{1F4C1}",
                                    &t!("summary.shared-folder"),
                                    &folder_display,
                                );
                            }
                        }

                        // TPM
                        if let Some(ref cfg) = config {
                            if cfg.tpm_enabled {
                                hw_row(
                                    ui,
                                    "\u{1F512}",
                                    &t!("summary.tpm"),
                                    &format!("v{} (swtpm)", cfg.tpm_version),
                                );
                            }
                        }

                        // Port Forwarding
                        if let Some(ref cfg) = config {
                            if !cfg.port_forwards.is_empty() {
                                let mut summary =
                                    String::with_capacity(cfg.port_forwards.len() * 20);
                                let _ = write!(
                                    summary,
                                    "{} rule{}: ",
                                    cfg.port_forwards.len(),
                                    if cfg.port_forwards.len() != 1 {
                                        "s"
                                    } else {
                                        ""
                                    }
                                );
                                for (i, r) in cfg.port_forwards.iter().enumerate() {
                                    if i > 0 {
                                        summary.push_str(", ");
                                    }
                                    if r.description.is_empty() {
                                        let _ = write!(
                                            summary,
                                            "{}:{}\u{2192}{}",
                                            r.protocol, r.host_port, r.guest_port
                                        );
                                    } else {
                                        summary.push_str(&r.description);
                                    }
                                }
                                hw_row(ui, "\u{1F517}", &t!("summary.port-forward"), &summary);
                            }
                        }

                        // Resource Limits / QoS
                        if let Some(ref cfg) = config {
                            if cfg.resource_limits.has_any() {
                                let parts = cfg.resource_limits.summary();
                                hw_row(
                                    ui,
                                    "\u{2696}",
                                    &t!("summary.resource-limits"),
                                    &parts.join(", "),
                                );
                            }
                        }

                        // Performance Profile
                        if let Some(ref cfg) = config {
                            if cfg.performance_profile != "default" {
                                hw_row(
                                    ui,
                                    "\u{1F3AE}",
                                    &t!("summary.profile"),
                                    &cfg.performance_profile,
                                );
                            }
                        }

                        // ===== Power User / Hardware Lab Features =====
                        if let Some(ref cfg) = config {
                            if cfg.box_type == BoxType::PowerUser
                                || cfg.box_type == BoxType::HardwareLab
                                || cfg.cpu_topology.is_some()
                                || !cfg.cpu_features.is_empty()
                                || cfg.hugepages
                                || !cfg.vfio_devices.is_empty()
                                || !cfg.custom_qemu_args.is_empty()
                            {
                                // CPU Topology
                                if let Some(ref topo) = cfg.cpu_topology {
                                    hw_row(
                                        ui,
                                        "\u{1F9E9}",
                                        &t!("summary.cpu-topology"),
                                        &topo.to_string(),
                                    );
                                }
                                // CPU Feature flags
                                if !cfg.cpu_features.is_empty() {
                                    hw_row(
                                        ui,
                                        "\u{2699}",
                                        &t!("summary.cpu-features"),
                                        &cfg.cpu_features.join(", "),
                                    );
                                }
                                // Hugepages
                                if cfg.hugepages {
                                    hw_row(
                                        ui,
                                        "\u{1F4D6}",
                                        &t!("summary.hugepages"),
                                        "Enabled (2 MiB)",
                                    );
                                }
                                // Disk cache
                                if cfg.disk_cache != "writeback" && !cfg.disk_cache.is_empty() {
                                    hw_row(
                                        ui,
                                        "\u{1F4BE}",
                                        &t!("summary.disk-cache"),
                                        &cfg.disk_cache,
                                    );
                                }
                                // I/O threads
                                if cfg.io_threads > 0 {
                                    hw_row(
                                        ui,
                                        "\u{26A1}",
                                        &t!("summary.io-threads"),
                                        &cfg.io_threads.to_string(),
                                    );
                                }
                                // VFIO devices
                                for dev in &cfg.vfio_devices {
                                    let desc = if dev.description.is_empty() {
                                        dev.pci_address.clone()
                                    } else {
                                        format!("{} ({})", dev.description, dev.pci_address)
                                    };
                                    hw_row(ui, "\u{1F3AE}", &t!("summary.vfio-passthrough"), &desc);
                                }
                                // Custom QEMU args
                                if !cfg.custom_qemu_args.is_empty() {
                                    hw_row(
                                        ui,
                                        "\u{2699}",
                                        &t!("summary.custom-qemu-args"),
                                        &format!("{} arg(s)", cfg.custom_qemu_args.len()),
                                    );
                                }
                            }
                        }
                    });
            });

        ui.add_space(theme::Spacing::MD);

        // Description
        if let Some(ref cfg) = config {
            if !cfg.description.is_empty() {
                egui::Frame::none()
                    .fill(AppColors::BG_CARD)
                    .rounding(theme::ThemeRounding::CARD)
                    .inner_margin(theme::Spacing::LG)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(t!("summary.description").as_ref())
                                .size(theme::FontSize::BODY)
                                .color(AppColors::TEXT_DIM),
                        );
                        ui.add_space(theme::Spacing::XS);
                        ui.label(
                            egui::RichText::new(&cfg.description)
                                .size(theme::FontSize::BODY)
                                .color(AppColors::TEXT),
                        );
                    });
                ui.add_space(theme::Spacing::MD);
            }
        }

        // Notes (Markdown)
        if let Some(ref cfg) = config {
            if !cfg.notes.is_empty() {
                egui::Frame::none()
                    .fill(AppColors::BG_CARD)
                    .rounding(theme::ThemeRounding::CARD)
                    .inner_margin(theme::Spacing::LG)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(t!("summary.notes").as_ref())
                                .size(theme::FontSize::BODY)
                                .color(AppColors::TEXT_DIM),
                        );
                        ui.add_space(theme::Spacing::XS);
                        // Render notes as plain text (Markdown rendering could be added later)
                        ui.label(
                            egui::RichText::new(&cfg.notes)
                                .size(theme::FontSize::BODY)
                                .color(AppColors::TEXT),
                        );
                    });
                ui.add_space(theme::Spacing::MD);
            }
        }

        // Machine Info
        egui::Frame::none()
            .fill(AppColors::BG_CARD)
            .rounding(theme::ThemeRounding::CARD)
            .inner_margin(theme::Spacing::LG)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(t!("summary.machine-info").as_ref())
                        .size(15.0)
                        .strong()
                        .color(AppColors::TEXT),
                );
                ui.add_space(theme::Spacing::SM);

                egui::Grid::new("vm_info")
                    .num_columns(2)
                    .spacing([16.0, 6.0])
                    .show(ui, |ui| {
                        info_row(ui, &t!("summary.uuid"), &vm.uuid);
                        info_row(ui, &t!("summary.state"), &vm.state.to_string());
                        if vm.state == VmState::Running {
                            let cpu_secs = vm.cpu_time_ns / 1_000_000_000;
                            info_row(
                                ui,
                                &t!("summary.cpu-time"),
                                &format!(
                                    "{}h {}m {}s",
                                    cpu_secs / 3600,
                                    (cpu_secs % 3600) / 60,
                                    cpu_secs % 60
                                ),
                            );
                        }
                        if let Some(ref cfg) = config {
                            info_row(ui, &t!("summary.uefi"), if cfg.uefi { "Yes" } else { "No" });
                            if cfg.gpu_accel {
                                info_row(ui, &t!("summary.gpu-accel"), "VirGL 3D Enabled");
                            }
                            if cfg.display_count > 1 {
                                info_row(
                                    ui,
                                    &t!("summary.displays"),
                                    &format!("{} heads", cfg.display_count),
                                );
                            }
                            if cfg.disk_encrypted {
                                info_row(ui, &t!("summary.encryption"), "\u{1F512} LUKS");
                            }
                            if cfg.tpm_enabled {
                                info_row(
                                    ui,
                                    &t!("summary.tpm"),
                                    &format!("v{} (swtpm emulated)", cfg.tpm_version),
                                );
                            }
                            if cfg.autostart {
                                info_row(ui, &t!("summary.autostart"), "Enabled");
                            }
                        }
                        // Managed save indicator
                        if vm.state == VmState::Off && app.has_managed_save(&name) {
                            info_row(ui, &t!("summary.saved-state"), "\u{1F4BE} Ready to resume");
                        }
                    });
            });

        // Guest Agent Information (when running and available)
        if vm.state == VmState::Running {
            if let Some(ref guest) = app.guest_info() {
                if guest.agent_available {
                    ui.add_space(theme::Spacing::MD);
                    egui::Frame::none()
                        .fill(AppColors::BG_CARD)
                        .rounding(theme::ThemeRounding::CARD)
                        .inner_margin(theme::Spacing::LG)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(t!("summary.guest-info").as_ref())
                                    .size(15.0)
                                    .strong()
                                    .color(AppColors::TEXT),
                            );
                            ui.add_space(theme::Spacing::XS);
                            ui.label(
                                egui::RichText::new(t!("summary.via-qga").as_ref())
                                    .size(10.0)
                                    .color(AppColors::TEXT_DIM),
                            );
                            ui.add_space(theme::Spacing::SM);

                            egui::Grid::new("guest_info")
                                .num_columns(2)
                                .spacing([16.0, 6.0])
                                .show(ui, |ui| {
                                    if let Some(ref hostname) = guest.hostname {
                                        info_row(ui, &t!("summary.hostname"), hostname);
                                    }
                                    if let Some(ref os_name) = guest.os_name {
                                        info_row(ui, &t!("summary.guest-os"), os_name);
                                    }
                                    if let Some(ref os_ver) = guest.os_version {
                                        info_row(ui, &t!("summary.kernel"), os_ver);
                                    }
                                    for ip in &guest.ip_addresses {
                                        info_row(
                                            ui,
                                            &format!("IP ({})", ip.interface),
                                            &format!("{}/{}", ip.address, ip.prefix),
                                        );
                                    }
                                    for fs in &guest.filesystems {
                                        let used_gb =
                                            fs.used_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                                        let total_gb =
                                            fs.total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                                        if total_gb > 0.0 {
                                            info_row(
                                                ui,
                                                &format!("Disk {}", fs.mountpoint),
                                                &format!(
                                                    "{:.1}/{:.1} GiB ({})",
                                                    used_gb, total_gb, fs.fs_type
                                                ),
                                            );
                                        }
                                    }
                                });
                        });
                }
            }
        }

        ui.add_space(theme::Spacing::MD);

        // Tags display
        if let Some(ref cfg) = config {
            if !cfg.tags.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(t!("summary.tags").as_ref())
                            .size(theme::FontSize::SMALL)
                            .color(AppColors::TEXT_DIM),
                    );
                    for tag in &cfg.tags {
                        egui::Frame::none()
                            .fill(AppColors::PRIMARY.gamma_multiply(0.3))
                            .rounding(theme::ThemeRounding::BUTTON_SMALL)
                            .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(tag).size(10.0).color(AppColors::TEXT),
                                );
                            });
                    }
                });
                ui.add_space(theme::Spacing::SM);
            }
        }

        // Action buttons
        ui.horizontal(|ui| {
            let state = &vm.state;
            let has_save = app.has_managed_save(&name);

            match state {
                VmState::Off | VmState::Crashed => {
                    let btn_label = if has_save {
                        format!("\u{25B6} {}", t!("summary.resume-saved"))
                    } else {
                        format!("\u{25B6} {}", t!("summary.power-on"))
                    };
                    let start_btn = egui::Button::new(
                        egui::RichText::new(&btn_label).color(egui::Color32::WHITE),
                    )
                    .fill(AppColors::SUCCESS)
                    .rounding(theme::ThemeRounding::BUTTON)
                    .min_size(egui::vec2(140.0, 32.0));
                    if ui.add(start_btn).clicked() {
                        app.action_start(&name);
                    }
                    if has_save {
                        let discard_btn = egui::Button::new(
                            egui::RichText::new(t!("summary.discard-save").as_ref()).size(12.0),
                        )
                        .rounding(theme::ThemeRounding::BUTTON);
                        if ui
                            .add(discard_btn)
                            .on_hover_text("Discard saved state — next start will boot fresh")
                            .clicked()
                        {
                            app.action_discard_save(&name);
                        }
                    }
                },
                VmState::Running => {
                    let console_label = format!("\u{1F5B5} {}", t!("summary.open-console"));
                    let console_btn = egui::Button::new(
                        egui::RichText::new(&console_label).color(egui::Color32::WHITE),
                    )
                    .fill(AppColors::PRIMARY)
                    .rounding(theme::ThemeRounding::BUTTON)
                    .min_size(egui::vec2(140.0, 32.0));
                    if ui.add(console_btn).clicked() {
                        app.action_console(&name);
                    }
                },
                VmState::Paused | VmState::Suspended => {
                    let resume_label = format!("\u{25B6} {}", t!("summary.resume"));
                    let resume_btn = egui::Button::new(
                        egui::RichText::new(&resume_label).color(egui::Color32::WHITE),
                    )
                    .fill(AppColors::SUCCESS)
                    .rounding(theme::ThemeRounding::BUTTON)
                    .min_size(egui::vec2(120.0, 32.0));
                    if ui.add(resume_btn).clicked() {
                        app.action_resume(&name);
                    }
                },
                _ => {},
            }

            // Edit Settings button — always available (metadata editable while running)
            let is_running = matches!(state, VmState::Running | VmState::Paused);
            let btn_label = if is_running {
                format!("\u{270F} {}", t!("summary.settings-metadata"))
            } else {
                format!("\u{270F} {}", t!("summary.edit-settings"))
            };
            let settings_btn =
                egui::Button::new(egui::RichText::new(&btn_label).size(theme::FontSize::BODY))
                    .rounding(theme::ThemeRounding::BUTTON)
                    .min_size(egui::vec2(130.0, 32.0));

            if ui.add(settings_btn).clicked() {
                if let Some(config) = app.selected_vm_config().cloned() {
                    app.set_editing_config(Some(config));
                    app.set_screen(Screen::VmSettings(name.clone()));
                }
            }
        });

        // Delete confirmation
        if app.confirm_delete() == Some(&name) {
            ui.add_space(theme::Spacing::MD);
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(60, 30, 30))
                .rounding(theme::ThemeRounding::CARD)
                .inner_margin(theme::Spacing::MD)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(t!("summary.delete-confirm").as_ref())
                            .color(AppColors::DANGER),
                    );
                    ui.horizontal(|ui| {
                        let yes = egui::Button::new(
                            egui::RichText::new(t!("summary.delete-yes").as_ref())
                                .color(egui::Color32::WHITE),
                        )
                        .fill(AppColors::DANGER);
                        if ui.add(yes).clicked() {
                            app.set_confirm_delete(None);
                            app.action_delete(&name);
                        }
                        if ui.button(t!("summary.delete-cancel").as_ref()).clicked() {
                            app.set_confirm_delete(None);
                        }
                    });
                });
        }
    });
}

fn hw_row(ui: &mut egui::Ui, icon: &str, label: &str, value: &str) {
    ui.label(egui::RichText::new(icon).size(14.0));
    ui.label(
        egui::RichText::new(label)
            .size(theme::FontSize::BODY)
            .color(AppColors::TEXT_DIM),
    );
    ui.label(
        egui::RichText::new(value)
            .size(theme::FontSize::BODY)
            .color(AppColors::TEXT),
    );
    ui.end_row();
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
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
