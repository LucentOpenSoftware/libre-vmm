//! Wave 12.2: Single-GPU passthrough setup wizard.
//!
//! When the host has only one GPU and the user wants to pass it through to a
//! VM (typically Windows for gaming), the host's display manager and console
//! need to release the GPU before the VM starts and reclaim it afterward.
//!
//! This is the one VFIO feature that *neither* VMware Workstation nor
//! VirtualBox can implement: it requires kernel-level cooperation
//! (vtconsole/framebuffer unbind, modprobe vfio-pci, display-manager
//! restart). On Linux+KVM it is straightforward — the wizard generates the
//! libvirt hook scripts, shows them to the user for review, and explains how
//! to install a small sudoers drop-in so the scripts can run as root.
//!
//! The wizard never writes to /etc/* or invokes sudo. It only:
//!   * reads sysfs / `/etc/systemd/system/display-manager.service` symlink,
//!   * writes the two hook scripts to `~/.local/share/libre-vmm/vfio-hooks/<vm>/`,
//!   * shows a sudoers snippet the user must install manually with `visudo`.

use eframe::egui;
use rust_i18n::t;
use vmm_core::pci::PciDevice;
use vmm_core::vfio;

use crate::app::LibreVmmApp;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};

// ─── State ──────────────────────────────────────────────────────────

/// Which step of the wizard we're on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleGpuStep {
    /// Detect host GPU(s), display manager, and warn the user about the risks.
    Detect,
    /// Show the generated before-start.sh / after-stop.sh hook scripts.
    Preview,
    /// Show the sudoers drop-in for manual installation.
    Permissions,
    /// Summary + Done.
    Confirm,
}

impl Default for SingleGpuStep {
    fn default() -> Self {
        Self::Detect
    }
}

/// Wizard state. Lives on `LibreVmmApp`.
pub struct SingleGpuWizardState {
    pub open: bool,
    pub step: SingleGpuStep,
    /// The single detected GPU (if any). When `None`, the user is told to use
    /// the multi-GPU passthrough flow instead.
    pub detected_gpu: Option<PciDevice>,
    /// Total host GPU count — used to decide whether to redirect to multi-GPU.
    pub gpu_count: usize,
    /// Detected display manager (gdm/sddm/lightdm/ly/...).
    pub detected_dm: Option<String>,
    /// Active TTY string (e.g. "tty2").
    pub detected_tty: Option<String>,
    /// VM name the hooks will be generated for. Pre-filled from the
    /// currently-edited VM; the user can adjust before saving.
    pub vm_name: String,
    /// Rendered before-start.sh contents (editable).
    pub before_script: String,
    /// Rendered after-stop.sh contents (editable).
    pub after_script: String,
    /// Sudoers drop-in suggestion (read-only — the user copies and installs).
    pub sudoers_snippet: String,
    /// Where the saved scripts live, once saved.
    pub saved_dir: Option<String>,
    /// Last error message (shown on the current step).
    pub error: Option<String>,
    /// "Scripts saved" success flag for the Confirm step.
    pub scripts_saved: bool,
}

impl Default for SingleGpuWizardState {
    fn default() -> Self {
        Self {
            open: false,
            step: SingleGpuStep::Detect,
            detected_gpu: None,
            gpu_count: 0,
            detected_dm: None,
            detected_tty: None,
            vm_name: String::new(),
            before_script: String::new(),
            after_script: String::new(),
            sudoers_snippet: String::new(),
            saved_dir: None,
            error: None,
            scripts_saved: false,
        }
    }
}

impl SingleGpuWizardState {
    /// Run host detection (cheap — sysfs reads only). Called when the wizard
    /// is opened, and again if the user clicks "Re-detect".
    pub fn detect(&mut self) {
        let gpus = vmm_core::pci::find_gpus();
        self.gpu_count = gpus.len();
        self.detected_gpu = match gpus.len() {
            1 => gpus.into_iter().next(),
            _ => None,
        };
        self.detected_dm = vfio::detect_display_manager();
        self.detected_tty = vfio::detect_active_tty();
    }

    /// Generate the script previews from the current state.
    pub fn regenerate_scripts(&mut self) {
        let bus = self
            .detected_gpu
            .as_ref()
            .map(|g| g.address.clone())
            .unwrap_or_else(|| "0000:01:00.0".to_string());
        let dm = self.detected_dm.clone().unwrap_or_default();
        self.before_script = vfio::render_before_start_script(&self.vm_name, &bus, &dm);
        self.after_script = vfio::render_after_stop_script(&self.vm_name, &bus, &dm);

        let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
        let dir = vfio::hook_dir_for_vm(&self.vm_name);
        // Use the parent (hook root) for the sudoers wildcard so future VMs
        // share the same drop-in.
        let hook_root = dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| dir.clone());
        self.sudoers_snippet = vfio::render_sudoers_snippet(&user, &hook_root);
    }
}

// ─── App-level helpers ──────────────────────────────────────────────

impl LibreVmmApp {
    /// Open the single-GPU passthrough wizard, seeded from the currently
    /// edited VM (or selected VM).
    pub fn open_single_gpu_wizard(&mut self) {
        // Seed VM name from whichever context is most relevant.
        let seed_name = self
            .editing_config()
            .map(|c| c.name.clone())
            .or_else(|| self.selected_vm().map(|s| s.to_string()))
            .unwrap_or_else(|| "gaming-vm".to_string());

        let state = self.single_gpu_state_mut();
        state.open = true;
        state.step = SingleGpuStep::Detect;
        state.error = None;
        state.saved_dir = None;
        state.scripts_saved = false;
        // Sanitize the seed name to match the hook-script allowlist.
        state.vm_name = seed_name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        state.detect();
        state.regenerate_scripts();
    }
}

// ─── Rendering ──────────────────────────────────────────────────────

/// Render the wizard as a floating window.
pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    if !app.single_gpu_state().open {
        return;
    }

    let mut open = true;
    egui::Window::new(t!("single-gpu.window-title"))
        .open(&mut open)
        .resizable(true)
        .collapsible(true)
        .default_width(720.0)
        .default_height(600.0)
        .show(ctx, |ui| {
            render_step_indicator(app, ui);
            ui.add_space(Spacing::SM);
            ui.separator();
            ui.add_space(Spacing::SM);

            let step = app.single_gpu_state().step;
            match step {
                SingleGpuStep::Detect => render_step_detect(app, ui),
                SingleGpuStep::Preview => render_step_preview(app, ui),
                SingleGpuStep::Permissions => render_step_permissions(app, ui, ctx),
                SingleGpuStep::Confirm => render_step_confirm(app, ui),
            }
        });

    if !open {
        app.single_gpu_state_mut().open = false;
    }
}

fn render_step_indicator(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let cur = app.single_gpu_state().step;
    ui.horizontal(|ui| {
        let steps = [
            (SingleGpuStep::Detect, t!("single-gpu.step-detect")),
            (SingleGpuStep::Preview, t!("single-gpu.step-preview")),
            (
                SingleGpuStep::Permissions,
                t!("single-gpu.step-permissions"),
            ),
            (SingleGpuStep::Confirm, t!("single-gpu.step-confirm")),
        ];
        for (s, label) in steps {
            let active = s == cur;
            let color = if active {
                AppColors::PRIMARY
            } else {
                AppColors::TEXT_DIM
            };
            ui.label(
                egui::RichText::new(label)
                    .color(color)
                    .strong()
                    .size(FontSize::SUBHEADING),
            );
            ui.label(egui::RichText::new("  >  ").color(AppColors::TEXT_DIM));
        }
    });
}

// ─── Step 1: Detection ──────────────────────────────────────────────

fn render_step_detect(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    // Snapshot read-only state up front (some clones to satisfy the borrow checker
    // since we also call mut methods further down).
    let gpu_count = app.single_gpu_state().gpu_count;
    let detected_gpu = app.single_gpu_state().detected_gpu.clone();
    let detected_dm = app.single_gpu_state().detected_dm.clone();
    let detected_tty = app.single_gpu_state().detected_tty.clone();

    // Big warning panel
    egui::Frame::none()
        .fill(AppColors::BANNER_BG)
        .rounding(ThemeRounding::FRAME)
        .inner_margin(Spacing::MD)
        .stroke(egui::Stroke::new(
            0.5,
            AppColors::WARNING.linear_multiply(0.6),
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("\u{26A0}")
                        .size(FontSize::HEADING)
                        .color(AppColors::WARNING),
                );
                ui.label(
                    egui::RichText::new(t!("single-gpu.warning-title"))
                        .size(FontSize::SUBHEADING)
                        .color(AppColors::WARNING)
                        .strong(),
                );
            });
            ui.add_space(Spacing::XS);
            ui.label(
                egui::RichText::new(t!("single-gpu.warning-body"))
                    .size(FontSize::BODY)
                    .color(AppColors::TEXT),
            );
        });

    ui.add_space(Spacing::SM);

    // ── GPU detection ────────────────────────────────────────────────
    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::CARD)
        .inner_margin(10.0)
        .stroke(egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(t!("single-gpu.host-gpus"))
                    .size(FontSize::HEADING)
                    .color(AppColors::TEXT)
                    .strong(),
            );
            ui.add_space(Spacing::XS);

            match gpu_count {
                0 => {
                    ui.label(
                        egui::RichText::new(t!("single-gpu.no-gpus")).color(AppColors::DANGER),
                    );
                },
                1 => {
                    if let Some(g) = detected_gpu.as_ref() {
                        ui.label(
                            egui::RichText::new(t!(
                                "single-gpu.one-gpu",
                                vendor = &g.vendor_name,
                                device = &g.device_name
                            ))
                            .color(AppColors::TEXT),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "PCI: {}   IOMMU group: {}   driver: {}",
                                g.address,
                                g.iommu_group
                                    .map(|i| i.to_string())
                                    .unwrap_or_else(|| "?".into()),
                                g.driver.as_deref().unwrap_or("none"),
                            ))
                            .size(FontSize::SMALL)
                            .family(egui::FontFamily::Monospace)
                            .color(AppColors::TEXT_DIM),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new(t!("single-gpu.detect-inconsistent"))
                                .color(AppColors::WARNING),
                        );
                    }
                },
                _ => {
                    ui.label(
                        egui::RichText::new(t!("single-gpu.multi-gpu", n = gpu_count))
                            .strong()
                            .color(AppColors::SUCCESS),
                    );
                    ui.label(
                        egui::RichText::new(t!("single-gpu.multi-gpu-hint"))
                            .size(FontSize::SMALL)
                            .color(AppColors::TEXT_DIM),
                    );
                },
            }
        });

    ui.add_space(Spacing::SM);

    // ── Display manager / TTY ────────────────────────────────────────
    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::CARD)
        .inner_margin(10.0)
        .stroke(egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(t!("single-gpu.display-env"))
                    .size(FontSize::HEADING)
                    .color(AppColors::TEXT)
                    .strong(),
            );
            ui.add_space(Spacing::XS);

            egui::Grid::new("single_gpu_env_grid")
                .num_columns(2)
                .spacing([Spacing::MD, 4.0])
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(t!("single-gpu.display-manager"))
                            .color(AppColors::TEXT_DIM),
                    );
                    match detected_dm.as_deref() {
                        Some(dm) => ui.label(
                            egui::RichText::new(dm)
                                .color(AppColors::SUCCESS)
                                .family(egui::FontFamily::Monospace),
                        ),
                        None => ui.label(
                            egui::RichText::new(t!("single-gpu.dm-unknown"))
                                .color(AppColors::WARNING),
                        ),
                    };
                    ui.end_row();

                    ui.label(
                        egui::RichText::new(t!("single-gpu.active-tty")).color(AppColors::TEXT_DIM),
                    );
                    ui.label(
                        egui::RichText::new(detected_tty.as_deref().unwrap_or("unknown"))
                            .color(AppColors::TEXT)
                            .family(egui::FontFamily::Monospace),
                    );
                    ui.end_row();
                });
        });

    ui.add_space(Spacing::SM);

    // ── VM name ──────────────────────────────────────────────────────
    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::CARD)
        .inner_margin(10.0)
        .stroke(egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(t!("single-gpu.vm-name-heading"))
                    .size(FontSize::HEADING)
                    .color(AppColors::TEXT)
                    .strong(),
            );
            ui.add_space(Spacing::XS);

            let state = app.single_gpu_state_mut();
            ui.text_edit_singleline(&mut state.vm_name);
            ui.label(
                egui::RichText::new(t!("single-gpu.vm-name-hint"))
                    .size(FontSize::SMALL)
                    .color(AppColors::TEXT_DIM),
            );

            if !vfio::validate_vm_name_for_hook(&state.vm_name) {
                ui.label(
                    egui::RichText::new(t!("single-gpu.vm-name-invalid"))
                        .color(AppColors::DANGER)
                        .size(FontSize::SMALL),
                );
            }
        });

    ui.add_space(Spacing::MD);

    // ── Navigation ───────────────────────────────────────────────────
    ui.horizontal(|ui| {
        if ui.button(t!("single-gpu.cancel")).clicked() {
            app.single_gpu_state_mut().open = false;
        }
        if ui.button(t!("single-gpu.re-detect")).clicked() {
            let state = app.single_gpu_state_mut();
            state.detect();
            state.regenerate_scripts();
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_continue =
                gpu_count == 1 && vfio::validate_vm_name_for_hook(&app.single_gpu_state().vm_name);
            let btn = egui::Button::new(egui::RichText::new(t!("single-gpu.continue")).color(
                if can_continue {
                    egui::Color32::WHITE
                } else {
                    AppColors::MUTED
                },
            ))
            .fill(if can_continue {
                AppColors::PRIMARY
            } else {
                AppColors::BG_CARD
            });
            if ui.add_enabled(can_continue, btn).clicked() {
                let state = app.single_gpu_state_mut();
                state.regenerate_scripts();
                state.step = SingleGpuStep::Preview;
            }
        });
    });
}

// ─── Step 2: Script preview ─────────────────────────────────────────

fn render_step_preview(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new(t!("single-gpu.review-title"))
            .size(FontSize::SUBHEADING)
            .color(AppColors::TEXT)
            .strong(),
    );
    ui.label(
        egui::RichText::new(t!("single-gpu.review-desc"))
            .size(FontSize::SMALL)
            .color(AppColors::TEXT_DIM),
    );
    ui.add_space(Spacing::SM);

    egui::ScrollArea::vertical()
        .max_height(420.0)
        .show(ui, |ui| {
            render_script_block(app, ui, "before-start.sh", true);
            ui.add_space(Spacing::SM);
            render_script_block(app, ui, "after-stop.sh", false);
        });

    if let Some(err) = app.single_gpu_state().error.clone() {
        ui.add_space(Spacing::SM);
        ui.label(egui::RichText::new(err).color(AppColors::DANGER));
    }

    ui.add_space(Spacing::MD);
    ui.horizontal(|ui| {
        if ui.button(t!("single-gpu.back")).clicked() {
            app.single_gpu_state_mut().step = SingleGpuStep::Detect;
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let btn = egui::Button::new(
                egui::RichText::new(t!("single-gpu.next-permissions")).color(egui::Color32::WHITE),
            )
            .fill(AppColors::PRIMARY);
            if ui.add(btn).clicked() {
                app.single_gpu_state_mut().step = SingleGpuStep::Permissions;
            }
            if ui.button(t!("single-gpu.save-scripts")).clicked() {
                let state = app.single_gpu_state();
                match vfio::save_hook_scripts(
                    &state.vm_name,
                    &state.before_script,
                    &state.after_script,
                ) {
                    Ok(dir) => {
                        let s = app.single_gpu_state_mut();
                        s.saved_dir = Some(dir.display().to_string());
                        s.scripts_saved = true;
                        s.error = None;
                    },
                    Err(e) => {
                        app.single_gpu_state_mut().error = Some(e);
                    },
                }
            }
        });
    });
}

fn render_script_block(app: &mut LibreVmmApp, ui: &mut egui::Ui, label: &str, is_before: bool) {
    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::FRAME)
        .inner_margin(Spacing::SM)
        .stroke(egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(label)
                        .strong()
                        .color(AppColors::PRIMARY)
                        .family(egui::FontFamily::Monospace),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t!("single-gpu.edit-in-editor")).clicked() {
                        // Try to launch $EDITOR on a temp copy. If $EDITOR isn't set,
                        // do nothing — explicit per spec.
                        if let Ok(editor) = std::env::var("EDITOR") {
                            let state = app.single_gpu_state();
                            let content = if is_before {
                                state.before_script.clone()
                            } else {
                                state.after_script.clone()
                            };
                            let tmp = std::env::temp_dir()
                                .join(format!("libre-vmm-{}-{}.sh", state.vm_name, label));
                            if std::fs::write(&tmp, &content).is_ok() {
                                let _ = std::process::Command::new(&editor).arg(&tmp).spawn();
                            }
                        }
                    }
                });
            });
            ui.add_space(Spacing::XS);

            let state = app.single_gpu_state_mut();
            let script_buf = if is_before {
                &mut state.before_script
            } else {
                &mut state.after_script
            };
            ui.add(
                egui::TextEdit::multiline(script_buf)
                    .code_editor()
                    .desired_rows(12)
                    .desired_width(f32::INFINITY),
            );
        });
}

// ─── Step 3: Sudoers ────────────────────────────────────────────────

fn render_step_permissions(app: &mut LibreVmmApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.label(
        egui::RichText::new(t!("single-gpu.perm-title"))
            .size(FontSize::SUBHEADING)
            .color(AppColors::TEXT)
            .strong(),
    );
    ui.label(
        egui::RichText::new(t!("single-gpu.perm-desc"))
            .size(FontSize::SMALL)
            .color(AppColors::TEXT_DIM),
    );
    ui.add_space(Spacing::SM);

    let snippet = app.single_gpu_state().sudoers_snippet.clone();
    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::FRAME)
        .inner_margin(Spacing::SM)
        .stroke(egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(t!("single-gpu.sudoers-suggested"))
                        .strong()
                        .color(AppColors::PRIMARY)
                        .family(egui::FontFamily::Monospace),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t!("single-gpu.copy-clipboard")).clicked() {
                        ctx.copy_text(snippet.clone());
                    }
                });
            });
            ui.add_space(Spacing::XS);

            let mut display = snippet.clone();
            ui.add(
                egui::TextEdit::multiline(&mut display)
                    .code_editor()
                    .desired_rows(4)
                    .desired_width(f32::INFINITY)
                    .interactive(false),
            );
        });

    ui.add_space(Spacing::SM);

    // Step-by-step instructions
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::FRAME)
        .inner_margin(10.0)
        .stroke(egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(t!("single-gpu.howto-title"))
                    .strong()
                    .color(AppColors::TEXT),
            );
            ui.add_space(Spacing::XS);

            ui.label(egui::RichText::new(t!("single-gpu.howto-1")).color(AppColors::TEXT));
            ui.label(
                egui::RichText::new(t!("single-gpu.howto-2", user = user))
                    .color(AppColors::TEXT)
                    .family(egui::FontFamily::Monospace),
            );
            ui.label(egui::RichText::new(t!("single-gpu.howto-3")).color(AppColors::TEXT));
            ui.label(
                egui::RichText::new(t!("single-gpu.howto-note"))
                    .color(AppColors::TEXT_DIM)
                    .size(FontSize::SMALL),
            );
        });

    ui.add_space(Spacing::MD);
    ui.horizontal(|ui| {
        if ui.button(t!("single-gpu.back")).clicked() {
            app.single_gpu_state_mut().step = SingleGpuStep::Preview;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let btn = egui::Button::new(
                egui::RichText::new(t!("single-gpu.next")).color(egui::Color32::WHITE),
            )
            .fill(AppColors::PRIMARY);
            if ui.add(btn).clicked() {
                app.single_gpu_state_mut().step = SingleGpuStep::Confirm;
            }
        });
    });
}

// ─── Step 4: Confirm + Done ─────────────────────────────────────────

fn render_step_confirm(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let saved_dir = app.single_gpu_state().saved_dir.clone();
    let scripts_saved = app.single_gpu_state().scripts_saved;
    let dm = app.single_gpu_state().detected_dm.clone();
    let vm_name = app.single_gpu_state().vm_name.clone();

    ui.label(
        egui::RichText::new(t!("single-gpu.summary"))
            .size(FontSize::SUBHEADING)
            .color(AppColors::TEXT)
            .strong(),
    );

    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::CARD)
        .inner_margin(10.0)
        .stroke(egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(t!("single-gpu.summary-vm", name = vm_name))
                    .color(AppColors::TEXT)
                    .strong(),
            );
            ui.add_space(Spacing::XS);

            let line = match saved_dir.as_deref() {
                Some(d) => t!("single-gpu.scripts-saved-at", dir = d).into_owned(),
                None => t!("single-gpu.scripts-not-saved").into_owned(),
            };
            let color = if scripts_saved {
                AppColors::SUCCESS
            } else {
                AppColors::WARNING
            };
            ui.label(egui::RichText::new(line).color(color));

            ui.label(
                egui::RichText::new(t!("single-gpu.sudoers-pending")).color(AppColors::WARNING),
            );
        });

    ui.add_space(Spacing::SM);

    // ── The recovery rehearsal warning ──────────────────────────────
    let dm_disp = dm.as_deref().unwrap_or("$DM");
    egui::Frame::none()
        .fill(AppColors::BANNER_BG)
        .rounding(ThemeRounding::FRAME)
        .inner_margin(Spacing::MD)
        .stroke(egui::Stroke::new(
            0.5,
            AppColors::DANGER.linear_multiply(0.6),
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("\u{26A0}")
                        .size(FontSize::HEADING)
                        .color(AppColors::DANGER),
                );
                ui.label(
                    egui::RichText::new(t!("single-gpu.rehearse-title"))
                        .size(FontSize::SUBHEADING)
                        .color(AppColors::DANGER)
                        .strong(),
                );
            });
            ui.add_space(Spacing::XS);
            ui.label(egui::RichText::new(t!("single-gpu.rehearse-desc")).color(AppColors::TEXT));
            ui.label(
                egui::RichText::new(format!(
                    "    systemctl stop {0}; sleep 5; systemctl start {0}",
                    dm_disp
                ))
                .family(egui::FontFamily::Monospace)
                .color(AppColors::TEXT),
            );
            ui.label(
                egui::RichText::new(t!("single-gpu.rehearse-note"))
                    .color(AppColors::TEXT_DIM)
                    .size(FontSize::SMALL),
            );
        });

    ui.add_space(Spacing::MD);
    ui.horizontal(|ui| {
        if ui.button(t!("single-gpu.back")).clicked() {
            app.single_gpu_state_mut().step = SingleGpuStep::Permissions;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let btn = egui::Button::new(
                egui::RichText::new(t!("single-gpu.done"))
                    .color(egui::Color32::WHITE)
                    .strong(),
            )
            .fill(AppColors::PRIMARY)
            .min_size(egui::vec2(120.0, 30.0));
            if ui.add(btn).clicked() {
                // Persist the hook directory into the editing VmConfig if one
                // is loaded — this is the minimal config integration.
                if let Some(dir) = saved_dir.clone() {
                    if let Some(cfg) = app.editing_config_mut() {
                        cfg.vfio_hook_dir = Some(dir);
                    }
                }
                app.single_gpu_state_mut().open = false;
            }
        });
    });
}
