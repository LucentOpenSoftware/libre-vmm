//! Picture-in-Picture floating console — shows a mini VNC console as
//! an always-on-top floating window within the app.

use crate::app::LibreVmmApp;
use crate::theme::AppColors;
use eframe::egui;
use rust_i18n::t;

/// PiP state tracked in the app.
pub struct PipState {
    pub open: bool,
    pub size: egui::Vec2,
    pub opacity: f32,
}

impl Default for PipState {
    fn default() -> Self {
        Self {
            open: false,
            size: egui::vec2(320.0, 240.0),
            opacity: 0.9,
        }
    }
}

/// Render PiP window as a floating egui::Window.
pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    if !app.pip_state().open {
        return;
    }

    let has_fb = app.console_framebuffer().is_some();
    if !has_fb {
        // No console — close PiP
        app.pip_state_mut().open = false;
        return;
    }

    let pip_size = app.pip_state().size;
    let opacity = app.pip_state().opacity;

    egui::Window::new("Picture-in-Picture")
        .id(egui::Id::new("pip_console"))
        .default_size(pip_size)
        .resizable(true)
        .collapsible(true)
        .title_bar(true)
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(egui::Color32::from_rgb(15, 15, 15))
                .multiply_with_opacity(opacity),
        )
        .show(ctx, |ui| {
            // Size controls
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (label, size, tip) in [
                    (
                        "S",
                        egui::vec2(240.0, 180.0),
                        t!("tooltip.pip-size-s").to_string(),
                    ),
                    (
                        "M",
                        egui::vec2(320.0, 240.0),
                        t!("tooltip.pip-size-m").to_string(),
                    ),
                    (
                        "L",
                        egui::vec2(480.0, 360.0),
                        t!("tooltip.pip-size-l").to_string(),
                    ),
                ] {
                    if ui.small_button(label).on_hover_text(tip).clicked() {
                        app.pip_state_mut().size = size;
                    }
                }

                // Opacity slider
                ui.separator();
                ui.label(
                    egui::RichText::new("Opacity:")
                        .size(10.0)
                        .color(AppColors::TEXT_DIM),
                );
                let mut op = app.pip_state().opacity;
                if ui
                    .add(
                        egui::Slider::new(&mut op, 0.3..=1.0)
                            .show_value(false)
                            .step_by(0.1),
                    )
                    .changed()
                {
                    app.pip_state_mut().opacity = op;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("\u{2716}")
                        .on_hover_text(t!("tooltip.close").to_string())
                        .clicked()
                    {
                        app.pip_state_mut().open = false;
                    }
                });
            });

            ui.separator();

            // Render mini console
            let Some(fb) = app.console_framebuffer() else {
                return;
            };

            // SECURITY: CWE-662 — Handle poisoned mutex; treat as no signal.
            let (width, height, pixels, connected, _error) = {
                let Some(state) = fb.get_state() else {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            egui::RichText::new("State error")
                                .size(12.0)
                                .color(AppColors::TEXT_DIM),
                        );
                    });
                    return;
                };
                (
                    state.width,
                    state.height,
                    state.pixels.clone(),
                    state.connected,
                    state.error.clone(),
                )
            };

            if !connected || width == 0 || height == 0 {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("No signal")
                            .size(12.0)
                            .color(AppColors::TEXT_DIM),
                    );
                });
                return;
            }

            let color_image = egui::ColorImage::from_rgba_unmultiplied(
                [width as usize, height as usize],
                &pixels,
            );
            let texture =
                ui.ctx()
                    .load_texture("pip-console", color_image, egui::TextureOptions::LINEAR);

            // Fit to available space maintaining aspect ratio
            let avail = ui.available_size();
            let aspect = width as f32 / height as f32;
            let display_size = if avail.x / avail.y.max(1.0) > aspect {
                egui::vec2(avail.y.max(1.0) * aspect, avail.y.max(1.0))
            } else {
                egui::vec2(avail.x.max(1.0), avail.x.max(1.0) / aspect)
            };

            let image = egui::Image::from_texture(egui::load::SizedTexture::new(
                texture.id(),
                display_size,
            ));
            ui.add(image);

            // VM name badge
            if let Some(name) = app.console_vm_name() {
                ui.label(
                    egui::RichText::new(name)
                        .size(9.0)
                        .color(AppColors::TEXT_DIM),
                );
            }
        });
}
