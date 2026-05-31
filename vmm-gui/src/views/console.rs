//! Embedded VM console — renders VNC or SPICE framebuffer directly in the GUI.
//! Automatically uses SPICE when available (richer protocol), VNC as fallback.
//! Controls (Ctrl+Alt+Del, Disconnect) are in the tab bar, so this
//! only renders the framebuffer and handles input.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::AppColors;
use crate::vnc;
use eframe::egui;

/// Cached framebuffer state to avoid cloning 8+ MB of pixel data every frame.
/// Only updates when the backend thread signals new pixel data via the generation counter.
pub struct ConsoleCache {
    /// Last pixel generation we cloned from the backend state.
    generation: u64,
    /// Cached pixel data (only re-cloned when generation changes).
    pixels: Vec<u8>,
    /// Cached texture handle (only rebuilt when pixels change).
    texture: Option<egui::TextureHandle>,
    /// Cached resolution dimensions (for the format string).
    cached_width: u16,
    cached_height: u16,
    /// Pre-formatted resolution string, rebuilt only when dimensions change.
    resolution_label: String,
}

impl Default for ConsoleCache {
    fn default() -> Self {
        Self {
            generation: u64::MAX, // Force first update
            pixels: Vec::new(),
            texture: None,
            cached_width: 0,
            cached_height: 0,
            resolution_label: String::new(),
        }
    }
}

/// Unified console state extracted from either VNC or SPICE backend.
struct ConsoleSnapshot {
    width: u16,
    height: u16,
    connected: bool,
    error: Option<String>,
}

/// Extract framebuffer state from the active backend, updating the cache if pixels changed.
/// Returns None if no backend is active or the mutex is poisoned.
fn extract_console_state(app: &mut LibreVmmApp) -> Option<ConsoleSnapshot> {
    if app.console_is_spice() {
        let fb = app.console_spice_framebuffer()?.clone();
        let state = fb.get_state()?;
        let gen = state.pixel_generation;
        let cache = app.console_cache_mut();
        if cache.generation != gen {
            cache.pixels.clear();
            cache.pixels.extend_from_slice(&state.pixels);
            cache.generation = gen;
            cache.texture = None;
        }
        Some(ConsoleSnapshot {
            width: state.width,
            height: state.height,
            connected: state.connected,
            error: state.error.clone(),
        })
    } else {
        let fb = app.console_framebuffer()?.clone();
        let state = fb.get_state()?;
        let gen = state.pixel_generation;
        let cache = app.console_cache_mut();
        if cache.generation != gen {
            cache.pixels.clear();
            cache.pixels.extend_from_slice(&state.pixels);
            cache.generation = gen;
            cache.texture = None;
        }
        Some(ConsoleSnapshot {
            width: state.width,
            height: state.height,
            connected: state.connected,
            error: state.error.clone(),
        })
    }
}

/// Send a key event to whatever backend is active.
fn send_key(app: &LibreVmmApp, down: bool, keysym: u32) {
    if app.console_is_spice() {
        if let Some(ref fb) = app.console_spice_framebuffer() {
            fb.send_key(down, keysym);
        }
    } else {
        if let Some(ref fb) = app.console_framebuffer() {
            fb.send_key(down, keysym);
        }
    }
}

/// Send a mouse event to whatever backend is active.
fn send_mouse(app: &LibreVmmApp, x: u16, y: u16, buttons: u8) {
    if app.console_is_spice() {
        if let Some(ref fb) = app.console_spice_framebuffer() {
            fb.send_mouse(x, y, buttons);
        }
    } else {
        if let Some(ref fb) = app.console_framebuffer() {
            fb.send_mouse(x, y, buttons);
        }
    }
}

pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui, name: &str) {
    let name = name.to_string();
    let is_spice = app.console_is_spice();

    // Check if any backend is connected
    let has_backend = if is_spice {
        app.console_spice_framebuffer().is_some()
    } else {
        app.console_framebuffer().is_some()
    };

    if !has_backend {
        // No active console — show connect prompt
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            let is_running = app
                .selected_vm_state()
                .map(|s| s == vmm_core::domain::VmState::Running)
                .unwrap_or(false);

            if is_running {
                ui.label(
                    egui::RichText::new("Console not connected")
                        .size(14.0)
                        .color(AppColors::TEXT_DIM),
                );
                ui.add_space(theme::Spacing::SM);
                if ui.button("Connect").clicked() {
                    app.action_console(&name);
                }
            } else {
                ui.label(
                    egui::RichText::new("VM is not running")
                        .size(14.0)
                        .color(AppColors::TEXT_DIM),
                );
                ui.label(
                    egui::RichText::new("Start the VM to access the console.")
                        .size(12.0)
                        .color(AppColors::MUTED),
                );
            }
        });
        return;
    }

    // Extract state from whichever backend is active
    let Some(snapshot) = extract_console_state(app) else {
        // Mutex poisoned
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            let proto = if is_spice { "SPICE" } else { "VNC" };
            ui.label(
                egui::RichText::new(format!(
                    "Internal error: {} state corrupted (mutex poisoned)",
                    proto
                ))
                .size(14.0)
                .color(AppColors::DANGER),
            );
            if ui.button("Reconnect").clicked() {
                app.action_console(&name);
            }
        });
        return;
    };

    let ConsoleSnapshot {
        width,
        height,
        connected,
        error,
    } = snapshot;

    if !connected {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            if let Some(err) = error {
                ui.label(
                    egui::RichText::new(&err)
                        .size(14.0)
                        .color(AppColors::DANGER),
                );
                ui.add_space(theme::Spacing::SM);
                ui.horizontal(|ui| {
                    if ui.button("Reconnect").clicked() {
                        app.action_console(&name);
                    }
                    if is_spice {
                        if ui.button("Try VNC instead").clicked() {
                            app.force_vnc_console(&name);
                        }
                    }
                });
            } else {
                let proto = if is_spice { "SPICE" } else { "VNC" };
                ui.label(
                    egui::RichText::new(format!("Connecting via {}...", proto))
                        .size(14.0)
                        .color(AppColors::TEXT_DIM),
                );
                ui.spinner();
                ui.add_space(theme::Spacing::SM);
                if ui.button("Cancel").clicked() {
                    app.disconnect_console();
                }
            }
        });
        return;
    }

    if width == 0 || height == 0 {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            let proto = if is_spice { "SPICE" } else { "VNC" };
            ui.label(
                egui::RichText::new(format!("Connected via {} — waiting for display...", proto))
                    .size(14.0)
                    .color(AppColors::TEXT_DIM),
            );
            ui.spinner();
            ui.add_space(theme::Spacing::XS);
            ui.label(
                egui::RichText::new("The guest display will appear once the GPU initializes.")
                    .size(theme::FontSize::SMALL)
                    .color(AppColors::MUTED),
            );
        });
        return;
    }

    // Build/reuse cached texture and resolution label.
    let (texture, resolution_label) = {
        let cache = app.console_cache_mut();

        // SECURITY: Validate pixel buffer size matches expected dimensions (CWE-680)
        let expected_len = (width as usize) * (height as usize) * 4;
        if cache.pixels.len() != expected_len || expected_len > 64 * 1024 * 1024 {
            ui.colored_label(egui::Color32::YELLOW, "Invalid framebuffer dimensions");
            return;
        }

        let texture = if let Some(ref tex) = cache.texture {
            tex.clone()
        } else {
            let color_image = egui::ColorImage::from_rgba_unmultiplied(
                [width as usize, height as usize],
                &cache.pixels,
            );
            let tex =
                ui.ctx()
                    .load_texture("vm-console", color_image, egui::TextureOptions::LINEAR);
            cache.texture = Some(tex.clone());
            tex
        };

        if cache.cached_width != width || cache.cached_height != height {
            cache.cached_width = width;
            cache.cached_height = height;
            let proto = if is_spice { "SPICE" } else { "VNC" };
            cache.resolution_label = format!("{}x{} ({})", width, height, proto);
        }

        (texture, cache.resolution_label.clone())
    };

    // === Console viewport rendering ===
    let panel_rect = ui.max_rect();
    let cursor = ui.cursor().min;
    let area = egui::Rect::from_min_max(cursor, panel_rect.max);
    let bg_response = ui.allocate_rect(area, egui::Sense::click_and_drag());
    let area = bg_response.rect;
    let aw = area.width().max(1.0);
    let ah = area.height().max(1.0);

    // Auto-resize
    let target_w = aw as u16;
    let target_h = ah as u16;
    app.maybe_request_console_resize(target_w, target_h);

    let aspect = width as f32 / height as f32;
    let display_size = if aw / ah > aspect {
        egui::vec2(ah * aspect, ah)
    } else {
        egui::vec2(aw, aw / aspect)
    };

    let painter = ui.painter();
    painter.rect_filled(area, 0.0, AppColors::CONSOLE_BG);

    let console_rect = egui::Rect::from_center_size(area.center(), display_size);

    painter.image(
        texture.id(),
        console_rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    painter.rect_stroke(
        console_rect,
        0.0,
        egui::Stroke::new(1.0, AppColors::STROKE_SUBTLE),
    );

    // Resolution + protocol label
    painter.text(
        egui::pos2(console_rect.right() - 4.0, console_rect.bottom() + 2.0),
        egui::Align2::RIGHT_TOP,
        &resolution_label,
        egui::FontId::proportional(9.0),
        egui::Color32::from_rgb(100, 100, 100),
    );

    // === Input handling ===
    {
        if bg_response.hovered() {
            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                if console_rect.contains(pos) {
                    let rel_x = ((pos.x - console_rect.min.x) / display_size.x * width as f32)
                        .max(0.0)
                        .min(width.saturating_sub(1) as f32) as u16;
                    let rel_y = ((pos.y - console_rect.min.y) / display_size.y * height as f32)
                        .max(0.0)
                        .min(height.saturating_sub(1) as f32)
                        as u16;

                    let mut buttons: u8 = 0;
                    let pointer = ui.input(|i| i.pointer.clone());
                    if pointer.button_down(egui::PointerButton::Primary) {
                        buttons |= 1;
                    }
                    if pointer.button_down(egui::PointerButton::Middle) {
                        buttons |= 2;
                    }
                    if pointer.button_down(egui::PointerButton::Secondary) {
                        buttons |= 4;
                    }

                    let scroll = ui.input(|i| i.raw_scroll_delta);
                    if scroll.y > 0.0 {
                        buttons |= 8;
                    } else if scroll.y < 0.0 {
                        buttons |= 16;
                    }

                    send_mouse(app, rel_x.min(width - 1), rel_y.min(height - 1), buttons);
                }
            }

            let has_events = ui.input(|i| !i.events.is_empty());
            if has_events {
                let events: Vec<egui::Event> = ui.input(|i| i.events.clone());
                let modifiers = ui.input(|i| i.modifiers);

                for event in &events {
                    match event {
                        egui::Event::Key { key, pressed, .. } => {
                            if modifiers.ctrl && !modifiers.alt {
                                continue;
                            }
                            if let Some(keysym) = vnc::egui_key_to_keysym(*key, &modifiers) {
                                let is_printable = keysym >= 0x20 && keysym <= 0x7e;
                                if !is_printable || modifiers.alt {
                                    send_key(app, *pressed, keysym);
                                }
                            }
                        },
                        egui::Event::Text(text) => {
                            for ch in text.chars() {
                                let keysym = ch as u32;
                                if keysym >= 0x20 && keysym <= 0x7e {
                                    send_key(app, true, keysym);
                                    send_key(app, false, keysym);
                                }
                            }
                        },
                        _ => {},
                    }
                }
            }

            bg_response.request_focus();
        }
    }

    // === Drag-and-Drop file transfer ===
    let has_dropped = ui.input(|i| !i.raw.dropped_files.is_empty());
    if has_dropped {
        let dropped_files: Vec<egui::DroppedFile> = ui.input(|i| i.raw.dropped_files.clone());
        for file in &dropped_files {
            if let Some(ref path) = file.path {
                let path_str = path.display().to_string();
                app.action_drop_file_to_guest(&name, &path_str);
            }
        }
    }

    let hovering_files = ui.input(|i| i.raw.hovered_files.len() > 0);
    if hovering_files {
        let painter = ui.painter();
        painter.rect_filled(
            area,
            0.0,
            egui::Color32::from_rgba_unmultiplied(30, 100, 200, 80),
        );
        painter.text(
            area.center(),
            egui::Align2::CENTER_CENTER,
            "Drop files to transfer to guest",
            egui::FontId::proportional(18.0),
            egui::Color32::WHITE,
        );
    }

    if let Some(ref msg) = app.drop_transfer_message() {
        let is_err = app.drop_transfer_is_error();
        let color = if is_err {
            AppColors::DANGER
        } else {
            AppColors::SUCCESS
        };
        let painter = ui.painter();
        let label_pos = egui::pos2(console_rect.left() + 8.0, console_rect.top() + 8.0);
        painter.text(
            label_pos,
            egui::Align2::LEFT_TOP,
            msg,
            egui::FontId::proportional(12.0),
            color,
        );
    }

    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
}
