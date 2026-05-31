//! Left sidebar — persistent VM library list (VMware Workstation style).

use crate::app::{LibreVmmApp, ManageTab, Screen, SidebarSort, ViewMode};
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;
use vmm_core::config::VmConfig;
use vmm_core::domain::VmState;

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    egui::SidePanel::left("vm_sidebar")
        .default_width(210.0)
        .min_width(160.0)
        .max_width(320.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.add_space(Spacing::XS);

            // Header
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(t!("sidebar.title"))
                        .size(FontSize::BODY)
                        .strong()
                        .color(AppColors::TEXT_DIM),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Sort button
                    let sort_label = match app.sidebar_sort() {
                        SidebarSort::Favorites => "\u{2605}",
                        SidebarSort::Name => "A-Z",
                        SidebarSort::State => "\u{25CF}",
                    };
                    let sort_btn = ui.small_button(sort_label);
                    if sort_btn.clicked() {
                        let next = match app.sidebar_sort() {
                            SidebarSort::Favorites => SidebarSort::Name,
                            SidebarSort::Name => SidebarSort::State,
                            SidebarSort::State => SidebarSort::Favorites,
                        };
                        app.set_sidebar_sort(next);
                    }
                    sort_btn.on_hover_text(t!("sidebar.sort-tooltip"));

                    // VM count badge
                    let running = app
                        .vms()
                        .iter()
                        .filter(|v| v.state == VmState::Running)
                        .count();
                    if running > 0 {
                        ui.label(
                            egui::RichText::new(t!("sidebar.running", count = running))
                                .size(FontSize::TINY)
                                .color(AppColors::RUNNING),
                        );
                    }
                });
            });

            ui.add_space(Spacing::XS);

            // Search box
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("\u{1F50D}")
                        .size(FontSize::LABEL)
                        .color(AppColors::TEXT_DIM),
                );
                let search = app.search_query_mut();
                ui.add(
                    egui::TextEdit::singleline(search)
                        .desired_width(ui.available_width())
                        .hint_text(t!("sidebar.search"))
                        .font(egui::TextStyle::Small),
                );
            });

            // Remote host indicator
            if let Some(host_name) = app.remote_host_name().map(|s| s.to_string()) {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("\u{1F4E1} {}", host_name))
                            .size(FontSize::SMALL)
                            .color(AppColors::PRIMARY),
                    );
                    if ui
                        .small_button("Local")
                        .on_hover_text(t!("sidebar.switch-local"))
                        .clicked()
                    {
                        app.action_connect_local();
                    }
                });
            }

            ui.add_space(Spacing::XS);
            ui.separator();
            ui.add_space(Spacing::XS);

            // Connection error
            if app.connection_error().is_some() {
                ui.label(
                    egui::RichText::new(t!("status.disconnected"))
                        .size(FontSize::LABEL)
                        .color(AppColors::DANGER),
                );
                ui.add_space(Spacing::SM);
                return;
            }

            // VM list with sorting and favorites
            let search = app.search_query().to_lowercase();
            let sort = app.sidebar_sort().clone();

            // Use cached configs (loaded during app refresh cycle, not per-frame disk I/O)
            let configs = app.vm_configs();

            // Collect filtered VM indices to avoid cloning VmInfo structs
            let mut vm_indices: Vec<usize> = app
                .vms()
                .iter()
                .enumerate()
                .filter(|(_, vm)| search.is_empty() || vm.name.to_lowercase().contains(&search))
                .map(|(i, _)| i)
                .collect();

            // Pre-compute lowercase names once instead of calling to_lowercase()
            // O(N) times per comparison inside sort (which does O(N log N) comparisons).
            let lower_names: Vec<String> =
                app.vms().iter().map(|vm| vm.name.to_lowercase()).collect();

            // Build a config-name→favorite lookup to replace O(N) linear scan per comparison
            let fav_map: std::collections::HashMap<&str, bool> = configs
                .iter()
                .map(|c| (c.name.as_str(), c.favorite))
                .collect();

            // Sort using indices (avoids cloning VmInfo)
            match sort {
                SidebarSort::Favorites => {
                    vm_indices.sort_by(|&a, &b| {
                        let a_fav = fav_map
                            .get(app.vms()[a].name.as_str())
                            .copied()
                            .unwrap_or(false);
                        let b_fav = fav_map
                            .get(app.vms()[b].name.as_str())
                            .copied()
                            .unwrap_or(false);
                        b_fav
                            .cmp(&a_fav)
                            .then_with(|| lower_names[a].cmp(&lower_names[b]))
                    });
                },
                SidebarSort::Name => {
                    vm_indices.sort_by(|&a, &b| lower_names[a].cmp(&lower_names[b]));
                },
                SidebarSort::State => {
                    vm_indices.sort_by(|&a, &b| {
                        let state_order = |s: &VmState| -> u8 {
                            match s {
                                VmState::Running => 0,
                                VmState::Paused => 1,
                                VmState::Suspended => 2,
                                VmState::ShuttingDown => 3,
                                VmState::Off => 4,
                                VmState::Crashed => 5,
                                VmState::Unknown => 6,
                            }
                        };
                        state_order(&app.vms()[a].state)
                            .cmp(&state_order(&app.vms()[b].state))
                            .then_with(|| lower_names[a].cmp(&lower_names[b]))
                    });
                },
            }

            if vm_indices.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(t!("sidebar.no-vms"))
                            .size(FontSize::BODY)
                            .color(AppColors::TEXT_DIM),
                    );
                });
            } else {
                // Snapshot VM data needed for rendering to avoid borrow conflicts
                // (render_vm_entry_data needs &mut app, but we also read app.vms())
                struct VmEntry {
                    name: String,
                    state: VmState,
                    folder: Option<String>,
                    is_fav: bool,
                }

                // Build a name→config lookup to avoid O(N) linear scans per VM
                let cfg_map: std::collections::HashMap<&str, &VmConfig> =
                    configs.iter().map(|c| (c.name.as_str(), c)).collect();

                let entries: Vec<VmEntry> = vm_indices
                    .iter()
                    .map(|&i| {
                        let vm = &app.vms()[i];
                        let cfg = cfg_map.get(vm.name.as_str());
                        VmEntry {
                            name: vm.name.clone(),
                            state: vm.state.clone(),
                            folder: cfg.and_then(|c| c.folder.clone()),
                            is_fav: cfg.map(|c| c.favorite).unwrap_or(false),
                        }
                    })
                    .collect();

                // Group by folder using references into entries (no extra clones)
                let mut grouped: std::collections::BTreeMap<Option<&str>, Vec<&VmEntry>> =
                    std::collections::BTreeMap::new();
                for entry in &entries {
                    grouped
                        .entry(entry.folder.as_deref())
                        .or_default()
                        .push(entry);
                }

                let has_groups = grouped.keys().any(|k| k.is_some());

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Ungrouped VMs first
                        if let Some(ungrouped) = grouped.get(&None) {
                            for entry in ungrouped {
                                render_vm_entry_data(
                                    app,
                                    ui,
                                    &entry.name,
                                    &entry.state,
                                    entry.is_fav,
                                );
                            }
                            if has_groups && !ungrouped.is_empty() {
                                ui.add_space(Spacing::XS);
                            }
                        }

                        // Grouped VMs with collapsible headers
                        let collapsed = app.sidebar_collapsed_groups().clone();
                        let mut batch_start: Option<String> = None;
                        let mut batch_stop: Option<String> = None;
                        let mut toggle_group: Option<String> = None;

                        for (&folder, group_entries) in &grouped {
                            if let Some(group_name) = folder {
                                let is_collapsed = collapsed.contains(group_name);
                                let running_count = group_entries
                                    .iter()
                                    .filter(|e| e.state == VmState::Running)
                                    .count();

                                ui.add_space(2.0);
                                egui::Frame::none()
                                    .fill(AppColors::BG_CARD)
                                    .rounding(ThemeRounding::BUTTON_SMALL)
                                    .inner_margin(egui::Margin::symmetric(6.0, 3.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            // Collapse toggle
                                            let arrow =
                                                if is_collapsed { "\u{25B6}" } else { "\u{25BC}" };
                                            if ui
                                                .small_button(arrow)
                                                .on_hover_text(
                                                    t!("tooltip.toggle-group").to_string(),
                                                )
                                                .clicked()
                                            {
                                                toggle_group = Some(group_name.to_string());
                                            }
                                            ui.label(
                                                egui::RichText::new(group_name)
                                                    .size(FontSize::SMALL)
                                                    .strong()
                                                    .color(AppColors::PRIMARY),
                                            );
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "({})",
                                                    group_entries.len()
                                                ))
                                                .size(FontSize::TINY)
                                                .color(AppColors::TEXT_DIM),
                                            );
                                            if running_count > 0 {
                                                ui.label(
                                                    egui::RichText::new(t!(
                                                        "sidebar.group-running",
                                                        count = running_count
                                                    ))
                                                    .size(FontSize::CAPTION)
                                                    .color(AppColors::RUNNING),
                                                );
                                            }

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .small_button("\u{25A0}")
                                                        .on_hover_text(t!("sidebar.stop-all"))
                                                        .clicked()
                                                    {
                                                        batch_stop = Some(group_name.to_string());
                                                    }
                                                    if ui
                                                        .small_button("\u{25B6}")
                                                        .on_hover_text(t!("sidebar.start-all"))
                                                        .clicked()
                                                    {
                                                        batch_start = Some(group_name.to_string());
                                                    }
                                                },
                                            );
                                        });
                                    });

                                // Show VMs in group (unless collapsed)
                                if !is_collapsed {
                                    for entry in group_entries {
                                        render_vm_entry_data(
                                            app,
                                            ui,
                                            &entry.name,
                                            &entry.state,
                                            entry.is_fav,
                                        );
                                    }
                                }
                                ui.add_space(2.0);
                            }
                        }

                        // Process deferred actions
                        if let Some(g) = toggle_group {
                            app.toggle_sidebar_group(&g);
                        }
                        if let Some(g) = batch_start {
                            app.action_batch_start(&g);
                        }
                        if let Some(g) = batch_stop {
                            app.action_batch_stop(&g);
                        }
                    });
            }

            // Bottom: New VM button
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(Spacing::XS);

                let btn = egui::Button::new(
                    egui::RichText::new(t!("sidebar.new-vm"))
                        .size(FontSize::BODY)
                        .color(egui::Color32::WHITE),
                )
                .fill(AppColors::PRIMARY)
                .rounding(ThemeRounding::BUTTON)
                .min_size(egui::vec2(ui.available_width(), 30.0));

                if ui.add(btn).clicked() && app.is_connected() {
                    app.set_screen(Screen::BoxSelector);
                }

                ui.add_space(Spacing::XS);
                ui.separator();
            });
        });
}

/// Render a single VM entry using pre-extracted data (avoids borrowing app.vms() during render).
fn render_vm_entry_data(
    app: &mut LibreVmmApp,
    ui: &mut egui::Ui,
    vm_name: &str,
    vm_state: &VmState,
    is_favorite: bool,
) {
    let is_selected = app.selected_vm() == Some(vm_name);

    let bg = if is_selected {
        AppColors::BG_HOVER
    } else {
        egui::Color32::TRANSPARENT
    };

    let frame = egui::Frame::none()
        .fill(bg)
        .rounding(ThemeRounding::BUTTON_SMALL)
        .inner_margin(egui::Margin::symmetric(6.0, Spacing::XS));

    let response = frame
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Favorite star
                if is_favorite {
                    ui.label(
                        egui::RichText::new("\u{2605}")
                            .size(FontSize::TINY)
                            .color(AppColors::STAR_COLOR),
                    );
                }

                // State dot
                let state_color = match vm_state {
                    VmState::Running => AppColors::RUNNING,
                    VmState::Paused => AppColors::PAUSED,
                    VmState::Crashed => AppColors::CRASHED,
                    _ => AppColors::OFF,
                };

                let (dot_rect, _) =
                    ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(dot_rect.center(), 4.0, state_color);

                // VM name + state
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(vm_name)
                            .size(FontSize::BODY)
                            .color(AppColors::TEXT)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(vm_state.as_str())
                            .size(FontSize::TINY)
                            .color(state_color),
                    );
                });
            });
        })
        .response;

    // Click to select
    if response.interact(egui::Sense::click()).clicked() {
        app.set_selected_vm(Some(vm_name.to_string()));
        app.set_screen(Screen::Home);
    }

    // Right-click context menu
    let name = vm_name.to_string();
    let state = vm_state.clone();
    response.context_menu(|ui| {
        ui.label(egui::RichText::new(&name).strong().color(AppColors::TEXT));
        ui.separator();

        // Favorite toggle
        let fav_label = if is_favorite {
            t!("sidebar.unfavorite")
        } else {
            t!("sidebar.favorite")
        };
        if ui.button(fav_label).clicked() {
            app.toggle_vm_favorite(&name);
            ui.close_menu();
        }

        ui.separator();

        // Power options
        match state {
            VmState::Off | VmState::Crashed => {
                if ui.button(t!("power.start")).clicked() {
                    app.action_start(&name);
                    ui.close_menu();
                }
            },
            VmState::Running => {
                if ui.button(t!("power.shutdown")).clicked() {
                    app.action_shutdown(&name);
                    ui.close_menu();
                }
                if ui.button(t!("power.pause")).clicked() {
                    app.action_pause(&name);
                    ui.close_menu();
                }
                if ui.button(t!("power.reboot")).clicked() {
                    app.action_reboot(&name);
                    ui.close_menu();
                }
                if ui
                    .button(egui::RichText::new(t!("power.power-off")).color(AppColors::DANGER))
                    .clicked()
                {
                    app.action_force_stop(&name);
                    ui.close_menu();
                }
            },
            VmState::Paused | VmState::Suspended => {
                if ui.button(t!("power.resume")).clicked() {
                    app.action_resume(&name);
                    ui.close_menu();
                }
                if ui
                    .button(egui::RichText::new(t!("power.power-off")).color(AppColors::DANGER))
                    .clicked()
                {
                    app.action_force_stop(&name);
                    ui.close_menu();
                }
            },
            _ => {},
        }

        ui.separator();

        if state == VmState::Running {
            if ui.button(t!("menu.vm.console")).clicked() {
                app.action_console(&name);
                ui.close_menu();
            }
            if ui.button(t!("sidebar.performance")).clicked() {
                app.set_selected_vm(Some(name.clone()));
                app.set_view_mode(ViewMode::Manage(ManageTab::Performance));
                app.set_screen(Screen::Home);
                ui.close_menu();
            }
            ui.separator();
        }

        if ui.button(t!("menu.vm.snapshots")).clicked() {
            app.set_selected_vm(Some(name.clone()));
            app.set_view_mode(ViewMode::Manage(ManageTab::Snapshots));
            app.set_screen(Screen::Home);
            ui.close_menu();
        }

        // Live Migrate (Wave 12.1) — open the migration wizard with this VM preselected.
        if ui.button(t!("menu.vm.migrate")).clicked() {
            app.set_selected_vm(Some(name.clone()));
            app.action_open_migration();
            ui.close_menu();
        }

        let is_off = matches!(state, VmState::Off | VmState::Crashed);

        // Clone
        if ui
            .add_enabled(is_off, egui::Button::new(t!("menu.vm.clone")))
            .clicked()
        {
            app.set_selected_vm(Some(name.clone()));
            app.clone_dialog_state_mut().open(&name);
            ui.close_menu();
        }

        // Export
        if ui.button(t!("menu.file.export-ova")).clicked() {
            app.set_selected_vm(Some(name.clone()));
            app.import_export_state_mut().open_export(&name);
            ui.close_menu();
        }

        // Save as Template
        if ui.button(t!("menu.vm.save-template")).clicked() {
            app.set_selected_vm(Some(name.clone()));
            app.template_manager_state_mut().open_save(&name);
            ui.close_menu();
        }

        if ui
            .add_enabled(is_off, egui::Button::new(t!("menu.vm.settings")))
            .clicked()
        {
            app.set_selected_vm(Some(name.clone()));
            if let Some(config) = app.selected_vm_config().cloned() {
                app.set_editing_config(Some(config));
                app.set_screen(Screen::VmSettings(name.clone()));
            }
            ui.close_menu();
        }

        ui.separator();

        if ui
            .button(egui::RichText::new(t!("menu.vm.delete")).color(AppColors::DANGER))
            .clicked()
        {
            app.set_confirm_delete(Some(name.clone()));
            ui.close_menu();
        }
    });
}
