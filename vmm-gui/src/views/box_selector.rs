//! Box type selector — choose between Standard, Hardware Lab, and Power User modes.
//!
//! This screen appears when creating a new VM. The user selects a "Box" type,
//! which determines the available architectures, exposed options, and UI accent color.

use crate::app::LibreVmmApp;
use crate::theme;
use crate::theme::BoxColors;
use eframe::egui;
use vmm_core::qemu_archs::BoxType;

/// Render the Box type selector as a full-screen chooser.
pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);

                ui.heading("Create a New Virtual Machine");
                ui.add_space(theme::Spacing::XS);
                ui.label(
                    egui::RichText::new("Choose a mode that matches your use case")
                        .color(crate::theme::AppColors::TEXT_DIM),
                );

                ui.add_space(30.0);

                // Three box cards in a horizontal layout
                let available_width = ui.available_width().min(900.0);
                let card_width = (available_width - 40.0) / 3.0;

                ui.horizontal(|ui| {
                    let total_width = card_width * 3.0 + 40.0;
                    let left_pad = (ui.available_width() - total_width).max(0.0) / 2.0;
                    ui.add_space(left_pad);

                    for box_type in BoxType::all() {
                        render_box_card(app, ui, &box_type, card_width);
                        ui.add_space(20.0);
                    }
                });

                ui.add_space(30.0);

                // Back button
                if ui.button("Cancel").clicked() {
                    app.set_screen(crate::app::Screen::Home);
                }
            });
        }); // ScrollArea
}

/// Render a single Box type card.
fn render_box_card(app: &mut LibreVmmApp, ui: &mut egui::Ui, box_type: &BoxType, width: f32) {
    let primary = BoxColors::primary(box_type);
    let hover = BoxColors::hover(box_type);

    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 280.0), egui::Sense::click());

    let is_hovered = response.hovered();
    let bg = if is_hovered {
        crate::theme::AppColors::BG_HOVER
    } else {
        crate::theme::AppColors::BG_CARD
    };

    let painter = ui.painter_at(rect);

    // Card background
    painter.rect_filled(rect, egui::Rounding::same(10.0), bg);

    // Accent stripe at top
    let stripe_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 4.0));
    painter.rect_filled(
        stripe_rect,
        egui::Rounding {
            nw: 10.0,
            ne: 10.0,
            sw: 0.0,
            se: 0.0,
        },
        if is_hovered { hover } else { primary },
    );

    // Border on hover
    if is_hovered {
        painter.rect_stroke(
            rect,
            egui::Rounding::same(10.0),
            egui::Stroke::new(2.0, primary),
        );
    }

    // Content layout inside the card
    let content_rect = rect.shrink2(egui::vec2(20.0, 16.0));
    let mut pos_y = content_rect.min.y + 12.0;

    // Box icon
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(content_rect.center().x - 24.0, pos_y),
        egui::vec2(48.0, 48.0),
    );
    painter.rect_filled(
        icon_rect,
        egui::Rounding::same(theme::ThemeRounding::CARD),
        primary.linear_multiply(0.2),
    );
    painter.text(
        icon_rect.center(),
        egui::Align2::CENTER_CENTER,
        box_type.icon(),
        egui::FontId::new(18.0, egui::FontFamily::Monospace),
        primary,
    );
    pos_y += 60.0;

    // Box name
    painter.text(
        egui::pos2(content_rect.center().x, pos_y),
        egui::Align2::CENTER_TOP,
        box_type.display_name(),
        egui::FontId::new(18.0, egui::FontFamily::Proportional),
        crate::theme::AppColors::TEXT,
    );
    pos_y += 30.0;

    // Description — word-wrapped manually
    let desc = box_type.description();
    let galley = painter.layout(
        desc.to_string(),
        egui::FontId::new(12.5, egui::FontFamily::Proportional),
        crate::theme::AppColors::TEXT_DIM,
        content_rect.width() - 8.0,
    );
    let text_pos = egui::pos2(content_rect.center().x - galley.size().x / 2.0, pos_y);
    painter.galley(text_pos, galley, crate::theme::AppColors::TEXT_DIM);
    pos_y += 45.0;

    // Architecture hint
    let arch_hint = match box_type {
        BoxType::Standard => "x86_64 / i386",
        BoxType::HardwareLab => "All 24 QEMU architectures",
        BoxType::PowerUser => "x86_64 / ARM64 + VFIO",
    };
    painter.text(
        egui::pos2(content_rect.center().x, pos_y),
        egui::Align2::CENTER_TOP,
        arch_hint,
        egui::FontId::new(11.0, egui::FontFamily::Monospace),
        primary.linear_multiply(0.7),
    );

    // Handle click — navigate to appropriate wizard
    if response.clicked() {
        match box_type {
            BoxType::Standard => {
                // Standard box: go to the existing template wizard
                app.set_wizard_box_type(BoxType::Standard);
                app.set_screen(crate::app::Screen::CreateWizard(
                    crate::app::WizardStep::ChooseTemplate,
                ));
            },
            BoxType::HardwareLab => {
                // Hardware Lab: go to the architecture wizard
                app.set_wizard_box_type(BoxType::HardwareLab);
                app.set_screen(crate::app::Screen::ArchWizard(
                    crate::app::ArchWizardStep::ChooseArch,
                ));
            },
            BoxType::PowerUser => {
                // Power User: go to dedicated power user wizard
                app.set_wizard_box_type(BoxType::PowerUser);
                app.set_screen(crate::app::Screen::PowerWizard(
                    crate::app::PowerWizardStep::ChooseTemplate,
                ));
            },
        }
    }
}
