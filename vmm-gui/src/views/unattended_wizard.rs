//! Unattended install wizard — generate Windows Autounattend.xml or cloud-init ISO.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding, GRID_SPACING};
use eframe::egui;
use rust_i18n::t;
use vmm_core::unattended::{CloudInitConfig, UnattendedTarget, WindowsUnattendedConfig};

/// Wizard step.
#[derive(Debug, Clone, PartialEq)]
pub enum UnattendedStep {
    ChooseTarget,
    Configure,
    Review,
    Done,
}

/// State for the unattended install wizard.
pub struct UnattendedWizardState {
    pub open: bool,
    pub step: UnattendedStep,
    pub target: UnattendedTarget,
    pub win_config: WindowsUnattendedConfig,
    pub win_password_confirm: String,
    pub show_password: bool,
    pub cloud_config: CloudInitConfig,
    pub ssh_key_input: String,
    pub package_input: String,
    pub iso_path: Option<String>,
    pub error: Option<String>,
    pub vm_name: String,
}

impl Default for UnattendedWizardState {
    fn default() -> Self {
        Self {
            open: false,
            step: UnattendedStep::ChooseTarget,
            target: UnattendedTarget::Windows,
            win_config: WindowsUnattendedConfig::default(),
            win_password_confirm: String::new(),
            show_password: false,
            cloud_config: CloudInitConfig::default(),
            ssh_key_input: String::new(),
            package_input: String::new(),
            iso_path: None,
            error: None,
            vm_name: String::new(),
        }
    }
}

impl UnattendedWizardState {
    pub fn open_for(&mut self, vm_name: &str) {
        self.open = true;
        self.step = UnattendedStep::ChooseTarget;
        self.vm_name = vm_name.to_string();
        self.iso_path = None;
        self.error = None;
    }
}

pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let state = app.unattended_wizard_state();
    if !state.open {
        return;
    }

    let mut open = true;

    egui::Window::new(t!("unattended.window-title"))
        .open(&mut open)
        .resizable(true)
        .default_width(480.0)
        .default_height(500.0)
        .collapsible(false)
        .show(ctx, |ui| {
            let step = app.unattended_wizard_state().step.clone();

            // Step indicator
            ui.horizontal(|ui| {
                let steps = [
                    t!("unattended.step1"),
                    t!("unattended.step2"),
                    t!("unattended.step3"),
                    t!("unattended.step4"),
                ];
                let current = match step {
                    UnattendedStep::ChooseTarget => 0,
                    UnattendedStep::Configure => 1,
                    UnattendedStep::Review => 2,
                    UnattendedStep::Done => 3,
                };
                for (i, s) in steps.iter().enumerate() {
                    let color = if i == current {
                        AppColors::PRIMARY
                    } else {
                        AppColors::TEXT_DIM
                    };
                    ui.label(
                        egui::RichText::new(s.as_ref())
                            .size(theme::FontSize::LABEL)
                            .color(color)
                            .strong(),
                    );
                    if i < 3 {
                        ui.label(egui::RichText::new(">").color(AppColors::TEXT_DIM));
                    }
                }
            });
            ui.separator();
            ui.add_space(Spacing::SM);

            match step {
                UnattendedStep::ChooseTarget => render_choose_target(app, ui),
                UnattendedStep::Configure => render_configure(app, ui),
                UnattendedStep::Review => render_review(app, ui),
                UnattendedStep::Done => render_done(app, ui),
            }
        });

    if !open {
        app.unattended_wizard_state_mut().open = false;
    }
}

fn render_choose_target(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new(t!("unattended.choose-target"))
            .size(FontSize::HEADING)
            .strong(),
    );
    ui.add_space(Spacing::MD);

    let target = app.unattended_wizard_state().target.clone();

    // Windows option
    let win_selected = target == UnattendedTarget::Windows;
    egui::Frame::none()
        .fill(if win_selected {
            AppColors::BG_HOVER
        } else {
            AppColors::BG_CARD
        })
        .rounding(ThemeRounding::CARD)
        .inner_margin(Spacing::LG)
        .show(ui, |ui| {
            if ui
                .selectable_label(
                    win_selected,
                    egui::RichText::new(t!("unattended.windows-target"))
                        .size(FontSize::SUBHEADING)
                        .strong(),
                )
                .clicked()
            {
                app.unattended_wizard_state_mut().target = UnattendedTarget::Windows;
            }
            ui.label(
                egui::RichText::new(t!("unattended.windows-desc"))
                    .size(theme::FontSize::SMALL)
                    .color(AppColors::TEXT_DIM),
            );
        });

    ui.add_space(Spacing::SM);

    // Linux option
    let linux_selected = target == UnattendedTarget::LinuxCloudInit;
    egui::Frame::none()
        .fill(if linux_selected {
            AppColors::BG_HOVER
        } else {
            AppColors::BG_CARD
        })
        .rounding(ThemeRounding::CARD)
        .inner_margin(Spacing::LG)
        .show(ui, |ui| {
            if ui
                .selectable_label(
                    linux_selected,
                    egui::RichText::new(t!("unattended.linux-target"))
                        .size(FontSize::SUBHEADING)
                        .strong(),
                )
                .clicked()
            {
                app.unattended_wizard_state_mut().target = UnattendedTarget::LinuxCloudInit;
            }
            ui.label(
                egui::RichText::new(t!("unattended.linux-desc"))
                    .size(theme::FontSize::SMALL)
                    .color(AppColors::TEXT_DIM),
            );
        });

    ui.add_space(Spacing::LG);

    ui.horizontal(|ui| {
        if ui.button(t!("unattended.cancel")).clicked() {
            app.unattended_wizard_state_mut().open = false;
        }
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(t!("unattended.next")).color(egui::Color32::WHITE),
                )
                .fill(AppColors::PRIMARY),
            )
            .clicked()
        {
            app.unattended_wizard_state_mut().step = UnattendedStep::Configure;
        }
    });
}

fn render_configure(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let target = app.unattended_wizard_state().target.clone();

    egui::ScrollArea::vertical().show(ui, |ui| match target {
        UnattendedTarget::Windows => render_windows_config(app, ui),
        UnattendedTarget::LinuxCloudInit => render_linux_config(app, ui),
    });

    ui.add_space(Spacing::SM);
    ui.horizontal(|ui| {
        if ui.button(t!("unattended.back")).clicked() {
            app.unattended_wizard_state_mut().step = UnattendedStep::ChooseTarget;
        }
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(t!("unattended.next")).color(egui::Color32::WHITE),
                )
                .fill(AppColors::PRIMARY),
            )
            .clicked()
        {
            app.unattended_wizard_state_mut().step = UnattendedStep::Review;
        }
    });
}

fn render_windows_config(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new(t!("unattended.windows-config"))
            .size(FontSize::SUBHEADING)
            .strong(),
    );
    ui.add_space(Spacing::SM);

    egui::Grid::new("win_config")
        .num_columns(2)
        .spacing(GRID_SPACING)
        .show(ui, |ui| {
            ui.label(t!("unattended.hostname"));
            ui.add(
                egui::TextEdit::singleline(
                    &mut app.unattended_wizard_state_mut().win_config.hostname,
                )
                .desired_width(250.0),
            );
            ui.end_row();

            ui.label(t!("unattended.username"));
            ui.add(
                egui::TextEdit::singleline(
                    &mut app.unattended_wizard_state_mut().win_config.username,
                )
                .desired_width(250.0),
            );
            ui.end_row();

            ui.label(t!("unattended.password"));
            let show = app.unattended_wizard_state().show_password;
            ui.add(
                egui::TextEdit::singleline(
                    &mut app.unattended_wizard_state_mut().win_config.password,
                )
                .password(!show)
                .desired_width(250.0),
            );
            ui.end_row();

            ui.label(t!("unattended.confirm"));
            let show = app.unattended_wizard_state().show_password;
            ui.add(
                egui::TextEdit::singleline(
                    &mut app.unattended_wizard_state_mut().win_password_confirm,
                )
                .password(!show)
                .desired_width(250.0),
            );
            ui.end_row();

            ui.label("");
            let mut show = app.unattended_wizard_state().show_password;
            if ui
                .checkbox(&mut show, t!("unattended.show-password"))
                .changed()
            {
                app.unattended_wizard_state_mut().show_password = show;
            }
            ui.end_row();

            ui.label(t!("unattended.locale"));
            ui.add(
                egui::TextEdit::singleline(
                    &mut app.unattended_wizard_state_mut().win_config.locale,
                )
                .desired_width(100.0)
                .hint_text("en-US"),
            );
            ui.end_row();

            ui.label(t!("unattended.timezone"));
            ui.add(
                egui::TextEdit::singleline(
                    &mut app.unattended_wizard_state_mut().win_config.timezone,
                )
                .desired_width(250.0)
                .hint_text("UTC"),
            );
            ui.end_row();

            ui.label(t!("unattended.product-key"));
            let mut key = app
                .unattended_wizard_state()
                .win_config
                .product_key
                .clone()
                .unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut key)
                        .desired_width(250.0)
                        .hint_text(t!("unattended.optional")),
                )
                .changed()
            {
                app.unattended_wizard_state_mut().win_config.product_key =
                    if key.is_empty() { None } else { Some(key) };
            }
            ui.end_row();
        });

    ui.add_space(Spacing::SM);
    ui.checkbox(
        &mut app.unattended_wizard_state_mut().win_config.skip_oobe,
        t!("unattended.skip-oobe"),
    );
    ui.checkbox(
        &mut app.unattended_wizard_state_mut().win_config.auto_login,
        t!("unattended.auto-login"),
    );
    ui.checkbox(
        &mut app.unattended_wizard_state_mut().win_config.enable_rdp,
        t!("unattended.enable-rdp"),
    );
}

fn render_linux_config(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new(t!("unattended.cloud-init-config"))
            .size(FontSize::SUBHEADING)
            .strong(),
    );
    ui.add_space(Spacing::SM);

    egui::Grid::new("linux_config")
        .num_columns(2)
        .spacing(GRID_SPACING)
        .show(ui, |ui| {
            ui.label(t!("unattended.hostname"));
            ui.add(
                egui::TextEdit::singleline(
                    &mut app.unattended_wizard_state_mut().cloud_config.hostname,
                )
                .desired_width(250.0),
            );
            ui.end_row();

            ui.label(t!("unattended.username"));
            ui.add(
                egui::TextEdit::singleline(
                    &mut app.unattended_wizard_state_mut().cloud_config.username,
                )
                .desired_width(250.0),
            );
            ui.end_row();

            ui.label(t!("unattended.password"));
            let mut pass = app
                .unattended_wizard_state()
                .cloud_config
                .password
                .clone()
                .unwrap_or_default();
            let show = app.unattended_wizard_state().show_password;
            if ui
                .add(
                    egui::TextEdit::singleline(&mut pass)
                        .password(!show)
                        .desired_width(250.0)
                        .hint_text(t!("unattended.optional")),
                )
                .changed()
            {
                app.unattended_wizard_state_mut().cloud_config.password =
                    if pass.is_empty() { None } else { Some(pass) };
            }
            ui.end_row();

            ui.label(t!("unattended.timezone"));
            ui.add(
                egui::TextEdit::singleline(
                    &mut app.unattended_wizard_state_mut().cloud_config.timezone,
                )
                .desired_width(250.0),
            );
            ui.end_row();
        });

    ui.add_space(Spacing::SM);

    // SSH keys
    ui.label(egui::RichText::new(t!("unattended.ssh-keys")).strong());
    ui.add(
        egui::TextEdit::multiline(&mut app.unattended_wizard_state_mut().ssh_key_input)
            .desired_width(ui.available_width())
            .desired_rows(3)
            .hint_text(t!("unattended.ssh-keys-hint")),
    );

    ui.add_space(Spacing::SM);

    // Packages
    ui.label(egui::RichText::new(t!("unattended.packages")).strong());
    ui.add(
        egui::TextEdit::singleline(&mut app.unattended_wizard_state_mut().package_input)
            .desired_width(ui.available_width())
            .hint_text(t!("unattended.packages-hint")),
    );
}

fn render_review(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new(t!("unattended.review-generate"))
            .size(FontSize::SUBHEADING)
            .strong(),
    );
    ui.add_space(Spacing::SM);

    let state = app.unattended_wizard_state();
    let vm_name = state.vm_name.clone();

    match &state.target {
        UnattendedTarget::Windows => {
            egui::Grid::new("review_win")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label(t!("unattended.target"));
                    ui.label(t!("unattended.windows-target"));
                    ui.end_row();
                    ui.label(t!("unattended.hostname"));
                    ui.label(&state.win_config.hostname);
                    ui.end_row();
                    ui.label(t!("unattended.username"));
                    ui.label(&state.win_config.username);
                    ui.end_row();
                    ui.label(t!("unattended.locale"));
                    ui.label(&state.win_config.locale);
                    ui.end_row();
                    ui.label(t!("unattended.timezone"));
                    ui.label(&state.win_config.timezone);
                    ui.end_row();
                    ui.label(t!("unattended.auto-login-label"));
                    ui.label(if state.win_config.auto_login {
                        t!("arch.yes")
                    } else {
                        t!("arch.no")
                    });
                    ui.end_row();
                    ui.label(t!("unattended.rdp-label"));
                    ui.label(if state.win_config.enable_rdp {
                        t!("unattended.enabled")
                    } else {
                        t!("unattended.disabled")
                    });
                    ui.end_row();
                });
        },
        UnattendedTarget::LinuxCloudInit => {
            egui::Grid::new("review_linux")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label(t!("unattended.target"));
                    ui.label(t!("unattended.linux-target"));
                    ui.end_row();
                    ui.label(t!("unattended.hostname"));
                    ui.label(&state.cloud_config.hostname);
                    ui.end_row();
                    ui.label(t!("unattended.username"));
                    ui.label(&state.cloud_config.username);
                    ui.end_row();
                    ui.label(t!("unattended.timezone"));
                    ui.label(&state.cloud_config.timezone);
                    ui.end_row();
                });
        },
    }

    // Error display
    if let Some(err) = app.unattended_wizard_state().error.clone() {
        ui.add_space(Spacing::SM);
        ui.label(egui::RichText::new(&err).color(AppColors::DANGER));
    }

    // Tool availability
    if !vmm_core::unattended::iso_tool_available() {
        ui.add_space(Spacing::SM);
        ui.label(
            egui::RichText::new(t!("unattended.genisoimage-missing"))
                .color(AppColors::DANGER)
                .size(theme::FontSize::SMALL),
        );
    }

    ui.add_space(Spacing::MD);
    ui.horizontal(|ui| {
        if ui.button(t!("unattended.back")).clicked() {
            app.unattended_wizard_state_mut().step = UnattendedStep::Configure;
        }
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(t!("unattended.generate-iso")).color(egui::Color32::WHITE),
                )
                .fill(AppColors::PRIMARY),
            )
            .clicked()
        {
            app.action_generate_unattended_iso(&vm_name);
        }
    });
}

fn render_done(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new(t!("unattended.iso-success"))
            .size(FontSize::HEADING)
            .strong()
            .color(AppColors::RUNNING),
    );
    ui.add_space(Spacing::MD);

    if let Some(ref path) = app.unattended_wizard_state().iso_path {
        ui.label(t!("unattended.output-file"));
        egui::Frame::none()
            .fill(AppColors::BG_CARD)
            .rounding(ThemeRounding::BUTTON_SMALL)
            .inner_margin(Spacing::SM)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(path)
                        .monospace()
                        .size(theme::FontSize::LABEL),
                );
            });
    }

    ui.add_space(Spacing::SM);
    ui.label(
        egui::RichText::new(t!("unattended.attach-iso-hint"))
            .size(theme::FontSize::LABEL)
            .color(AppColors::TEXT_DIM),
    );

    ui.add_space(Spacing::LG);
    if ui.button(t!("unattended.close")).clicked() {
        app.unattended_wizard_state_mut().open = false;
    }
}
