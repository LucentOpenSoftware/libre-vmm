//! Multi-display support — layout management for multi-head VMs.
//!
//! When a VM has `display_count > 1`, this module provides layout controls
//! and state management for viewing multiple display heads. The actual VNC
//! framebuffer rendering is handled by the console view.
//!
//! TODO(wave-14): Wire `render_layout_toolbar` into `console_toolbar` (or the
//! console view) and use `compute_layout_rects` to partition the framebuffer
//! area when `display_count > 1`. The state, accessors on `LibreVmmApp`
//! (`multi_display_state` / `multi_display_state_mut`) and rendering helpers
//! are already in place — only the call sites in the console view are missing.

use crate::app::LibreVmmApp;
use crate::theme::AppColors;
use eframe::egui;

/// Display layout mode.
// TODO(wave-14): used by `render_layout_toolbar` / `compute_layout_rects`;
// remove `allow(dead_code)` once the console view consumes them.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum DisplayLayout {
    SplitHorizontal,
    SplitVertical,
    Single(usize), // Show only display N
}

/// State for multi-display management.
// TODO(wave-14): consumed by the (yet-to-be-wired) layout toolbar.
#[allow(dead_code)]
pub struct MultiDisplayState {
    pub layout: DisplayLayout,
    pub display_count: usize,
    /// Which display head is currently active (for keyboard/mouse input routing).
    pub active_head: usize,
}

impl Default for MultiDisplayState {
    fn default() -> Self {
        Self {
            layout: DisplayLayout::SplitHorizontal,
            display_count: 1,
            active_head: 0,
        }
    }
}

/// Render the display layout selector toolbar.
/// Only shown when `display_count > 1`.
// TODO(wave-14): call from `console_toolbar` once multi-head console rendering
// is implemented.
#[allow(dead_code)]
pub fn render_layout_toolbar(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let display_count = app.multi_display_state().display_count;
    if display_count <= 1 {
        return;
    }

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} displays", display_count))
                .size(10.0)
                .color(AppColors::TEXT_DIM),
        );

        ui.separator();

        let layout = app.multi_display_state().layout.clone();

        if ui
            .selectable_label(layout == DisplayLayout::SplitHorizontal, "\u{25A3} Split H")
            .on_hover_text("Side by side")
            .clicked()
        {
            app.multi_display_state_mut().layout = DisplayLayout::SplitHorizontal;
        }
        if ui
            .selectable_label(layout == DisplayLayout::SplitVertical, "\u{25A4} Split V")
            .on_hover_text("Stacked")
            .clicked()
        {
            app.multi_display_state_mut().layout = DisplayLayout::SplitVertical;
        }

        for i in 0..display_count {
            if ui
                .selectable_label(
                    layout == DisplayLayout::Single(i),
                    format!("Display {}", i + 1),
                )
                .clicked()
            {
                app.multi_display_state_mut().layout = DisplayLayout::Single(i);
            }
        }

        ui.separator();

        // Active head indicator
        let active = app.multi_display_state().active_head;
        ui.label(
            egui::RichText::new(format!("Input: Display {}", active + 1))
                .size(10.0)
                .color(AppColors::PRIMARY),
        );
    });
}

/// Compute the layout rectangles for each display panel.
/// Returns a Vec of (display_index, sub-rect) within the available area.
// TODO(wave-14): consumed by the console view once multi-head rendering lands.
#[allow(dead_code)]
pub fn compute_layout_rects(
    available: egui::Vec2,
    layout: &DisplayLayout,
    display_count: usize,
) -> Vec<(usize, egui::Rect)> {
    let mut rects = Vec::new();

    match layout {
        DisplayLayout::Single(idx) => {
            let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), available);
            rects.push((*idx, rect));
        },
        DisplayLayout::SplitHorizontal => {
            let panel_width = available.x / display_count as f32;
            for i in 0..display_count {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(i as f32 * panel_width, 0.0),
                    egui::vec2(panel_width, available.y),
                );
                rects.push((i, rect));
            }
        },
        DisplayLayout::SplitVertical => {
            let panel_height = available.y / display_count as f32;
            for i in 0..display_count {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(0.0, i as f32 * panel_height),
                    egui::vec2(available.x, panel_height),
                );
                rects.push((i, rect));
            }
        },
    }

    rects
}
