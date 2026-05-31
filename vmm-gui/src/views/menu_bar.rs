//! Menu bar — File / VM / View / Help, VMware Workstation style.

use crate::app::{LibreVmmApp, ManageTab, Screen, ViewMode};
use crate::theme::{AppColors, BoxColors, FontSize};
use eframe::egui;
use rust_i18n::t;
use vmm_core::domain::VmState;

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            // App branding — accent colored by active box type
            let brand_color = BoxColors::primary(app.active_box_type());
            ui.label(
                egui::RichText::new(t!("app.brand"))
                    .color(brand_color)
                    .strong()
                    .size(FontSize::MENU_BRAND),
            );

            ui.separator();

            // ===== FILE MENU =====
            ui.menu_button(t!("menu.file"), |ui| {
                if ui.button(t!("menu.file.new-vm")).clicked() {
                    app.set_screen(Screen::BoxSelector);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(t!("menu.file.import-ova")).clicked() {
                    app.import_export_state_mut().open_import();
                    ui.close_menu();
                }
                if ui.button(t!("menu.file.import-wizard")).clicked() {
                    app.open_import_wizard();
                    ui.close_menu();
                }
                if ui.button(t!("menu.file.import-vmware-library")).clicked() {
                    app.action_import_vmware_library();
                    ui.close_menu();
                }
                if ui.button(t!("menu.file.import-vbox-library")).clicked() {
                    app.action_import_vbox_library();
                    ui.close_menu();
                }
                if ui.button(t!("menu.file.first-run")).clicked() {
                    app.action_open_first_run();
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        app.selected_vm().is_some(),
                        egui::Button::new(t!("menu.file.export-ova")),
                    )
                    .clicked()
                {
                    if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                        app.import_export_state_mut().open_export(&name);
                    }
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(t!("menu.file.templates")).clicked() {
                    app.template_manager_state_mut().open();
                    ui.close_menu();
                }
                if ui.button(t!("menu.file.remote-hosts")).clicked() {
                    app.remote_hosts_state_mut().open();
                    ui.close_menu();
                }
                if ui.button(t!("menu.file.iso-library")).clicked() {
                    app.set_show_iso_picker(true);
                    ui.close_menu();
                }
                if ui.button(t!("menu.file.network-editor")).clicked() {
                    app.action_open_network_editor();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(t!("menu.file.refresh")).clicked() {
                    app.action_refresh();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(t!("menu.file.quit")).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    ui.close_menu();
                }
            });

            // ===== VM MENU =====
            let has_vm = app.selected_vm().is_some();
            let state = app.selected_vm_state();

            ui.menu_button(t!("menu.vm"), |ui| {
                if !has_vm {
                    ui.label(
                        egui::RichText::new(t!("menu.vm.no-selection"))
                            .color(AppColors::TEXT_DIM)
                            .italics(),
                    );
                    return;
                }

                // Power submenu
                ui.menu_button(t!("menu.vm.power"), |ui| {
                    let is_off = matches!(state, Some(VmState::Off) | Some(VmState::Crashed));
                    let is_running = matches!(state, Some(VmState::Running));
                    let is_paused =
                        matches!(state, Some(VmState::Paused) | Some(VmState::Suspended));

                    if ui
                        .add_enabled(is_off, egui::Button::new(t!("power.start")))
                        .clicked()
                    {
                        if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                            app.action_start(&name);
                        }
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(is_running, egui::Button::new(t!("power.shutdown")))
                        .clicked()
                    {
                        if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                            app.action_shutdown(&name);
                        }
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            is_running || is_paused,
                            egui::Button::new(t!("power.power-off")),
                        )
                        .clicked()
                    {
                        if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                            app.action_force_stop(&name);
                        }
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(is_running, egui::Button::new(t!("power.pause")))
                        .clicked()
                    {
                        if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                            app.action_pause(&name);
                        }
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(is_paused, egui::Button::new(t!("power.resume")))
                        .clicked()
                    {
                        if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                            app.action_resume(&name);
                        }
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(is_running, egui::Button::new(t!("power.reboot")))
                        .clicked()
                    {
                        if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                            app.action_reboot(&name);
                        }
                        ui.close_menu();
                    }

                    ui.separator();

                    // Boot to Firmware (UEFI Setup)
                    if ui
                        .add_enabled(is_off, egui::Button::new(t!("menu.vm.boot-firmware")))
                        .on_hover_text(t!("menu.vm.boot-firmware-tooltip"))
                        .clicked()
                    {
                        app.action_boot_to_firmware();
                        ui.close_menu();
                    }
                });

                ui.separator();

                if ui
                    .add_enabled(
                        matches!(state, Some(VmState::Running)),
                        egui::Button::new(t!("menu.vm.console")),
                    )
                    .clicked()
                {
                    if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                        app.action_console(&name);
                    }
                    ui.close_menu();
                }

                if ui.button(t!("menu.vm.snapshots")).clicked() {
                    app.set_view_mode(ViewMode::Manage(ManageTab::Snapshots));
                    app.set_screen(Screen::Home);
                    ui.close_menu();
                }

                ui.separator();

                // Clone
                let is_off = matches!(state, Some(VmState::Off) | Some(VmState::Crashed));
                if ui
                    .add_enabled(is_off, egui::Button::new(t!("menu.vm.clone")))
                    .clicked()
                {
                    if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                        app.clone_dialog_state_mut().open(&name);
                    }
                    ui.close_menu();
                }

                // Save as Template
                if ui.button(t!("menu.vm.save-template")).clicked() {
                    if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                        app.template_manager_state_mut().open_save(&name);
                    }
                    ui.close_menu();
                }

                // USB Manager
                if ui
                    .add_enabled(
                        matches!(state, Some(VmState::Running)),
                        egui::Button::new(t!("menu.vm.usb")),
                    )
                    .clicked()
                {
                    app.usb_manager_state_mut().open();
                    ui.close_menu();
                }

                // Port Forwarding
                if ui.button(t!("menu.vm.port-forward")).clicked() {
                    app.action_open_port_forwards();
                    ui.close_menu();
                }

                // Install Guest Tools
                if ui
                    .add_enabled(
                        matches!(state, Some(VmState::Running)),
                        egui::Button::new(t!("menu.vm.guest-tools")),
                    )
                    .clicked()
                {
                    app.action_open_guest_tools();
                    ui.close_menu();
                }

                // CD/DVD Media
                if ui
                    .add_enabled(
                        matches!(state, Some(VmState::Running)),
                        egui::Button::new(t!("menu.vm.media")),
                    )
                    .clicked()
                {
                    app.action_open_media_dialog();
                    ui.close_menu();
                }

                // Host-Guest Integration
                if ui
                    .add_enabled(
                        matches!(state, Some(VmState::Running)),
                        egui::Button::new(t!("menu.vm.host-guest")),
                    )
                    .clicked()
                {
                    app.action_open_host_guest();
                    ui.close_menu();
                }

                // Live Migration
                if ui
                    .add_enabled(
                        matches!(state, Some(VmState::Running)),
                        egui::Button::new(t!("menu.vm.migrate")),
                    )
                    .clicked()
                {
                    app.action_open_migration();
                    ui.close_menu();
                }

                ui.separator();

                // Parallels-inspired features
                if ui.button(t!("menu.vm.rollback")).clicked() {
                    app.action_open_rollback();
                    ui.close_menu();
                }

                if ui.button(t!("menu.vm.disk-manage")).clicked() {
                    app.action_open_disk_manage();
                    ui.close_menu();
                }

                if ui
                    .add_enabled(
                        app.selected_vm().is_some(),
                        egui::Button::new(t!("menu.vm.backup")),
                    )
                    .clicked()
                {
                    app.backup_restore_state_mut().open();
                    if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                        app.backup_restore_state_mut().refresh_backups(&name);
                    }
                    ui.close_menu();
                }

                if ui
                    .add_enabled(
                        matches!(state, Some(VmState::Running)),
                        egui::Button::new(t!("menu.vm.net-cond")),
                    )
                    .clicked()
                {
                    app.action_open_net_cond();
                    ui.close_menu();
                }

                ui.separator();

                // Wave 4: Enterprise features
                if ui
                    .add_enabled(
                        matches!(state, Some(VmState::Running)),
                        egui::Button::new(t!("menu.vm.novnc")),
                    )
                    .clicked()
                {
                    app.action_open_novnc_panel();
                    ui.close_menu();
                }

                if ui.button(t!("menu.vm.unattended")).clicked() {
                    app.action_open_unattended_wizard();
                    ui.close_menu();
                }

                // Change Disk Password (only when encrypted and off)
                if let Some(config) = app.selected_vm_config() {
                    if config.disk_encrypted {
                        if ui
                            .add_enabled(is_off, egui::Button::new(t!("menu.vm.change-passphrase")))
                            .clicked()
                        {
                            if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                                app.action_open_change_passphrase(&name);
                            }
                            ui.close_menu();
                        }
                    }
                }

                // Guest File Manager
                if ui
                    .add_enabled(
                        matches!(state, Some(VmState::Running)),
                        egui::Button::new(t!("menu.vm.file-manager")),
                    )
                    .clicked()
                {
                    app.action_open_guest_file_manager();
                    ui.close_menu();
                }

                // Screen Recording submenu
                ui.menu_button(t!("menu.vm.recording"), |ui| {
                    let is_recording = app.screen_recording_state().is_recording();
                    if !is_recording {
                        if ui.button(t!("menu.vm.recording.start")).clicked() {
                            app.action_start_recording();
                            ui.close_menu();
                        }
                    } else {
                        if ui.button(t!("menu.vm.recording.stop")).clicked() {
                            app.action_stop_recording();
                            ui.close_menu();
                        }
                    }
                    if ui.button(t!("menu.vm.recording.screenshot")).clicked() {
                        app.action_take_screenshot();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(t!("menu.vm.recording.settings")).clicked() {
                        app.action_toggle_recording_settings();
                        ui.close_menu();
                    }
                });

                ui.separator();

                // Settings - always available now (metadata editable while running)
                if ui.button(t!("menu.vm.settings")).clicked() {
                    if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                        // Load config for editing
                        if let Some(config) = app.selected_vm_config().cloned() {
                            app.set_editing_config(Some(config));
                            app.set_screen(Screen::VmSettings(name));
                        }
                    }
                    ui.close_menu();
                }

                ui.separator();

                if ui
                    .button(egui::RichText::new(t!("menu.vm.delete")).color(AppColors::DANGER))
                    .clicked()
                {
                    if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                        app.set_confirm_delete(Some(name));
                    }
                    ui.close_menu();
                }
            });

            // ===== VIEW MENU =====
            ui.menu_button(t!("menu.view"), |ui| {
                let sidebar_label = if app.sidebar_visible() {
                    t!("menu.view.hide-sidebar")
                } else {
                    t!("menu.view.show-sidebar")
                };
                if ui.button(sidebar_label).clicked() {
                    app.toggle_sidebar();
                    ui.close_menu();
                }

                let log_label = if app.show_event_log() {
                    t!("menu.view.hide-log")
                } else {
                    t!("menu.view.show-log")
                };
                if ui.button(log_label).clicked() {
                    app.toggle_event_log();
                    ui.close_menu();
                }

                let task_label = if app.show_task_panel() {
                    t!("menu.view.hide-tasks")
                } else {
                    t!("menu.view.show-tasks")
                };
                if ui.button(task_label).clicked() {
                    app.set_show_task_panel(!app.show_task_panel());
                    ui.close_menu();
                }

                ui.separator();

                if ui.button(t!("menu.view.refresh")).clicked() {
                    app.action_refresh();
                    ui.close_menu();
                }

                ui.separator();

                // Parallels-inspired: PiP
                let pip_label = if app.pip_state().open {
                    t!("menu.view.close-pip")
                } else {
                    t!("menu.view.pip")
                };
                if ui
                    .add_enabled(
                        app.console_framebuffer().is_some(),
                        egui::Button::new(pip_label),
                    )
                    .clicked()
                {
                    app.action_toggle_pip();
                    ui.close_menu();
                }

                // Display Auto-Resize
                let auto_resize = app.display_auto_resize();
                let ar_label = if auto_resize {
                    format!("\u{2714} {}", t!("menu.view.auto-resize-on"))
                } else {
                    t!("menu.view.auto-resize-off").into_owned()
                };
                if ui
                    .button(ar_label)
                    .on_hover_text(t!("menu.view.auto-resize-tooltip"))
                    .clicked()
                {
                    app.set_display_auto_resize(!auto_resize);
                    ui.close_menu();
                }

                // Parallels-inspired: Auto-Pause
                let auto_pause = app.auto_pause_enabled();
                let ap_label = if auto_pause {
                    format!("\u{2714} {}", t!("menu.view.auto-pause-on"))
                } else {
                    t!("menu.view.auto-pause-off").into_owned()
                };
                if ui
                    .button(ap_label)
                    .on_hover_text(t!("menu.view.auto-pause-tooltip"))
                    .clicked()
                {
                    app.set_auto_pause(!auto_pause);
                    ui.close_menu();
                }
            });

            // ===== CONFIGURATION MENU =====
            ui.menu_button(t!("menu.config"), |ui| {
                if ui.button(t!("menu.config.language")).clicked() {
                    app.set_screen(Screen::Settings);
                    ui.close_menu();
                }
                if ui.button(t!("menu.config.display")).clicked() {
                    app.set_screen(Screen::Settings);
                    ui.close_menu();
                }
                if ui.button(t!("menu.config.default-hw")).clicked() {
                    app.set_screen(Screen::Settings);
                    ui.close_menu();
                }
                if ui.button(t!("menu.config.notifications")).clicked() {
                    app.set_screen(Screen::Settings);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(t!("menu.config.preferences")).clicked() {
                    app.set_screen(Screen::Settings);
                    ui.close_menu();
                }
            });

            // ===== HELP MENU =====
            ui.menu_button(t!("menu.help"), |ui| {
                if ui.button(t!("menu.help.about")).clicked() {
                    app.set_screen(Screen::Settings);
                    ui.close_menu();
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(t!("menu.help.shortcuts"))
                        .color(AppColors::TEXT)
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(t!("menu.help.shortcut.start"))
                        .size(FontSize::LABEL)
                        .color(AppColors::TEXT_DIM),
                );
                ui.label(
                    egui::RichText::new(t!("menu.help.shortcut.power-off"))
                        .size(FontSize::LABEL)
                        .color(AppColors::TEXT_DIM),
                );
                ui.label(
                    egui::RichText::new(t!("menu.help.shortcut.shutdown"))
                        .size(FontSize::LABEL)
                        .color(AppColors::TEXT_DIM),
                );
                ui.label(
                    egui::RichText::new(t!("menu.help.shortcut.pause"))
                        .size(FontSize::LABEL)
                        .color(AppColors::TEXT_DIM),
                );
                ui.label(
                    egui::RichText::new(t!("menu.help.shortcut.resume"))
                        .size(FontSize::LABEL)
                        .color(AppColors::TEXT_DIM),
                );
                ui.label(
                    egui::RichText::new(t!("menu.help.shortcut.reboot"))
                        .size(FontSize::LABEL)
                        .color(AppColors::TEXT_DIM),
                );
                ui.label(
                    egui::RichText::new(t!("menu.help.shortcut.console"))
                        .size(FontSize::LABEL)
                        .color(AppColors::TEXT_DIM),
                );
                ui.label(
                    egui::RichText::new(t!("menu.help.shortcut.console-manage"))
                        .size(FontSize::LABEL)
                        .color(AppColors::TEXT_DIM),
                );
                ui.label(
                    egui::RichText::new(t!("menu.help.shortcut.refresh"))
                        .size(FontSize::LABEL)
                        .color(AppColors::TEXT_DIM),
                );
            });

            // Right-aligned: search + settings
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t!("appsettings.title")).clicked() {
                    app.set_screen(Screen::Settings);
                }

                let search = app.search_query_mut();
                ui.add(
                    egui::TextEdit::singleline(search)
                        .hint_text(t!("sidebar.search"))
                        .desired_width(160.0),
                );
            });
        });
    });
}
