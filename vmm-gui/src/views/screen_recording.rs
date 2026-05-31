//! Screen recording state and settings panel.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::{AppColors, Spacing, ThemeRounding, GRID_SPACING};
use eframe::egui;
use rust_i18n::t;
use vmm_core::screen_recording::{RecordingConfig, RecordingStatus, VideoFormat};

/// State for screen recording.
pub struct ScreenRecordingState {
    pub recording: Option<vmm_core::screen_recording::ScreenRecording>,
    pub config: RecordingConfig,
    pub output_dir: String,
    pub show_settings: bool,
}

impl Default for ScreenRecordingState {
    fn default() -> Self {
        Self {
            recording: None,
            config: RecordingConfig::default(),
            output_dir: vmm_core::screen_recording::default_output_dir(),
            show_settings: false,
        }
    }
}

impl ScreenRecordingState {
    pub fn is_recording(&self) -> bool {
        self.recording
            .as_ref()
            .map(|r| r.status == RecordingStatus::Recording)
            .unwrap_or(false)
    }

    pub fn elapsed_secs(&self) -> u64 {
        self.recording
            .as_ref()
            .and_then(|r| r.started_at)
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0)
    }
}

/// Render recording controls in the console toolbar.
pub fn render_toolbar_controls(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let is_recording = app.screen_recording_state().is_recording();

    if is_recording {
        let elapsed = app.screen_recording_state().elapsed_secs();
        let mins = elapsed / 60;
        let secs = elapsed % 60;

        // Pulsing red recording indicator
        let time = ui.input(|i| i.time);
        let alpha = ((time * 3.0).sin() * 0.5 + 0.5) as u8;
        let pulse_color = egui::Color32::from_rgba_unmultiplied(255, 50, 50, 128 + alpha);

        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!("\u{25CF} {:02}:{:02}", mins, secs))
                        .size(theme::FontSize::SMALL)
                        .color(egui::Color32::WHITE),
                )
                .fill(pulse_color)
                .rounding(ThemeRounding::BUTTON_SMALL),
            )
            .on_hover_text(t!("recording.stop-tooltip"))
            .clicked()
        {
            app.action_stop_recording();
        }

        // Request repaint for animation
        ui.ctx().request_repaint();
    } else {
        // Record button
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(t!("recording.rec"))
                        .size(theme::FontSize::SMALL)
                        .color(AppColors::TEXT),
                )
                .fill(AppColors::BG_HOVER)
                .rounding(ThemeRounding::BUTTON_SMALL),
            )
            .on_hover_text(t!("recording.start-tooltip"))
            .clicked()
        {
            app.action_start_recording();
        }
    }

    // Screenshot button
    if ui
        .add(
            egui::Button::new(
                egui::RichText::new("\u{1F4F7}")
                    .size(theme::FontSize::SMALL)
                    .color(AppColors::TEXT),
            )
            .fill(AppColors::BG_HOVER)
            .rounding(ThemeRounding::BUTTON_SMALL),
        )
        .on_hover_text(t!("recording.screenshot-tooltip"))
        .clicked()
    {
        app.action_take_screenshot();
    }
}

/// Render recording settings dialog.
pub fn render_settings(app: &mut LibreVmmApp, ctx: &egui::Context) {
    let show = app.screen_recording_state().show_settings;
    if !show {
        return;
    }

    // Check Esc to close the dialog (CWE-1216 — UX accessibility)
    if theme::escape_pressed(ctx) {
        app.screen_recording_state_mut().show_settings = false;
        return;
    }

    let mut open = true;

    egui::Window::new(t!("recording.settings-title"))
        .open(&mut open)
        .resizable(false)
        .default_width(320.0)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::Grid::new("rec_settings")
                .num_columns(2)
                .spacing(GRID_SPACING)
                .show(ui, |ui| {
                    ui.label(t!("recording.fps"));
                    let fps_options: &[(u32, &str)] =
                        &[(5, "5"), (10, "10"), (15, "15"), (30, "30")];
                    let current_fps = app.screen_recording_state().config.fps;
                    egui::ComboBox::from_id_salt("rec_fps")
                        .selected_text(format!("{}", current_fps))
                        .show_ui(ui, |ui| {
                            for (val, label) in fps_options {
                                if ui.selectable_label(current_fps == *val, *label).clicked() {
                                    app.screen_recording_state_mut().config.fps = *val;
                                }
                            }
                        });
                    ui.end_row();

                    ui.label(t!("recording.format"));
                    let current_fmt = app.screen_recording_state().config.format.clone();
                    let fmt_label = match current_fmt {
                        VideoFormat::Mp4 => "MP4",
                        VideoFormat::WebM => "WebM",
                        VideoFormat::Gif => "GIF",
                    };
                    egui::ComboBox::from_id_salt("rec_format")
                        .selected_text(fmt_label)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(current_fmt == VideoFormat::Mp4, "MP4")
                                .clicked()
                            {
                                app.screen_recording_state_mut().config.format = VideoFormat::Mp4;
                            }
                            if ui
                                .selectable_label(current_fmt == VideoFormat::WebM, "WebM")
                                .clicked()
                            {
                                app.screen_recording_state_mut().config.format = VideoFormat::WebM;
                            }
                            if ui
                                .selectable_label(current_fmt == VideoFormat::Gif, "GIF")
                                .clicked()
                            {
                                app.screen_recording_state_mut().config.format = VideoFormat::Gif;
                            }
                        });
                    ui.end_row();

                    ui.label(t!("recording.output-dir"));
                    ui.add(
                        egui::TextEdit::singleline(
                            &mut app.screen_recording_state_mut().output_dir,
                        )
                        .desired_width(200.0),
                    );
                    ui.end_row();
                });

            if !vmm_core::screen_recording::ffmpeg_available() {
                ui.add_space(Spacing::SM);
                ui.label(
                    egui::RichText::new(t!("recording.ffmpeg-missing"))
                        .color(AppColors::DANGER)
                        .size(theme::FontSize::SMALL),
                );
            }
        });

    if !open {
        app.screen_recording_state_mut().show_settings = false;
    }
}
