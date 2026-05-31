//! Live Migration wizard — migrate VMs between hypervisor hosts.
//!
//! Polished as a four-step flow (Wave 12.1):
//!   1. Choose destination host (from configured remote hosts)
//!   2. Choose migration type (Live / Offline / P2P) + options
//!   3. Confirm and run
//!   4. Live progress / final result
//!
//! Uses the same step-indicator + accent-stripe pattern as the other
//! wizards (wizard.rs, arch_wizard.rs, power_wizard.rs).

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::{AppColors, ThemeRounding};
use eframe::egui;
use rust_i18n::t;
use vmm_core::migration::{
    MigrationOptions, MigrationPreflight, MigrationProgress, MigrationType, SharedMigrationProgress,
};
use vmm_core::remote::RemoteHostsConfig;

/// Visible steps of the migration wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MigrationStep {
    #[default]
    ChooseHost,
    ChooseType,
    Confirm,
    InProgress,
    Done,
}

/// Migration wizard state.
#[derive(Default)]
pub struct MigrationState {
    pub open: bool,
    /// Current wizard step.
    pub step: MigrationStep,
    /// Index into remote hosts list (the selected destination).
    pub selected_host_idx: Option<usize>,
    pub options: MigrationOptions,
    /// Preflight check results (run on demand in the Confirm step).
    pub preflight: Option<MigrationPreflight>,
    pub preflight_running: bool,
    /// Active migration progress handle.
    pub progress: Option<SharedMigrationProgress>,
    /// SECURITY: CWE-404 — Store migration thread JoinHandle for orderly cleanup.
    /// Previously the handle was dropped (detached thread), preventing shutdown cleanup.
    pub migration_thread: Option<std::thread::JoinHandle<()>>,
    /// Cached progress snapshot for UI rendering.
    pub progress_snapshot: MigrationProgress,
    /// Error message (general — surfaced on the current step).
    pub error: Option<String>,
    /// VM name being migrated.
    pub vm_name: String,
    /// Cached remote hosts.
    pub remote_hosts: Option<RemoteHostsConfig>,
}

impl MigrationState {
    pub fn open(&mut self, vm_name: &str) {
        self.open = true;
        self.step = MigrationStep::ChooseHost;
        self.vm_name = vm_name.to_string();
        self.selected_host_idx = None;
        self.options = MigrationOptions::default();
        self.preflight = None;
        self.preflight_running = false;
        self.progress = None;
        self.progress_snapshot = MigrationProgress::default();
        self.error = None;
        // Load remote hosts
        self.remote_hosts = Some(RemoteHostsConfig::load());
    }

    /// Close the migration dialog and tear down any associated worker thread.
    /// Returns `true` if the migration thread was still running at close time
    /// (i.e., the caller should warn the user that the migration is being
    /// cancelled in the background). Returns `false` for a clean close.
    pub fn close(&mut self) -> bool {
        self.open = false;
        let vm_name = std::mem::take(&mut self.vm_name);
        self.progress = None;
        // SECURITY: CWE-404 — Join migration thread on close for orderly cleanup.
        let mut still_running = false;
        if let Some(jh) = self.migration_thread.take() {
            if jh.is_finished() {
                let _ = jh.join();
            } else {
                // Ask the migration to cancel via virsh domjobabort. The worker
                // thread will then exit and be joined on Drop of LibreVmmApp.
                // Without this call we'd leak the migration job server-side.
                if !vm_name.is_empty() {
                    if let Err(e) = vmm_core::migration::cancel_migration(&vm_name) {
                        tracing::warn!(
                            "Failed to cancel migration for '{}' on dialog close: {}",
                            vm_name,
                            e
                        );
                    }
                }
                tracing::warn!(
                    "Migration dialog closed while migration thread still running; \
                     a cancellation has been requested via cancel_migration()"
                );
                still_running = true;
            }
        }
        self.preflight = None;
        self.remote_hosts = None;
        still_running
    }

    /// Whether a migration is currently in progress.
    pub fn is_migrating(&self) -> bool {
        self.progress.is_some() && !self.progress_snapshot.completed
    }
}

// Accent used across the wizard. We pick the primary blue here — migration
// is a core feature, not box-type specific.
const ACCENT: egui::Color32 = AppColors::PRIMARY;

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    if !app.migration_state().open {
        return;
    }

    let mut open = true;

    egui::Window::new(t!("migration.window-title").to_string())
        .open(&mut open)
        .resizable(true)
        .default_width(620.0)
        .default_height(540.0)
        .min_width(480.0)
        .show(ctx, |ui| {
            // Sync progress snapshot from the background migration thread on each frame.
            let progress_arc = app.migration_state().progress.clone();
            if let Some(ref progress) = progress_arc {
                if let Ok(p) = progress.lock() {
                    app.migration_state_mut().progress_snapshot = p.clone();
                }
            }

            // Auto-advance from InProgress → Done when the worker reports completion.
            let snap_done = app.migration_state().progress_snapshot.completed;
            let in_progress = app.migration_state().step == MigrationStep::InProgress;
            if in_progress && snap_done {
                app.migration_state_mut().step = MigrationStep::Done;
            }

            // Accent stripe (matches the other wizards' visual language).
            let stripe = ui.allocate_space(egui::vec2(ui.available_width(), 3.0));
            ui.painter().rect_filled(stripe.1, 0.0, ACCENT);
            ui.add_space(6.0);

            // Header
            let vm_name = app.migration_state().vm_name.clone();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(t!("migration.header"))
                        .color(ACCENT)
                        .strong()
                        .size(14.0),
                );
                ui.label(egui::RichText::new("  |  ").color(AppColors::MUTED));
                ui.label(
                    egui::RichText::new(&vm_name)
                        .color(AppColors::TEXT)
                        .strong()
                        .size(14.0),
                );
            });
            ui.add_space(6.0);

            // Step indicator
            let current = app.migration_state().step;
            render_step_indicator(ui, current);
            ui.add_space(theme::Spacing::SM);
            ui.separator();
            ui.add_space(theme::Spacing::SM);

            match current {
                MigrationStep::ChooseHost => render_step_choose_host(app, ui),
                MigrationStep::ChooseType => render_step_choose_type(app, ui),
                MigrationStep::Confirm => render_step_confirm(app, ui),
                MigrationStep::InProgress => render_step_progress(app, ui),
                MigrationStep::Done => render_step_done(app, ui),
            }
        });

    if !open {
        close_dialog(app);
    }
}

// ===================== Step indicator =====================

fn render_step_indicator(ui: &mut egui::Ui, current: MigrationStep) {
    let steps = [
        (
            MigrationStep::ChooseHost,
            t!("migration.step-host").to_string(),
        ),
        (
            MigrationStep::ChooseType,
            t!("migration.step-type").to_string(),
        ),
        (
            MigrationStep::Confirm,
            t!("migration.step-confirm").to_string(),
        ),
        (
            MigrationStep::InProgress,
            t!("migration.step-progress").to_string(),
        ),
    ];

    let current_idx = match current {
        MigrationStep::ChooseHost => 0,
        MigrationStep::ChooseType => 1,
        MigrationStep::Confirm => 2,
        MigrationStep::InProgress | MigrationStep::Done => 3,
    };

    ui.horizontal(|ui| {
        for (i, (_, label)) in steps.iter().enumerate() {
            let is_active = i == current_idx;
            let is_done = i < current_idx;
            let color = if is_active {
                ACCENT
            } else if is_done {
                AppColors::SUCCESS
            } else {
                AppColors::TEXT_DIM
            };
            let prefix = if is_done { "\u{2713} " } else { "" };
            ui.label(
                egui::RichText::new(format!("{}{}. {}", prefix, i + 1, label))
                    .color(color)
                    .strong(),
            );
            if i < steps.len() - 1 {
                ui.label(egui::RichText::new(" \u{203A} ").color(AppColors::MUTED));
            }
        }
    });
}

// ===================== Step 1: Choose Host =====================

fn render_step_choose_host(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let vm_name = app.migration_state().vm_name.clone();

    ui.label(
        egui::RichText::new(t!("migration.choose-host-title", name = vm_name))
            .color(AppColors::TEXT)
            .strong()
            .size(15.0),
    );
    ui.add_space(6.0);

    let hosts = app
        .migration_state()
        .remote_hosts
        .clone()
        .unwrap_or_default();

    if hosts.hosts.is_empty() {
        egui::Frame::none()
            .fill(AppColors::BG_CARD)
            .rounding(ThemeRounding::BUTTON)
            .inner_margin(egui::Margin::same(16.0))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(t!("migration.no-remotes-title"))
                        .color(AppColors::TEXT)
                        .strong(),
                );
                ui.add_space(theme::Spacing::XS);
                ui.label(
                    egui::RichText::new(t!("migration.no-remotes-body"))
                        .color(AppColors::TEXT_DIM)
                        .size(12.0),
                );
                ui.add_space(theme::Spacing::SM);
                let btn = egui::Button::new(
                    egui::RichText::new(t!("migration.add-remote")).color(egui::Color32::WHITE),
                )
                .fill(ACCENT)
                .rounding(ThemeRounding::BUTTON);
                if ui.add(btn).clicked() {
                    app.remote_hosts_state_mut().open();
                }
            });
        ui.add_space(theme::Spacing::MD);
        nav_buttons(
            app,
            ui,
            NavConfig {
                back: None,
                next: None,
                cancel: true,
            },
        );
        return;
    }

    // Render a card per remote host
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 70.0)
        .show(ui, |ui| {
            let selected_idx = app.migration_state().selected_host_idx;
            let mut click_idx: Option<usize> = None;

            for (i, host) in hosts.hosts.iter().enumerate() {
                let is_selected = selected_idx == Some(i);
                let stroke = if is_selected {
                    egui::Stroke::new(1.5, ACCENT)
                } else {
                    egui::Stroke::new(0.5, AppColors::BG_HOVER)
                };
                let fill = if is_selected {
                    ACCENT.linear_multiply(0.18)
                } else {
                    AppColors::BG_CARD
                };

                let frame = egui::Frame::none()
                    .fill(fill)
                    .rounding(ThemeRounding::BUTTON)
                    .stroke(stroke)
                    .inner_margin(egui::Margin::same(10.0));

                let response = frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Status dot (last-tested reachability)
                        let (dot_rect, _) =
                            ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                        // We don't poll continuously here — only paint a neutral dot
                        // until a preflight has run for this host in the Confirm step.
                        // Use the host's last preflight status if it's the selected one.
                        let dot_color = if is_selected {
                            match &app.migration_state().preflight {
                                Some(pf) if pf.all_ok() => AppColors::SUCCESS,
                                Some(_) => AppColors::DANGER,
                                None => AppColors::MUTED,
                            }
                        } else {
                            AppColors::MUTED
                        };
                        ui.painter()
                            .circle_filled(dot_rect.center(), 5.0, dot_color);

                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(&host.name)
                                    .color(AppColors::TEXT)
                                    .strong()
                                    .size(14.0),
                            );
                            let user_part = if host.username.is_empty() {
                                host.hostname.clone()
                            } else {
                                format!("{}@{}", host.username, host.hostname)
                            };
                            let uri_preview = if host.ssh_port != 22 {
                                format!("qemu+ssh://{}:{}/...", user_part, host.ssh_port)
                            } else {
                                format!("qemu+ssh://{}/...", user_part)
                            };
                            ui.label(
                                egui::RichText::new(uri_preview)
                                    .color(AppColors::TEXT_DIM)
                                    .size(theme::FontSize::SMALL)
                                    .family(egui::FontFamily::Monospace),
                            );
                        });
                    });
                });

                if response.response.interact(egui::Sense::click()).clicked() {
                    click_idx = Some(i);
                }
                ui.add_space(theme::Spacing::XS);
            }

            if let Some(i) = click_idx {
                let st = app.migration_state_mut();
                st.selected_host_idx = Some(i);
                // Reset preflight when switching hosts.
                st.preflight = None;
                st.error = None;
            }

            ui.add_space(6.0);
            if ui.button(t!("migration.manage-remotes")).clicked() {
                app.remote_hosts_state_mut().open();
            }
        });

    ui.add_space(theme::Spacing::SM);
    ui.separator();
    ui.add_space(6.0);

    let has_selection = app.migration_state().selected_host_idx.is_some();
    nav_buttons(
        app,
        ui,
        NavConfig {
            back: None,
            next: Some(NextAction {
                label: t!("migration.next-type").to_string(),
                enabled: has_selection,
                target: MigrationStep::ChooseType,
            }),
            cancel: true,
        },
    );
}

// ===================== Step 2: Choose Type =====================

fn render_step_choose_type(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new(t!("migration.type-title"))
            .color(AppColors::TEXT)
            .strong()
            .size(15.0),
    );
    ui.add_space(theme::Spacing::SM);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 70.0)
        .show(ui, |ui| {
            // Radio cards for each migration type.
            let current = app.migration_state().options.migration_type.clone();
            let mut new_type: Option<MigrationType> = None;

            for (mt, title_key, body_key) in [
                (
                    MigrationType::Live,
                    "migration.type-live",
                    "migration.type-live-desc",
                ),
                (
                    MigrationType::Offline,
                    "migration.type-offline",
                    "migration.type-offline-desc",
                ),
                (
                    MigrationType::PeerToPeer,
                    "migration.type-p2p",
                    "migration.type-p2p-desc",
                ),
            ] {
                let is_selected = current == mt;
                let fill = if is_selected {
                    ACCENT.linear_multiply(0.18)
                } else {
                    AppColors::BG_CARD
                };
                let stroke = if is_selected {
                    egui::Stroke::new(1.5, ACCENT)
                } else {
                    egui::Stroke::new(0.5, AppColors::BG_HOVER)
                };
                let frame = egui::Frame::none()
                    .fill(fill)
                    .rounding(ThemeRounding::BUTTON)
                    .stroke(stroke)
                    .inner_margin(egui::Margin::same(10.0));

                let response = frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let dot = if is_selected { "\u{25C9}" } else { "\u{25CB}" };
                        ui.label(
                            egui::RichText::new(dot)
                                .color(ACCENT)
                                .size(theme::FontSize::HEADING),
                        );
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(t!(title_key))
                                    .color(AppColors::TEXT)
                                    .strong()
                                    .size(theme::FontSize::BODY),
                            );
                            ui.label(
                                egui::RichText::new(t!(body_key))
                                    .color(AppColors::TEXT_DIM)
                                    .size(theme::FontSize::SMALL),
                            );
                        });
                    });
                });
                if response.response.interact(egui::Sense::click()).clicked() {
                    new_type = Some(mt);
                }
                ui.add_space(theme::Spacing::XS);
            }

            if let Some(t) = new_type {
                app.migration_state_mut().options.migration_type = t;
            }

            ui.add_space(theme::Spacing::SM);
            ui.separator();
            ui.add_space(theme::Spacing::SM);

            // Options group (compression / bandwidth / extras)
            ui.label(
                egui::RichText::new(t!("migration.options-title"))
                    .color(ACCENT)
                    .strong(),
            );
            ui.add_space(theme::Spacing::XS);

            // Compression (the core API exposes this as `compressed`)
            let mut compressed = app.migration_state().options.compressed;
            if ui
                .checkbox(&mut compressed, t!("migration.opt-compressed").to_string())
                .changed()
            {
                app.migration_state_mut().options.compressed = compressed;
            }
            ui.label(
                egui::RichText::new(t!("migration.opt-compressed-desc"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::SMALL),
            );

            // Bandwidth cap
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(t!("migration.opt-bandwidth"));
                let mut bw = app.migration_state().options.bandwidth_mib as f32;
                if ui
                    .add(
                        egui::DragValue::new(&mut bw)
                            .range(0.0..=10000.0)
                            .speed(10.0),
                    )
                    .changed()
                {
                    app.migration_state_mut().options.bandwidth_mib = bw as u64;
                }
                ui.label(
                    egui::RichText::new(t!("migration.opt-bandwidth-unit"))
                        .color(AppColors::TEXT_DIM)
                        .size(theme::FontSize::SMALL),
                );
            });
            ui.label(
                egui::RichText::new(t!("migration.opt-bandwidth-desc"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::SMALL),
            );

            // Copy storage (local disks)
            ui.add_space(6.0);
            let mut copy = app.migration_state().options.copy_storage;
            if ui
                .checkbox(&mut copy, t!("migration.opt-copy-storage").to_string())
                .changed()
            {
                app.migration_state_mut().options.copy_storage = copy;
            }
            ui.label(
                egui::RichText::new(t!("migration.opt-copy-storage-desc"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::SMALL),
            );

            // Auto-converge
            ui.add_space(6.0);
            let mut ac = app.migration_state().options.auto_converge;
            if ui
                .checkbox(&mut ac, t!("migration.opt-auto-converge").to_string())
                .changed()
            {
                app.migration_state_mut().options.auto_converge = ac;
            }
            ui.label(
                egui::RichText::new(t!("migration.opt-auto-converge-desc"))
                    .color(AppColors::TEXT_DIM)
                    .size(theme::FontSize::SMALL),
            );

            // Persist + undefine
            ui.add_space(6.0);
            let mut persistent = app.migration_state().options.persistent;
            if ui
                .checkbox(&mut persistent, t!("migration.opt-persistent").to_string())
                .changed()
            {
                app.migration_state_mut().options.persistent = persistent;
            }
            let mut undefine = app.migration_state().options.undefine_source;
            if ui
                .checkbox(&mut undefine, t!("migration.opt-undefine").to_string())
                .changed()
            {
                app.migration_state_mut().options.undefine_source = undefine;
            }
        });

    ui.add_space(theme::Spacing::SM);
    ui.separator();
    ui.add_space(6.0);

    nav_buttons(
        app,
        ui,
        NavConfig {
            back: Some(MigrationStep::ChooseHost),
            next: Some(NextAction {
                label: t!("migration.next-confirm").to_string(),
                enabled: true,
                target: MigrationStep::Confirm,
            }),
            cancel: true,
        },
    );
}

// ===================== Step 3: Confirm =====================

fn render_step_confirm(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let vm_name = app.migration_state().vm_name.clone();
    let hosts = app
        .migration_state()
        .remote_hosts
        .clone()
        .unwrap_or_default();
    let host_idx = app.migration_state().selected_host_idx;
    let host = host_idx.and_then(|i| hosts.hosts.get(i).cloned());

    let Some(host) = host else {
        ui.label(egui::RichText::new(t!("migration.err-no-host")).color(AppColors::DANGER));
        nav_buttons(
            app,
            ui,
            NavConfig {
                back: Some(MigrationStep::ChooseHost),
                next: None,
                cancel: true,
            },
        );
        return;
    };

    let mtype = app.migration_state().options.migration_type.clone();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 100.0)
        .show(ui, |ui| {
            // Summary card
            egui::Frame::none()
                .fill(AppColors::BG_CARD)
                .rounding(ThemeRounding::BUTTON)
                .inner_margin(egui::Margin::same(14.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(t!(
                            "migration.confirm-summary",
                            name = vm_name,
                            kind = mtype.to_string(),
                            host = host.name.clone()
                        ))
                        .color(AppColors::TEXT)
                        .strong()
                        .size(14.0),
                    );
                    ui.add_space(theme::Spacing::XS);
                    ui.label(
                        egui::RichText::new(host.connection_uri())
                            .color(AppColors::TEXT_DIM)
                            .size(theme::FontSize::SMALL)
                            .family(egui::FontFamily::Monospace),
                    );
                });
            ui.add_space(theme::Spacing::SM);

            // Local disk warning. The core API uses `copy_storage` for this.
            // If the user hasn't enabled it, warn that local-only disks won't be visible.
            if !app.migration_state().options.copy_storage {
                egui::Frame::none()
                    .fill(AppColors::WARNING.linear_multiply(0.15))
                    .rounding(ThemeRounding::BUTTON)
                    .stroke(egui::Stroke::new(1.0, AppColors::WARNING))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("\u{26A0}")
                                    .color(AppColors::WARNING)
                                    .size(theme::FontSize::HEADING),
                            );
                            ui.label(
                                egui::RichText::new(t!("migration.warn-local-disks"))
                                    .color(AppColors::TEXT)
                                    .size(12.0),
                            );
                        });
                    });
                ui.add_space(theme::Spacing::SM);
            }

            // Preflight section
            ui.label(
                egui::RichText::new(t!("migration.preflight-title"))
                    .color(ACCENT)
                    .strong(),
            );
            ui.add_space(theme::Spacing::XS);

            let pf_running = app.migration_state().preflight_running;
            let has_pf = app.migration_state().preflight.is_some();
            ui.horizontal(|ui| {
                if pf_running {
                    ui.spinner();
                    ui.label(t!("migration.preflight-running"));
                } else if ui
                    .button(if has_pf {
                        t!("migration.preflight-rerun").to_string()
                    } else {
                        t!("migration.preflight-run").to_string()
                    })
                    .clicked()
                {
                    app.migration_state_mut().preflight_running = true;
                    let pf_host = host.clone();
                    let result = vmm_core::migration::preflight_check(&pf_host);
                    match result {
                        Ok(pf) => {
                            app.migration_state_mut().preflight = Some(pf);
                        },
                        Err(e) => {
                            app.migration_state_mut().error = Some(e.to_string());
                        },
                    }
                    app.migration_state_mut().preflight_running = false;
                }
            });

            if let Some(ref pf) = app.migration_state().preflight {
                ui.add_space(theme::Spacing::XS);
                egui::Frame::none()
                    .fill(AppColors::BG_CARD)
                    .rounding(ThemeRounding::BUTTON)
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        for (name, ok, detail) in pf.summary() {
                            ui.horizontal(|ui| {
                                let (icon, color) = if ok {
                                    ("\u{2713}", AppColors::SUCCESS)
                                } else {
                                    ("\u{2717}", AppColors::DANGER)
                                };
                                ui.label(egui::RichText::new(icon).color(color));
                                ui.label(egui::RichText::new(&name).color(AppColors::TEXT));
                                ui.label(
                                    egui::RichText::new(&detail)
                                        .color(AppColors::TEXT_DIM)
                                        .size(theme::FontSize::SMALL),
                                );
                            });
                        }
                    });
            }

            if let Some(ref err) = app.migration_state().error {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(err).color(AppColors::DANGER));
            }
        });

    ui.add_space(theme::Spacing::SM);
    ui.separator();
    ui.add_space(6.0);

    // Action row
    ui.horizontal(|ui| {
        if ui
            .button(format!("\u{25C0}  {}", t!("migration.back")))
            .clicked()
        {
            app.migration_state_mut().step = MigrationStep::ChooseType;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let start_btn = egui::Button::new(
                egui::RichText::new(t!("migration.start"))
                    .color(egui::Color32::WHITE)
                    .strong()
                    .size(14.0),
            )
            .fill(ACCENT)
            .rounding(ThemeRounding::BUTTON)
            .min_size(egui::vec2(160.0, 32.0));
            if ui.add(start_btn).clicked() {
                app.action_start_migration();
                app.migration_state_mut().step = MigrationStep::InProgress;
            }
            if ui.button(t!("migration.cancel")).clicked() {
                close_dialog(app);
            }
        });
    });
}

// ===================== Step 4a: Progress =====================

fn render_step_progress(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let snapshot = app.migration_state().progress_snapshot.clone();

    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(&snapshot.phase)
            .color(AppColors::TEXT)
            .strong()
            .size(14.0),
    );
    ui.add_space(theme::Spacing::SM);

    let pct = if snapshot.percent >= 0 {
        (snapshot.percent as f32 / 100.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ui.add(
        egui::ProgressBar::new(pct)
            .show_percentage()
            .desired_width(ui.available_width()),
    );
    ui.add_space(10.0);

    egui::Grid::new("migration_stats")
        .num_columns(2)
        .spacing([24.0, 4.0])
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(t!("migration.stat-transferred")).color(AppColors::TEXT_DIM),
            );
            ui.label(format_bytes_display(snapshot.data_transferred));
            ui.end_row();

            if snapshot.data_remaining > 0 {
                ui.label(
                    egui::RichText::new(t!("migration.stat-remaining")).color(AppColors::TEXT_DIM),
                );
                ui.label(format_bytes_display(snapshot.data_remaining));
                ui.end_row();
            }

            if snapshot.memory_bps > 0 {
                ui.label(egui::RichText::new(t!("migration.stat-rate")).color(AppColors::TEXT_DIM));
                ui.label(format!("{}/s", format_bytes_display(snapshot.memory_bps)));
                ui.end_row();
            }

            ui.label(egui::RichText::new(t!("migration.stat-elapsed")).color(AppColors::TEXT_DIM));
            ui.label(format!("{} s", snapshot.elapsed_secs));
            ui.end_row();
        });

    ui.add_space(theme::Spacing::MD);
    ui.separator();
    ui.add_space(6.0);

    let vm_name = app.migration_state().vm_name.clone();
    ui.horizontal(|ui| {
        if ui
            .button(egui::RichText::new(t!("migration.cancel-migration")).color(AppColors::DANGER))
            .clicked()
        {
            // Core does support cancellation via virsh domjobabort.
            if let Err(e) = vmm_core::migration::cancel_migration(&vm_name) {
                app.migration_state_mut().error = Some(e.to_string());
            } else {
                let mut p = MigrationProgress::default();
                p.cancelled = true;
                p.completed = true;
                p.phase = t!("migration.cancelled").to_string();
                app.migration_state_mut().progress_snapshot = p;
                app.migration_state_mut().step = MigrationStep::Done;
            }
        }
        if let Some(ref err) = app.migration_state().error {
            ui.label(egui::RichText::new(err).color(AppColors::DANGER));
        }
    });
}

// ===================== Step 4b: Done =====================

fn render_step_done(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let snapshot = app.migration_state().progress_snapshot.clone();
    let host_idx = app.migration_state().selected_host_idx;
    let hosts = app
        .migration_state()
        .remote_hosts
        .clone()
        .unwrap_or_default();
    let host = host_idx.and_then(|i| hosts.hosts.get(i).cloned());

    ui.add_space(10.0);

    if let Some(ref err) = snapshot.error {
        egui::Frame::none()
            .fill(AppColors::DANGER.linear_multiply(0.15))
            .rounding(ThemeRounding::BUTTON)
            .stroke(egui::Stroke::new(1.0, AppColors::DANGER))
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!("\u{2717} {}", t!("migration.failed")))
                        .color(AppColors::DANGER)
                        .strong()
                        .size(theme::FontSize::HEADING),
                );
                ui.add_space(theme::Spacing::XS);
                ui.label(
                    egui::RichText::new(err)
                        .color(AppColors::TEXT)
                        .family(egui::FontFamily::Monospace)
                        .size(12.0),
                );
            });
    } else if snapshot.cancelled {
        ui.label(
            egui::RichText::new(format!("\u{2298} {}", t!("migration.cancelled")))
                .color(AppColors::WARNING)
                .strong()
                .size(theme::FontSize::HEADING),
        );
    } else {
        let host_name = host
            .as_ref()
            .map(|h| h.name.clone())
            .unwrap_or_else(|| t!("migration.destination-fallback").to_string());
        egui::Frame::none()
            .fill(AppColors::SUCCESS.linear_multiply(0.15))
            .rounding(ThemeRounding::BUTTON)
            .stroke(egui::Stroke::new(1.0, AppColors::SUCCESS))
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!("\u{2713} {}", t!("migration.success")))
                        .color(AppColors::SUCCESS)
                        .strong()
                        .size(theme::FontSize::HEADING),
                );
                ui.add_space(theme::Spacing::XS);
                ui.label(
                    egui::RichText::new(t!(
                        "migration.success-detail",
                        host = host_name,
                        elapsed = snapshot.elapsed_secs.to_string()
                    ))
                    .color(AppColors::TEXT)
                    .size(12.0),
                );
            });
    }

    ui.add_space(theme::Spacing::MD);
    ui.separator();
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        // Offer "Switch to <host>" on success
        let success = snapshot.error.is_none() && !snapshot.cancelled;
        if success {
            if let Some(ref h) = host {
                let label = t!("migration.switch-to", host = h.name.clone());
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(label).color(egui::Color32::WHITE))
                            .fill(ACCENT)
                            .rounding(ThemeRounding::BUTTON),
                    )
                    .clicked()
                {
                    let uri = h.connection_uri();
                    let name = h.name.clone();
                    close_dialog(app);
                    app.action_connect_remote(&uri, &name);
                    return;
                }
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t!("migration.close")).clicked() {
                let needs_refresh = success;
                close_dialog(app);
                if needs_refresh {
                    app.action_refresh();
                }
            }
        });
    });
}

// ===================== Nav helpers =====================

struct NextAction {
    label: String,
    enabled: bool,
    target: MigrationStep,
}

struct NavConfig {
    back: Option<MigrationStep>,
    next: Option<NextAction>,
    cancel: bool,
}

fn nav_buttons(app: &mut LibreVmmApp, ui: &mut egui::Ui, cfg: NavConfig) {
    ui.horizontal(|ui| {
        if let Some(back) = cfg.back {
            if ui
                .button(format!("\u{25C0}  {}", t!("migration.back")))
                .clicked()
            {
                app.migration_state_mut().step = back;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(ref next) = cfg.next {
                let btn = egui::Button::new(
                    egui::RichText::new(format!("{}  \u{25B6}", next.label)).color(
                        if next.enabled {
                            egui::Color32::WHITE
                        } else {
                            AppColors::MUTED
                        },
                    ),
                )
                .fill(if next.enabled {
                    ACCENT
                } else {
                    AppColors::BG_CARD
                })
                .rounding(ThemeRounding::BUTTON);
                if ui.add_enabled(next.enabled, btn).clicked() {
                    app.migration_state_mut().step = next.target;
                }
            }
            if cfg.cancel && ui.button(t!("migration.cancel")).clicked() {
                close_dialog(app);
            }
        });
    });
}

/// Close the migration dialog and, if the worker thread was still running,
/// surface a visible warning to the user (the audit flagged that we used to
/// only log a `tracing::warn!` here, which was invisible to the user). The
/// in-flight migration is cancelled inside `MigrationState::close()` itself.
fn close_dialog(app: &mut LibreVmmApp) {
    let was_running = app.migration_state_mut().close();
    if was_running {
        app.report_validation_error(t!("migration.close-while-running").to_string());
    }
}

/// Format bytes for display.
fn format_bytes_display(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
