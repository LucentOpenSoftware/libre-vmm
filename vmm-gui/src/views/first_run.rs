//! First-Run Setup Wizard
//!
//! A multi-step welcome experience for new users. Detects whether the host has
//! KVM / libvirt / OVMF / swtpm installed, offers copy-paste install commands
//! when something is missing, then scans the home directory and `/etc/libvirt`
//! for existing VMs and offers to import them.
//!
//! Triggered automatically on first launch (when `Preferences.first_run_completed`
//! is false AND the user has zero VMs registered). Setting the completion flag
//! is what guarantees the wizard does not reopen on every subsequent launch.

use eframe::egui;
use rust_i18n::t;
use std::collections::HashSet;

use crate::app::LibreVmmApp;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use vmm_core::system_check::{distro_family, DistroFamily, SystemCheck};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// All first-run wizard state. Persisted in [`LibreVmmApp`] while the wizard is open.
#[derive(Debug, Clone, Default)]
pub struct FirstRunState {
    pub open: bool,
    pub step: FirstRunStep,
    pub system_check: SystemCheck,
    /// Whether the system check has been populated at least once.
    pub system_check_done: bool,
    pub discovered_vms: Vec<vmm_core::import::ImportedVm>,
    pub discovery_in_progress: bool,
    pub discovery_done: bool,
    pub selected_for_import: HashSet<usize>,
    pub import_results: Vec<(String, Result<(), String>)>,
    /// Index of VM currently being imported (drives the progress bar).
    #[allow(dead_code)]
    pub import_progress: usize,
}

/// Step in the first-run wizard flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstRunStep {
    /// Welcome screen with brief intro and Continue / Skip buttons.
    Welcome,
    /// Detect KVM, libvirt, OVMF, swtpm, qemu binary, group membership.
    SystemCheck,
    /// Shown only when SystemCheck found missing pieces — copy-paste install commands.
    HelpInstall,
    /// Scan home dirs and `/etc/libvirt` for existing VMs.
    Discover,
    /// Checkbox list of discovered VMs.
    SelectImports,
    /// Progress bar while importing.
    Importing,
    /// Summary + Open VM Library button.
    Done,
}

impl Default for FirstRunStep {
    fn default() -> Self {
        FirstRunStep::Welcome
    }
}

impl FirstRunState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Render the first-run wizard if it's open. No-op otherwise.
pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let open = app.first_run_state().open;
    if !open {
        return;
    }

    let mut window_open = true;
    egui::Window::new(t!("first-run.title").to_string())
        .open(&mut window_open)
        .resizable(true)
        .default_width(640.0)
        .min_width(520.0)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            render_step_header(app, ui);
            ui.separator();
            ui.add_space(Spacing::MD);
            render_step_body(app, ui);
        });

    if !window_open {
        app.action_dismiss_first_run();
    }
}

// ---------------------------------------------------------------------------
// Step indicator
// ---------------------------------------------------------------------------

fn render_step_header(app: &LibreVmmApp, ui: &mut egui::Ui) {
    let step = app.first_run_state().step;
    let labels = [
        (
            FirstRunStep::Welcome,
            t!("first-run.step.welcome").to_string(),
        ),
        (
            FirstRunStep::SystemCheck,
            t!("first-run.step.check").to_string(),
        ),
        (
            FirstRunStep::Discover,
            t!("first-run.step.discover").to_string(),
        ),
        (FirstRunStep::Done, t!("first-run.step.done").to_string()),
    ];

    ui.horizontal(|ui| {
        for (i, (s, label)) in labels.iter().enumerate() {
            let active = matches_step_group(step, *s);
            let color = if active {
                AppColors::PRIMARY
            } else {
                AppColors::TEXT_DIM
            };
            ui.label(
                egui::RichText::new(label.as_str())
                    .color(color)
                    .strong()
                    .size(FontSize::SUBHEADING),
            );
            if i < labels.len() - 1 {
                ui.label(egui::RichText::new("\u{25B8}").color(AppColors::TEXT_DIM));
            }
        }
    });
}

fn matches_step_group(current: FirstRunStep, group: FirstRunStep) -> bool {
    match group {
        FirstRunStep::Welcome => current == FirstRunStep::Welcome,
        FirstRunStep::SystemCheck => matches!(
            current,
            FirstRunStep::SystemCheck | FirstRunStep::HelpInstall
        ),
        FirstRunStep::Discover => matches!(
            current,
            FirstRunStep::Discover | FirstRunStep::SelectImports | FirstRunStep::Importing
        ),
        FirstRunStep::Done => current == FirstRunStep::Done,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Body dispatch
// ---------------------------------------------------------------------------

fn render_step_body(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let step = app.first_run_state().step;
    match step {
        FirstRunStep::Welcome => render_welcome(app, ui),
        FirstRunStep::SystemCheck => render_system_check(app, ui),
        FirstRunStep::HelpInstall => render_help_install(app, ui),
        FirstRunStep::Discover => render_discover(app, ui),
        FirstRunStep::SelectImports => render_select_imports(app, ui),
        FirstRunStep::Importing => render_importing(app, ui),
        FirstRunStep::Done => render_done(app, ui),
    }
}

// ---------------------------------------------------------------------------
// Welcome
// ---------------------------------------------------------------------------

fn render_welcome(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.add_space(Spacing::MD);
    ui.label(
        egui::RichText::new(t!("first-run.welcome.heading").to_string())
            .color(AppColors::TEXT)
            .size(FontSize::PAGE_TITLE)
            .strong(),
    );
    ui.add_space(Spacing::SM);
    ui.label(
        egui::RichText::new(t!("first-run.welcome.body").to_string())
            .color(AppColors::TEXT)
            .size(FontSize::BODY),
    );
    ui.add_space(Spacing::LG);

    let mut go_continue = false;
    let mut go_skip = false;
    ui.horizontal(|ui| {
        if ui.button(t!("first-run.btn.skip").to_string()).clicked() {
            go_skip = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let btn = egui::Button::new(
                egui::RichText::new(t!("first-run.btn.continue").to_string())
                    .color(egui::Color32::WHITE)
                    .size(FontSize::SUBHEADING),
            )
            .fill(AppColors::PRIMARY)
            .min_size(egui::vec2(140.0, 32.0))
            .rounding(ThemeRounding::BUTTON);
            if ui.add(btn).clicked() {
                go_continue = true;
            }
        });
    });

    if go_continue {
        app.action_first_run_run_check();
    }
    if go_skip {
        app.action_dismiss_first_run();
    }
}

// ---------------------------------------------------------------------------
// System Check
// ---------------------------------------------------------------------------

fn render_system_check(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    // Always re-run on first entry — cheap, safe, idempotent.
    if !app.first_run_state().system_check_done {
        app.action_first_run_run_check();
    }

    let sc = app.first_run_state().system_check.clone();

    ui.add_space(Spacing::SM);
    ui.label(
        egui::RichText::new(t!("first-run.check.heading").to_string())
            .color(AppColors::TEXT)
            .size(FontSize::PAGE_TITLE)
            .strong(),
    );
    ui.add_space(Spacing::SM);
    ui.label(
        egui::RichText::new(t!("first-run.check.body").to_string())
            .color(AppColors::TEXT_DIM)
            .size(FontSize::BODY),
    );
    ui.add_space(Spacing::MD);

    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::CARD)
        .inner_margin(Spacing::MD)
        .show(ui, |ui| {
            check_row(
                ui,
                &t!("first-run.check.kvm-module").to_string(),
                sc.kvm_module_loaded,
            );
            check_row(
                ui,
                &t!("first-run.check.kvm-dev").to_string(),
                sc.kvm_dev_present,
            );
            check_row(
                ui,
                &t!("first-run.check.libvirtd").to_string(),
                sc.libvirtd_running,
            );
            check_row(
                ui,
                &t!("first-run.check.libvirt-group").to_string(),
                sc.user_in_libvirt_group,
            );
            check_row(
                ui,
                &t!("first-run.check.kvm-group").to_string(),
                sc.user_in_kvm_group,
            );
            check_row_some(
                ui,
                &t!("first-run.check.qemu").to_string(),
                sc.qemu_binary_found.as_deref(),
            );
            check_row_some(
                ui,
                &t!("first-run.check.ovmf").to_string(),
                sc.ovmf_present.as_deref(),
            );
            check_row_some(
                ui,
                &t!("first-run.check.swtpm").to_string(),
                sc.swtpm_binary_found.as_deref(),
            );
        });

    ui.add_space(Spacing::LG);

    let ok = sc.all_essentials_ok() && sc.swtpm_binary_found.is_some() && sc.ovmf_present.is_some();

    let mut go_back = false;
    let mut go_help = false;
    let mut go_discover = false;
    let mut do_recheck = false;
    ui.horizontal(|ui| {
        if ui.button(t!("wizard.back").to_string()).clicked() {
            go_back = true;
        }
        if ui.button(t!("first-run.btn.recheck").to_string()).clicked() {
            do_recheck = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ok {
                let btn = egui::Button::new(
                    egui::RichText::new(t!("first-run.btn.continue-discover").to_string())
                        .color(egui::Color32::WHITE),
                )
                .fill(AppColors::PRIMARY)
                .min_size(egui::vec2(180.0, 30.0));
                if ui.add(btn).clicked() {
                    go_discover = true;
                }
            } else {
                let btn = egui::Button::new(
                    egui::RichText::new(t!("first-run.btn.help-install").to_string())
                        .color(egui::Color32::WHITE),
                )
                .fill(AppColors::WARNING)
                .min_size(egui::vec2(180.0, 30.0));
                if ui.add(btn).clicked() {
                    go_help = true;
                }
            }
        });
    });

    if go_back {
        app.first_run_state_mut().step = FirstRunStep::Welcome;
    }
    if do_recheck {
        app.action_first_run_run_check();
    }
    if go_help {
        app.first_run_state_mut().step = FirstRunStep::HelpInstall;
    }
    if go_discover {
        app.action_first_run_start_discovery();
    }
}

fn check_row(ui: &mut egui::Ui, label: &str, ok: bool) {
    ui.horizontal(|ui| {
        let (icon, color) = if ok {
            ("\u{2713}", AppColors::SUCCESS)
        } else {
            ("\u{2717}", AppColors::DANGER)
        };
        ui.label(
            egui::RichText::new(icon)
                .color(color)
                .size(FontSize::SUBHEADING)
                .strong(),
        );
        ui.label(
            egui::RichText::new(label)
                .color(AppColors::TEXT)
                .size(FontSize::BODY),
        );
    });
}

fn check_row_some(ui: &mut egui::Ui, label: &str, value: Option<&str>) {
    ui.horizontal(|ui| {
        let (icon, color) = if value.is_some() {
            ("\u{2713}", AppColors::SUCCESS)
        } else {
            ("\u{2717}", AppColors::DANGER)
        };
        ui.label(
            egui::RichText::new(icon)
                .color(color)
                .size(FontSize::SUBHEADING)
                .strong(),
        );
        ui.label(
            egui::RichText::new(label)
                .color(AppColors::TEXT)
                .size(FontSize::BODY),
        );
        if let Some(path) = value {
            ui.label(
                egui::RichText::new(path)
                    .color(AppColors::TEXT_DIM)
                    .size(FontSize::SMALL),
            );
        }
    });
}

// ---------------------------------------------------------------------------
// Help Install (copy-paste commands)
// ---------------------------------------------------------------------------

fn render_help_install(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let sc = app.first_run_state().system_check.clone();
    let family = sc
        .distro_id
        .as_deref()
        .map(distro_family)
        .unwrap_or(DistroFamily::Unknown);

    ui.add_space(Spacing::SM);
    ui.label(
        egui::RichText::new(t!("first-run.install.heading").to_string())
            .color(AppColors::TEXT)
            .size(FontSize::PAGE_TITLE)
            .strong(),
    );
    ui.add_space(Spacing::SM);
    ui.label(
        egui::RichText::new(
            t!("first-run.install.detected-distro", family = family.label()).to_string(),
        )
        .color(AppColors::TEXT_DIM)
        .size(FontSize::BODY),
    );
    ui.add_space(Spacing::MD);

    egui::ScrollArea::vertical()
        .max_height(360.0)
        .show(ui, |ui| {
            // 1. Packages
            command_block(
                ui,
                &t!("first-run.install.packages").to_string(),
                family.install_command(),
            );
            // 2. Groups
            command_block(
                ui,
                &t!("first-run.install.groups").to_string(),
                family.group_command(),
            );
            // 3. Service
            command_block(
                ui,
                &t!("first-run.install.service").to_string(),
                family.enable_libvirtd_command(),
            );
            // 4. Alternate distros for reference
            ui.add_space(Spacing::SM);
            ui.collapsing(t!("first-run.install.other-distros").to_string(), |ui| {
                for fam in [
                    DistroFamily::Debian,
                    DistroFamily::Fedora,
                    DistroFamily::Arch,
                    DistroFamily::Suse,
                ] {
                    if fam == family {
                        continue;
                    }
                    command_block(ui, fam.label(), fam.install_command());
                }
            });
        });

    ui.add_space(Spacing::LG);
    let mut go_back = false;
    let mut do_recheck = false;
    let mut do_later = false;
    ui.horizontal(|ui| {
        if ui.button(t!("wizard.back").to_string()).clicked() {
            go_back = true;
        }
        if ui.button(t!("first-run.btn.recheck").to_string()).clicked() {
            do_recheck = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let btn = egui::Button::new(
                egui::RichText::new(t!("first-run.btn.do-later").to_string())
                    .color(AppColors::TEXT),
            )
            .fill(AppColors::BG_HOVER)
            .min_size(egui::vec2(140.0, 30.0));
            if ui.add(btn).clicked() {
                do_later = true;
            }
        });
    });

    if go_back {
        app.first_run_state_mut().step = FirstRunStep::SystemCheck;
    }
    if do_recheck {
        app.action_first_run_run_check();
    }
    if do_later {
        app.action_first_run_start_discovery();
    }
}

fn command_block(ui: &mut egui::Ui, title: &str, cmd: &str) {
    ui.add_space(Spacing::SM);
    ui.label(
        egui::RichText::new(title)
            .color(AppColors::TEXT)
            .size(FontSize::SUBHEADING)
            .strong(),
    );
    egui::Frame::none()
        .fill(AppColors::CONSOLE_BG)
        .rounding(ThemeRounding::FRAME)
        .inner_margin(Spacing::MD)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(cmd)
                        .color(AppColors::TEXT)
                        .monospace()
                        .size(FontSize::BODY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t!("first-run.btn.copy").to_string()).clicked() {
                        ui.output_mut(|o| o.copied_text = cmd.to_string());
                    }
                });
            });
        });
}

// ---------------------------------------------------------------------------
// Discover (spinner while scanning)
// ---------------------------------------------------------------------------

fn render_discover(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let done = app.first_run_state().discovery_done;
    if !done {
        // Run scan synchronously — it's I/O-bound but typically completes within
        // a few hundred ms even on large home dirs. A spinner is shown for one
        // frame, then we advance.
        app.action_first_run_run_discovery();
    }

    ui.add_space(Spacing::LG);
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(
            egui::RichText::new(t!("first-run.discover.scanning").to_string())
                .color(AppColors::PRIMARY)
                .size(FontSize::SUBHEADING),
        );
    });

    // Auto-advance once the scan finished.
    if app.first_run_state().discovery_done {
        app.first_run_state_mut().step = FirstRunStep::SelectImports;
    }
}

// ---------------------------------------------------------------------------
// Select imports
// ---------------------------------------------------------------------------

fn render_select_imports(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let vms = app.first_run_state().discovered_vms.clone();

    ui.add_space(Spacing::SM);
    ui.label(
        egui::RichText::new(t!("first-run.select.heading").to_string())
            .color(AppColors::TEXT)
            .size(FontSize::PAGE_TITLE)
            .strong(),
    );
    ui.add_space(Spacing::SM);

    if vms.is_empty() {
        ui.label(
            egui::RichText::new(t!("first-run.select.none-found").to_string())
                .color(AppColors::TEXT_DIM)
                .size(FontSize::BODY),
        );
        ui.add_space(Spacing::LG);
        if ui.button(t!("first-run.btn.finish").to_string()).clicked() {
            app.action_first_run_finish();
        }
        return;
    }

    ui.label(
        egui::RichText::new(
            t!("first-run.select.found-count", n = vms.len().to_string()).to_string(),
        )
        .color(AppColors::TEXT_DIM)
        .size(FontSize::BODY),
    );
    ui.add_space(Spacing::MD);

    // VM checkbox list (scrollable).
    egui::ScrollArea::vertical()
        .max_height(280.0)
        .show(ui, |ui| {
            for (i, vm) in vms.iter().enumerate() {
                let mut checked = app.first_run_state().selected_for_import.contains(&i);
                egui::Frame::none()
                    .fill(AppColors::BG_CARD)
                    .rounding(ThemeRounding::CARD)
                    .inner_margin(Spacing::MD)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut checked, "").changed() {
                                let set = &mut app.first_run_state_mut().selected_for_import;
                                if checked {
                                    set.insert(i);
                                } else {
                                    set.remove(&i);
                                }
                            }
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new(&vm.name)
                                        .color(AppColors::TEXT)
                                        .size(FontSize::SUBHEADING)
                                        .strong(),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} \u{2022} {} vCPU \u{2022} {} MiB",
                                        vm.source, vm.vcpus, vm.memory_mib,
                                    ))
                                    .color(AppColors::TEXT_DIM)
                                    .size(FontSize::SMALL),
                                );
                                if let Some(first_disk) = vm.disk_paths.first() {
                                    ui.label(
                                        egui::RichText::new(first_disk.display().to_string())
                                            .color(AppColors::TEXT_DIM)
                                            .size(FontSize::SMALL),
                                    );
                                }
                            });
                        });
                    });
                ui.add_space(Spacing::XS);
            }
        });

    ui.add_space(Spacing::LG);
    let mut go_back = false;
    let mut go_skip = false;
    let mut do_select_all = false;
    let mut do_import = false;
    ui.horizontal(|ui| {
        if ui.button(t!("wizard.back").to_string()).clicked() {
            go_back = true;
        }
        if ui
            .button(t!("first-run.btn.select-all").to_string())
            .clicked()
        {
            do_select_all = true;
        }
        if ui
            .button(t!("first-run.btn.skip-all").to_string())
            .clicked()
        {
            go_skip = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let any_selected = !app.first_run_state().selected_for_import.is_empty();
            let btn = egui::Button::new(
                egui::RichText::new(t!("first-run.btn.import-selected").to_string())
                    .color(egui::Color32::WHITE),
            )
            .fill(if any_selected {
                AppColors::SUCCESS
            } else {
                AppColors::MUTED
            })
            .min_size(egui::vec2(180.0, 30.0));
            if ui.add_enabled(any_selected, btn).clicked() {
                do_import = true;
            }
        });
    });

    if go_back {
        app.first_run_state_mut().step = FirstRunStep::SystemCheck;
    }
    if do_select_all {
        let total = vms.len();
        let set = &mut app.first_run_state_mut().selected_for_import;
        for i in 0..total {
            set.insert(i);
        }
    }
    if go_skip {
        app.action_first_run_finish();
    }
    if do_import {
        app.first_run_state_mut().step = FirstRunStep::Importing;
        app.action_first_run_run_imports();
    }
}

// ---------------------------------------------------------------------------
// Importing (progress)
// ---------------------------------------------------------------------------

fn render_importing(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let state = app.first_run_state();
    let total = state.selected_for_import.len();
    let done_count = state.import_results.len();

    ui.add_space(Spacing::LG);
    ui.label(
        egui::RichText::new(
            t!(
                "first-run.importing.heading",
                done = done_count.to_string(),
                total = total.to_string()
            )
            .to_string(),
        )
        .color(AppColors::TEXT)
        .size(FontSize::SUBHEADING),
    );
    ui.add_space(Spacing::SM);

    let fraction = if total == 0 {
        1.0
    } else {
        done_count as f32 / total as f32
    };
    ui.add(
        egui::ProgressBar::new(fraction)
            .desired_width(420.0)
            .show_percentage(),
    );

    if done_count >= total {
        app.first_run_state_mut().step = FirstRunStep::Done;
    }
}

// ---------------------------------------------------------------------------
// Done (summary)
// ---------------------------------------------------------------------------

fn render_done(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let results = app.first_run_state().import_results.clone();
    let succeeded = results.iter().filter(|(_, r)| r.is_ok()).count();
    let failed = results.len().saturating_sub(succeeded);

    ui.add_space(Spacing::MD);
    ui.label(
        egui::RichText::new(t!("first-run.done.heading").to_string())
            .color(AppColors::SUCCESS)
            .size(FontSize::PAGE_TITLE)
            .strong(),
    );
    ui.add_space(Spacing::SM);
    ui.label(
        egui::RichText::new(
            t!(
                "first-run.done.summary",
                ok = succeeded.to_string(),
                failed = failed.to_string()
            )
            .to_string(),
        )
        .color(AppColors::TEXT)
        .size(FontSize::BODY),
    );
    ui.add_space(Spacing::MD);

    if !results.is_empty() {
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                for (name, result) in &results {
                    let (icon, color, msg) = match result {
                        Ok(()) => (
                            "\u{2713}",
                            AppColors::SUCCESS,
                            t!("first-run.done.ok").to_string(),
                        ),
                        Err(e) => ("\u{2717}", AppColors::DANGER, e.clone()),
                    };
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(icon)
                                .color(color)
                                .strong()
                                .size(FontSize::SUBHEADING),
                        );
                        ui.label(
                            egui::RichText::new(name)
                                .color(AppColors::TEXT)
                                .size(FontSize::BODY),
                        );
                        ui.label(
                            egui::RichText::new(msg)
                                .color(AppColors::TEXT_DIM)
                                .size(FontSize::SMALL),
                        );
                    });
                }
            });
    }

    ui.add_space(Spacing::LG);
    if ui
        .button(
            egui::RichText::new(t!("first-run.btn.open-library").to_string())
                .size(FontSize::SUBHEADING),
        )
        .clicked()
    {
        app.action_first_run_finish();
    }
}

// ---------------------------------------------------------------------------
// Tests — light coverage for the pure helpers in this module.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_first_run_step_is_welcome() {
        let s: FirstRunState = FirstRunState::default();
        assert_eq!(s.step, FirstRunStep::Welcome);
        assert!(!s.open);
        assert!(s.discovered_vms.is_empty());
        assert!(s.selected_for_import.is_empty());
    }

    #[test]
    fn step_grouping() {
        assert!(matches_step_group(
            FirstRunStep::HelpInstall,
            FirstRunStep::SystemCheck
        ));
        assert!(matches_step_group(
            FirstRunStep::SelectImports,
            FirstRunStep::Discover
        ));
        assert!(matches_step_group(
            FirstRunStep::Importing,
            FirstRunStep::Discover
        ));
        assert!(!matches_step_group(
            FirstRunStep::Welcome,
            FirstRunStep::Done
        ));
    }

    #[test]
    fn reset_returns_to_welcome() {
        let mut s = FirstRunState {
            open: true,
            step: FirstRunStep::Done,
            ..Default::default()
        };
        s.reset();
        assert!(!s.open);
        assert_eq!(s.step, FirstRunStep::Welcome);
    }

    #[test]
    fn run_system_check_smoke() {
        // Sanity check — the integration touches the real system_check API.
        let _ = vmm_core::system_check::run_system_check();
    }
}
