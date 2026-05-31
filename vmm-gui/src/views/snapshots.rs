//! Snapshot manager panel — visual tree, create, revert, delete snapshots.
//!
//! Wave 11.1: Upgraded the flat indented list to a proper visual snapshot tree
//! using egui::Painter for the connector lines. Each row paints:
//!   - vertical "│" lines for every ancestor depth that still has more siblings
//!   - a "├──" tee or "└──" elbow connector for the node itself
//!   - a filled (current) or hollow (others) node disc
//! Rows are selectable, highlight when selected, and expose a right-click
//! context menu plus inline action buttons for the selected snapshot.

use crate::app::{LibreVmmApp, SnapshotOp};
use crate::theme;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;
use vmm_core::snapshot::{build_snapshot_tree, SnapshotInfo, SnapshotTreeNode};

/// Horizontal indent per tree depth level. Wide enough to fit "├── " glyphs
/// comfortably and visually distinguish nested branches.
const INDENT_PER_LEVEL: f32 = 22.0;
/// Vertical row height reserved for one snapshot entry's node area.
const ROW_HEIGHT: f32 = 56.0;
/// Radius of the node disc that marks each snapshot.
const NODE_RADIUS: f32 = 5.0;

pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let _vm_name = match app.selected_vm() {
        Some(name) => name.to_string(),
        None => {
            ui.label(t!("menu.vm.no-selection"));
            return;
        },
    };

    ui.horizontal(|ui| {
        ui.heading(
            egui::RichText::new(t!("snap.title"))
                .size(FontSize::HEADING)
                .color(AppColors::TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(format!("🔄 {}", t!("snap.refresh")))
                .clicked()
            {
                app.refresh_snapshots();
            }
        });
    });
    ui.add_space(theme::Spacing::SM);

    // Create snapshot section — unchanged from prior implementation.
    egui::Frame::none()
        .fill(AppColors::BG_CARD)
        .rounding(ThemeRounding::CARD)
        .inner_margin(theme::Spacing::MD)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(t!("snap.take"))
                    .size(FontSize::SUBHEADING)
                    .strong()
                    .color(AppColors::TEXT),
            );
            ui.add_space(Spacing::SM);

            egui::Grid::new("snapshot_create")
                .num_columns(2)
                .spacing([Spacing::MD, Spacing::SM])
                .show(ui, |ui| {
                    ui.label(t!("snap.name"));
                    let name = app.snapshot_name_mut();
                    ui.add(
                        egui::TextEdit::singleline(name)
                            .hint_text(t!("snap.name-hint"))
                            .desired_width(250.0),
                    );
                    ui.end_row();

                    ui.label(t!("snap.description"));
                    let desc = app.snapshot_description_mut();
                    ui.add(
                        egui::TextEdit::singleline(desc)
                            .hint_text(t!("snap.desc-hint"))
                            .desired_width(250.0),
                    );
                    ui.end_row();
                });

            ui.add_space(Spacing::SM);

            let can_create = !app.snapshot_name().is_empty();
            let btn =
                egui::Button::new(egui::RichText::new(t!("snap.take")).color(egui::Color32::WHITE))
                    .fill(if can_create {
                        AppColors::PRIMARY
                    } else {
                        AppColors::MUTED
                    })
                    .rounding(ThemeRounding::BUTTON);

            if ui.add_enabled(can_create, btn).clicked() {
                app.action_take_snapshot();
            }
        });

    ui.add_space(theme::Spacing::MD);

    // Empty state
    let snap_count = app.snapshots().len();
    if snap_count == 0 {
        ui.add_space(Spacing::XL);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(t!("snap.no-snapshots"))
                    .size(FontSize::SUBHEADING)
                    .color(AppColors::TEXT_DIM),
            );
            ui.label(
                egui::RichText::new(t!("snap.no-snapshots-sub"))
                    .size(FontSize::LABEL)
                    .color(AppColors::MUTED),
            );
        });
        return;
    }

    ui.label(
        egui::RichText::new(t!(
            "snap.count",
            count = snap_count,
            s = if snap_count != 1 { "s" } else { "" }
        ))
        .size(FontSize::BODY)
        .color(AppColors::TEXT_DIM),
    );
    ui.add_space(Spacing::XS);

    // Build the tree once per frame (O(N) hashmap). The list only changes on
    // refresh_snapshots so this is cheap.
    let snapshots = app.snapshots().to_vec();
    let tree = build_snapshot_tree(&snapshots);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // `ancestor_continues[d]` = true iff there is a vertical "│" line
            // to be drawn at depth `d` because some ancestor at that level
            // still has more siblings below. Initialized empty; we push/pop
            // as we descend.
            let mut ancestor_continues: Vec<bool> = Vec::new();
            let root_count = tree.len();
            for (i, root) in tree.iter().enumerate() {
                let is_last_root = i + 1 == root_count;
                render_node(app, ui, root, &mut ancestor_continues, is_last_root);
            }
        });
}

/// Recursively render a node and its children.
///
/// `ancestor_continues` describes the spine lines above the current node:
/// for each ancestor depth, true means "draw a │ at this column".
fn render_node(
    app: &mut LibreVmmApp,
    ui: &mut egui::Ui,
    node: &SnapshotTreeNode,
    ancestor_continues: &mut Vec<bool>,
    is_last_sibling: bool,
) {
    render_row(app, ui, &node.info, ancestor_continues, is_last_sibling);

    // Recurse into children with our own "still has more siblings?" pushed.
    if !node.children.is_empty() {
        ancestor_continues.push(!is_last_sibling);
        let child_count = node.children.len();
        for (i, child) in node.children.iter().enumerate() {
            let last = i + 1 == child_count;
            render_node(app, ui, child, ancestor_continues, last);
        }
        ancestor_continues.pop();
    }
}

/// Render one row: tree connector lines + selectable snapshot card.
fn render_row(
    app: &mut LibreVmmApp,
    ui: &mut egui::Ui,
    snap: &SnapshotInfo,
    ancestor_continues: &[bool],
    is_last_sibling: bool,
) {
    let depth = ancestor_continues.len();
    let total_indent = depth as f32 * INDENT_PER_LEVEL + INDENT_PER_LEVEL;
    let full_width = ui.available_width();

    // Allocate the row rectangle up-front so we can paint connectors anywhere
    // inside it (egui::Painter draws into the current clip rect).
    let (row_rect, row_response) =
        ui.allocate_exact_size(egui::vec2(full_width, ROW_HEIGHT), egui::Sense::click());

    let painter = ui.painter_at(row_rect);
    let line_color = AppColors::STROKE_SUBTLE;
    let line_stroke = egui::Stroke::new(1.0, line_color);

    // 1) Draw vertical spine lines for every ancestor depth still continuing.
    for (d, continues) in ancestor_continues.iter().enumerate() {
        if !continues {
            continue;
        }
        let x = row_rect.left() + (d as f32 + 0.5) * INDENT_PER_LEVEL;
        painter.line_segment(
            [
                egui::pos2(x, row_rect.top()),
                egui::pos2(x, row_rect.bottom()),
            ],
            line_stroke,
        );
    }

    // 2) Draw the connector for *this* node (tee or elbow) if not a root.
    let node_center_y = row_rect.center().y;
    if depth > 0 {
        let trunk_x = row_rect.left() + (depth as f32 - 0.5) * INDENT_PER_LEVEL;
        // Vertical part of the connector: from top of row down to node center
        // (full row for tee, half row for elbow).
        let trunk_top = row_rect.top();
        let trunk_bottom = if is_last_sibling {
            node_center_y
        } else {
            row_rect.bottom()
        };
        painter.line_segment(
            [
                egui::pos2(trunk_x, trunk_top),
                egui::pos2(trunk_x, trunk_bottom),
            ],
            line_stroke,
        );
        // Horizontal arm: from trunk over to where the node sits.
        let node_x = row_rect.left() + total_indent - INDENT_PER_LEVEL * 0.5;
        painter.line_segment(
            [
                egui::pos2(trunk_x, node_center_y),
                egui::pos2(node_x, node_center_y),
            ],
            line_stroke,
        );
    }

    // 3) Draw the node disc — filled for current snapshot, hollow otherwise.
    let node_center = egui::pos2(
        row_rect.left() + total_indent - INDENT_PER_LEVEL * 0.5,
        node_center_y,
    );
    if snap.is_current {
        painter.circle_filled(node_center, NODE_RADIUS, AppColors::RUNNING);
    } else {
        painter.circle_stroke(
            node_center,
            NODE_RADIUS,
            egui::Stroke::new(1.5, AppColors::TEXT_DIM),
        );
    }

    // 4) Lay out the snapshot card to the right of the node.
    let card_left = node_center.x + NODE_RADIUS + 8.0;
    let card_rect = egui::Rect::from_min_max(
        egui::pos2(card_left, row_rect.top() + 2.0),
        egui::pos2(row_rect.right() - 4.0, row_rect.bottom() - 2.0),
    );

    let is_selected = app
        .selected_snapshot()
        .map(|s| s == snap.name)
        .unwrap_or(false);

    // Background highlight for selection / hover.
    let bg = if is_selected {
        AppColors::CARD_SELECTED_BG
    } else if row_response.hovered() {
        AppColors::BG_CARD.linear_multiply(1.15)
    } else {
        AppColors::BG_CARD
    };
    let stroke = if snap.is_current {
        egui::Stroke::new(1.5, AppColors::RUNNING)
    } else if is_selected {
        egui::Stroke::new(1.0, AppColors::PRIMARY)
    } else {
        egui::Stroke::new(0.5, AppColors::STROKE_SUBTLE)
    };
    painter.rect_filled(card_rect, ThemeRounding::BUTTON, bg);
    painter.rect_stroke(card_rect, ThemeRounding::BUTTON, stroke);

    // 5) Card content — render with a child UI bounded by the card rect.
    let mut content_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(card_rect.shrink2(egui::vec2(10.0, 6.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    render_card_content(&mut content_ui, snap, is_selected, app);

    // 6) Row click → select; right-click → context menu.
    if row_response.clicked() {
        app.set_selected_snapshot(Some(snap.name.clone()));
    }
    row_response.context_menu(|ui| {
        ui.label(
            egui::RichText::new(&snap.name)
                .size(FontSize::SMALL)
                .strong()
                .color(AppColors::TEXT),
        );
        ui.separator();
        if ui.button(t!("snap.revert")).clicked() {
            if let Some(vm_name) = app.selected_vm() {
                let vm = vm_name.to_string();
                app.request_confirm_snapshot_op(vm, snap.name.clone(), SnapshotOp::Revert);
            }
            ui.close_menu();
        }
        if ui.button(t!("snap.edit-description")).clicked() {
            app.action_prefill_snapshot_edit(&snap.name);
            ui.close_menu();
        }
        if ui
            .button(egui::RichText::new(t!("snap.delete")).color(AppColors::DANGER))
            .clicked()
        {
            if let Some(vm_name) = app.selected_vm() {
                let vm = vm_name.to_string();
                app.request_confirm_snapshot_op(vm, snap.name.clone(), SnapshotOp::Delete);
            }
            ui.close_menu();
        }
    });

    ui.add_space(Spacing::XS);
}

/// Inner content of the snapshot card — name + metadata on the left, action
/// buttons on the right (only for the selected row).
fn render_card_content(
    ui: &mut egui::Ui,
    snap: &SnapshotInfo,
    is_selected: bool,
    app: &mut LibreVmmApp,
) {
    // Left column: name, current badge, description, timestamp.
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(&snap.name)
                    .size(FontSize::SUBHEADING)
                    .strong()
                    .color(AppColors::TEXT),
            );
            if snap.is_current {
                ui.label(
                    egui::RichText::new(t!("snap.current"))
                        .size(FontSize::TINY)
                        .strong()
                        .color(AppColors::RUNNING),
                );
            }
            if !snap.state.is_empty() {
                ui.label(
                    egui::RichText::new(format!("({})", snap.state))
                        .size(FontSize::SMALL)
                        .color(AppColors::MUTED),
                );
            }
        });

        ui.horizontal(|ui| {
            if !snap.description.is_empty() {
                ui.label(
                    egui::RichText::new(&snap.description)
                        .size(FontSize::LABEL)
                        .color(AppColors::TEXT_DIM),
                );
            }
        });
    });

    // Right column: timestamp + (when selected) action buttons.
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if is_selected {
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(t!("snap.delete"))
                            .size(FontSize::LABEL)
                            .color(AppColors::DANGER),
                    )
                    .rounding(ThemeRounding::BUTTON_SMALL),
                )
                .clicked()
            {
                if let Some(vm_name) = app.selected_vm() {
                    let vm = vm_name.to_string();
                    app.request_confirm_snapshot_op(vm, snap.name.clone(), SnapshotOp::Delete);
                }
            }
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(t!("snap.edit-description"))
                            .size(FontSize::LABEL)
                            .color(AppColors::TEXT),
                    )
                    .rounding(ThemeRounding::BUTTON_SMALL),
                )
                .clicked()
            {
                app.action_prefill_snapshot_edit(&snap.name);
            }
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(t!("snap.revert"))
                            .size(FontSize::LABEL)
                            .color(egui::Color32::WHITE),
                    )
                    .fill(AppColors::PRIMARY)
                    .rounding(ThemeRounding::BUTTON_SMALL),
                )
                .clicked()
            {
                if let Some(vm_name) = app.selected_vm() {
                    let vm = vm_name.to_string();
                    app.request_confirm_snapshot_op(vm, snap.name.clone(), SnapshotOp::Revert);
                }
            }
            ui.add_space(Spacing::SM);
        }

        // Timestamp (dimmed) — always on the far right.
        if snap.creation_time > 0 {
            if let Some(dt) = chrono::DateTime::from_timestamp(snap.creation_time, 0) {
                let local = dt.with_timezone(&chrono::Local);
                ui.label(
                    egui::RichText::new(local.format("%Y-%m-%d %H:%M").to_string())
                        .size(FontSize::SMALL)
                        .color(AppColors::MUTED),
                );
            }
        }
    });
}
