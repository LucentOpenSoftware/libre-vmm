//! Background task panel — shows running/completed tasks with progress.
//! Appears as a floating panel or bottom section, similar to Proxmox's task log.

use crate::app::LibreVmmApp;
use crate::theme::AppColors;
use eframe::egui;
use rust_i18n::t;
use vmm_core::task::{TaskHandle, TaskStatus};

/// Render the task panel as a floating window.
pub fn render(app: &mut LibreVmmApp, ctx: &egui::Context) {
    if !app.show_task_panel() {
        return;
    }

    let mut open = true;

    egui::Window::new("Background Tasks")
        .open(&mut open)
        .default_width(500.0)
        .default_height(300.0)
        .resizable(true)
        .collapsible(true)
        .show(ctx, |ui| {
            render_task_list(app, ui);
        });

    if !open {
        app.set_show_task_panel(false);
    }
}

/// Render a compact task indicator in the status bar.
/// Shows a spinning icon + count when tasks are running.
pub fn render_status_indicator(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let active = app.task_manager().active_count();
    if active == 0 {
        return;
    }

    let text = format!("⏳ {} task{}", active, if active == 1 { "" } else { "s" });
    let resp = ui.selectable_label(false, egui::RichText::new(text).color(AppColors::PRIMARY));
    if resp.clicked() {
        app.set_show_task_panel(true);
    }
    if resp.hovered() {
        // Show summary tooltip
        resp.on_hover_ui(|ui| {
            for handle in app.task_manager().active_tasks() {
                let info = handle.info();
                ui.label(format!("• {}", info.description));
            }
        });
    }
}

fn render_task_list(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    // Header
    ui.horizontal(|ui| {
        let active = app.task_manager().active_count();
        let total = app.task_manager().tasks().len();
        ui.label(format!("{} active, {} total", active, total));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("Clear History")
                .on_hover_text(t!("tooltip.clear-task-history").to_string())
                .clicked()
            {
                app.task_manager_mut().clear_history();
            }
        });
    });

    ui.separator();

    // Task list
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let tasks: Vec<TaskHandle> = app.task_manager().tasks().to_vec();

            if tasks.is_empty() {
                ui.colored_label(AppColors::TEXT_DIM, "No background tasks");
                return;
            }

            for handle in tasks.iter().rev() {
                let info = handle.info();
                render_task_row(ui, &info, handle);
                ui.separator();
            }
        });
}

fn render_task_row(ui: &mut egui::Ui, info: &vmm_core::task::TaskInfo, handle: &TaskHandle) {
    ui.horizontal(|ui| {
        // Status icon
        let (icon, color) = match &info.status {
            TaskStatus::Pending => ("⏸", AppColors::TEXT_DIM),
            TaskStatus::Running => ("▶", AppColors::PRIMARY),
            TaskStatus::Completed => ("✓", AppColors::RUNNING),
            TaskStatus::Failed(_) => ("✗", AppColors::DANGER),
            TaskStatus::Cancelled => ("⊘", AppColors::TEXT_DIM),
        };
        ui.colored_label(color, icon);

        // Description + category badge
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(&info.description);
                ui.colored_label(
                    AppColors::TEXT_DIM,
                    egui::RichText::new(format!("[{}]", info.category)).small(),
                );
            });

            // Progress bar for running tasks
            match &info.status {
                TaskStatus::Running => {
                    if info.progress.fraction >= 0.0 {
                        let bar = egui::ProgressBar::new(info.progress.fraction as f32)
                            .text(&info.progress.message);
                        ui.add(bar);
                    } else {
                        // Indeterminate spinner
                        ui.horizontal(|ui| {
                            ui.spinner();
                            if !info.progress.message.is_empty() {
                                ui.label(&info.progress.message);
                            }
                        });
                    }
                },
                TaskStatus::Failed(err) => {
                    ui.colored_label(AppColors::DANGER, format!("Error: {}", err));
                },
                TaskStatus::Completed => {
                    let elapsed = info.created_at.elapsed();
                    ui.colored_label(
                        AppColors::TEXT_DIM,
                        format!("Completed in {:.1}s", elapsed.as_secs_f64()),
                    );
                },
                _ => {},
            }
        });

        // Cancel button for running tasks
        if info.status == TaskStatus::Running || info.status == TaskStatus::Pending {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Cancel").clicked() {
                    handle.request_cancel();
                }
            });
        }
    });
}
