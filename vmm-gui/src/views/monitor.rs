//! Performance monitoring view — real-time CPU, memory, disk I/O, and network charts.
//!
//! Renders as the "Performance" tab content. Uses egui's built-in Plot widget
//! to draw time-series line charts.

use crate::app::LibreVmmApp;
use crate::theme::{AppColors, FontSize, Spacing, ThemeRounding};
use eframe::egui;
use rust_i18n::t;
use std::fmt::Write;
use vmm_core::monitor::PerfSample;

/// Which performance metric is currently selected for the large chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfMetric {
    Cpu,
    Memory,
    DiskIo,
    NetworkIo,
}

/// State for the performance monitor view.
pub struct MonitorState {
    pub active_metric: PerfMetric,
    pub poll_interval: std::time::Duration,
    pub last_poll: std::time::Instant,
    /// Reusable buffer for chart point calculations (avoids per-frame allocation).
    pub chart_points: Vec<egui::Pos2>,
    /// Second reusable buffer for dual-line charts (disk/network I/O).
    pub chart_points2: Vec<egui::Pos2>,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            active_metric: PerfMetric::Cpu,
            poll_interval: std::time::Duration::from_secs(2),
            last_poll: std::time::Instant::now() - std::time::Duration::from_secs(10),
            chart_points: Vec::with_capacity(128),
            chart_points2: Vec::with_capacity(128),
        }
    }
}

pub fn render(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let vm_name = match app.selected_vm() {
        Some(name) => name.to_string(),
        None => return,
    };

    // Auto-poll
    let should_poll = app.monitor_state().last_poll.elapsed() >= app.monitor_state().poll_interval;
    if should_poll {
        app.poll_monitor(&vm_name);
    }

    // Top: metric selector tabs + summary badges
    ui.horizontal(|ui| {
        let active = app.monitor_state().active_metric;

        if metric_tab(ui, "CPU", active == PerfMetric::Cpu) {
            app.monitor_state_mut().active_metric = PerfMetric::Cpu;
        }
        if metric_tab(ui, "Memory", active == PerfMetric::Memory) {
            app.monitor_state_mut().active_metric = PerfMetric::Memory;
        }
        if metric_tab(ui, "Disk I/O", active == PerfMetric::DiskIo) {
            app.monitor_state_mut().active_metric = PerfMetric::DiskIo;
        }
        if metric_tab(ui, "Network", active == PerfMetric::NetworkIo) {
            app.monitor_state_mut().active_metric = PerfMetric::NetworkIo;
        }

        // Read values before entering the layout closure to avoid borrow conflicts.
        let badge_data = app
            .vm_monitor()
            .map(|monitor| (monitor.latest_cpu(), monitor.latest_memory_mib()));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some((cpu, mem)) = badge_data {
                ui.label(
                    egui::RichText::new(format!("MEM: {} MiB", mem))
                        .size(FontSize::TINY)
                        .color(AppColors::TEXT_DIM),
                );
                ui.label(
                    egui::RichText::new(format!("CPU: {:.1}%", cpu))
                        .size(FontSize::TINY)
                        .color(if cpu > 80.0 {
                            AppColors::DANGER
                        } else {
                            AppColors::TEXT_DIM
                        }),
                );
            }
        });
    });

    ui.separator();
    ui.add_space(Spacing::XS);

    // Borrow the samples slice directly from VecDeque — avoids cloning the entire Vec each frame.
    // VecDeque::make_contiguous() is a no-op when already contiguous (common case).
    let has_samples = app.vm_monitor().map_or(false, |m| !m.samples.is_empty());

    if !has_samples {
        ui.add_space(2.0 * Spacing::XL);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(t!("monitor.collecting").as_ref())
                    .size(FontSize::SUBHEADING)
                    .color(AppColors::TEXT_DIM),
            );
            ui.add_space(Spacing::SM);
            ui.spinner();
        });
        return;
    }

    let active_metric = app.monitor_state().active_metric; // Copy, no clone needed

    // Take reusable point buffers out of MonitorState to avoid borrow conflicts.
    let mut pts1 = std::mem::take(&mut app.monitor_state_mut().chart_points);
    let mut pts2 = std::mem::take(&mut app.monitor_state_mut().chart_points2);

    // Make the deque contiguous so we can get a single &[PerfSample] slice.
    if let Some(monitor) = app.vm_monitor_mut() {
        monitor.samples.make_contiguous();
    }

    // Scope the immutable borrow of samples so it ends before render_balloon_section
    // needs &mut app.
    if let Some(monitor) = app.vm_monitor() {
        let samples = monitor.samples.as_slices().0;

        // Main chart area
        match active_metric {
            PerfMetric::Cpu => render_cpu_chart(ui, samples, &mut pts1),
            PerfMetric::Memory => render_memory_chart(ui, samples, &mut pts1),
            PerfMetric::DiskIo => render_io_chart(ui, samples, true, &mut pts1, &mut pts2),
            PerfMetric::NetworkIo => render_io_chart(ui, samples, false, &mut pts1, &mut pts2),
        }

        ui.add_space(Spacing::SM);
    }

    // Put buffers back.
    app.monitor_state_mut().chart_points = pts1;
    app.monitor_state_mut().chart_points2 = pts2;

    // Memory Ballooning section (needs &mut app)
    render_balloon_section(app, ui);

    ui.add_space(Spacing::SM);

    // Re-borrow for the stats grid (immutable, cheap — no copy needed)
    if let Some(monitor) = app.vm_monitor() {
        let samples = monitor.samples.as_slices().0;
        render_stats_grid(ui, samples);
    }
}

fn render_balloon_section(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    let stats = app.balloon_stats().clone();

    egui::CollapsingHeader::new(
        egui::RichText::new(t!("monitor.memory-ballooning").as_ref())
            .size(FontSize::LABEL)
            .strong(),
    )
    .default_open(false)
    .show(ui, |ui| {
        if let Some(ref stats) = stats {
            if !stats.driver_available {
                ui.label(
                    egui::RichText::new(t!("monitor.balloon-unavailable").as_ref())
                        .color(AppColors::TEXT_DIM)
                        .size(FontSize::SMALL),
                );
                return;
            }

            // Current / Max display
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "Current: {} MiB / {} MiB",
                        stats.current_mib, stats.maximum_mib
                    ))
                    .size(FontSize::LABEL),
                );

                if stats.available_mib > 0 {
                    ui.label(
                        egui::RichText::new(format!("  (Available: {} MiB)", stats.available_mib))
                            .size(FontSize::SMALL)
                            .color(AppColors::TEXT_DIM),
                    );
                }
            });

            // Progress bar
            let ratio = if stats.maximum_mib > 0 {
                stats.current_mib as f32 / stats.maximum_mib as f32
            } else {
                0.0
            };
            let bar = egui::ProgressBar::new(ratio).text(format!("{}%", (ratio * 100.0) as u32));
            ui.add(bar);

            // Slider to adjust
            let mut target = app.balloon_target_mib() as i32;
            let max = stats.maximum_mib.max(256) as i32;
            ui.horizontal(|ui| {
                ui.label(t!("monitor.target").as_ref());
                if ui
                    .add(
                        egui::Slider::new(&mut target, 128..=max)
                            .text("MiB")
                            .step_by(128.0),
                    )
                    .changed()
                {
                    app.set_balloon_target_mib(target as u64);
                }
                if ui.button(t!("monitor.apply").as_ref()).clicked() {
                    app.action_set_balloon_memory();
                }
            });

            // Swap info
            if stats.swap_in_bytes > 0 || stats.swap_out_bytes > 0 {
                ui.label(
                    egui::RichText::new(format!(
                        "Swap: in {} / out {}",
                        format_bytes(stats.swap_in_bytes),
                        format_bytes(stats.swap_out_bytes),
                    ))
                    .size(FontSize::TINY)
                    .color(AppColors::TEXT_DIM),
                );
            }
        } else {
            ui.label(
                egui::RichText::new(t!("monitor.no-balloon-data").as_ref())
                    .color(AppColors::TEXT_DIM)
                    .size(FontSize::SMALL),
            );
            if ui.small_button(t!("monitor.refresh").as_ref()).clicked() {
                if let Some(name) = app.selected_vm().map(|s| s.to_string()) {
                    app.action_refresh_balloon(&name);
                }
            }
        }
    });
}

fn metric_tab(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    let fill = if active {
        AppColors::PRIMARY
    } else {
        egui::Color32::TRANSPARENT
    };
    let text_color = if active {
        egui::Color32::WHITE
    } else {
        AppColors::TEXT_DIM
    };

    let btn = egui::Button::new(
        egui::RichText::new(label)
            .size(FontSize::SMALL)
            .color(text_color),
    )
    .fill(fill)
    .rounding(ThemeRounding::BUTTON_SMALL)
    .min_size(egui::vec2(70.0, 24.0));

    ui.add(btn).clicked()
}

fn render_cpu_chart(ui: &mut egui::Ui, samples: &[PerfSample], points: &mut Vec<egui::Pos2>) {
    ui.label(
        egui::RichText::new(t!("monitor.cpu-usage").as_ref())
            .size(FontSize::BODY)
            .strong()
            .color(AppColors::TEXT),
    );
    ui.add_space(Spacing::XS);

    let chart_height = (ui.available_height() - 120.0).max(100.0);

    // Draw chart using egui's painter (no external crate dependency)
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), chart_height),
        egui::Sense::hover(),
    );

    let painter = ui.painter_at(rect);

    // Background
    painter.rect_filled(rect, 4.0, AppColors::BG_CARD);

    // Grid lines
    draw_grid(&painter, rect, 5, &["0%", "25%", "50%", "75%", "100%"]);

    // Data line
    if let (Some(first), Some(last)) = (samples.first(), samples.last()) {
        let t_min = first.time_secs;
        let t_max = last.time_secs;
        points.clear();
        for s in samples {
            let x = remap(
                s.time_secs,
                t_min,
                t_max,
                rect.left() + 30.0,
                rect.right() - 10.0,
            );
            let y = remap(
                s.cpu_percent,
                0.0,
                100.0,
                rect.bottom() - 20.0,
                rect.top() + 10.0,
            );
            points.push(egui::pos2(x, y));
        }

        draw_line_chart(&painter, points, AppColors::PRIMARY, rect);
    }

    // Latest value label
    if let Some(last) = samples.last() {
        let text = format!("{:.1}%", last.cpu_percent);
        painter.text(
            egui::pos2(rect.right() - 8.0, rect.top() + 12.0),
            egui::Align2::RIGHT_TOP,
            text,
            egui::FontId::proportional(FontSize::SUBHEADING),
            AppColors::PRIMARY,
        );
    }
}

fn render_memory_chart(ui: &mut egui::Ui, samples: &[PerfSample], points: &mut Vec<egui::Pos2>) {
    let total = samples.last().map(|s| s.memory_total_mib).unwrap_or(1);

    ui.label(
        egui::RichText::new(t!("monitor.memory-usage", total = total).as_ref())
            .size(FontSize::BODY)
            .strong()
            .color(AppColors::TEXT),
    );
    ui.add_space(Spacing::XS);

    let chart_height = (ui.available_height() - 120.0).max(100.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), chart_height),
        egui::Sense::hover(),
    );

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, AppColors::BG_CARD);

    let max_mem = total as f64;
    // Use a fixed array + write! to avoid allocating 5 Strings every frame.
    let mut label_bufs: [String; 5] = Default::default();
    for (i, buf) in label_bufs.iter_mut().enumerate() {
        buf.clear();
        let _ = write!(buf, "{} MiB", (max_mem * i as f64 / 4.0) as u64);
    }
    let label_refs: [&str; 5] = [
        &label_bufs[0],
        &label_bufs[1],
        &label_bufs[2],
        &label_bufs[3],
        &label_bufs[4],
    ];
    draw_grid(&painter, rect, 5, &label_refs);

    if let (Some(first), Some(last)) = (samples.first(), samples.last()) {
        let t_min = first.time_secs;
        let t_max = last.time_secs;
        points.clear();
        for s in samples {
            let x = remap(
                s.time_secs,
                t_min,
                t_max,
                rect.left() + 50.0,
                rect.right() - 10.0,
            );
            let y = remap(
                s.memory_used_mib as f64,
                0.0,
                max_mem,
                rect.bottom() - 20.0,
                rect.top() + 10.0,
            );
            points.push(egui::pos2(x, y));
        }

        draw_line_chart(&painter, points, AppColors::SUCCESS, rect);
    }

    if let Some(last) = samples.last() {
        let pct = if total > 0 {
            last.memory_used_mib as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        painter.text(
            egui::pos2(rect.right() - 8.0, rect.top() + 12.0),
            egui::Align2::RIGHT_TOP,
            format!("{} MiB ({:.0}%)", last.memory_used_mib, pct),
            egui::FontId::proportional(FontSize::SUBHEADING),
            AppColors::SUCCESS,
        );
    }
}

fn render_io_chart(
    ui: &mut egui::Ui,
    samples: &[PerfSample],
    is_disk: bool,
    read_points: &mut Vec<egui::Pos2>,
    write_points: &mut Vec<egui::Pos2>,
) {
    let title = if is_disk {
        t!("monitor.disk-io")
    } else {
        t!("monitor.network-io")
    };
    ui.label(
        egui::RichText::new(title.as_ref())
            .size(FontSize::BODY)
            .strong()
            .color(AppColors::TEXT),
    );
    ui.add_space(Spacing::XS);

    let chart_height = (ui.available_height() - 120.0).max(100.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), chart_height),
        egui::Sense::hover(),
    );

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, AppColors::BG_CARD);

    // Determine max for scaling
    let max_val = samples
        .iter()
        .map(|s| {
            if is_disk {
                s.disk_read_bytes.max(s.disk_write_bytes)
            } else {
                s.net_rx_bytes.max(s.net_tx_bytes)
            }
        })
        .max()
        .unwrap_or(1024)
        .max(1024) as f64;

    if let (Some(first), Some(last)) = (samples.first(), samples.last()) {
        let t_min = first.time_secs;
        let t_max = last.time_secs;

        // Read/RX line
        read_points.clear();
        for s in samples.iter() {
            let val = if is_disk {
                s.disk_read_bytes
            } else {
                s.net_rx_bytes
            };
            let x = remap(
                s.time_secs,
                t_min,
                t_max,
                rect.left() + 30.0,
                rect.right() - 10.0,
            );
            let y = remap(
                val as f64,
                0.0,
                max_val,
                rect.bottom() - 20.0,
                rect.top() + 10.0,
            );
            read_points.push(egui::pos2(x, y));
        }

        // Write/TX line
        write_points.clear();
        for s in samples.iter() {
            let val = if is_disk {
                s.disk_write_bytes
            } else {
                s.net_tx_bytes
            };
            let x = remap(
                s.time_secs,
                t_min,
                t_max,
                rect.left() + 30.0,
                rect.right() - 10.0,
            );
            let y = remap(
                val as f64,
                0.0,
                max_val,
                rect.bottom() - 20.0,
                rect.top() + 10.0,
            );
            write_points.push(egui::pos2(x, y));
        }

        draw_line_chart(&painter, read_points, AppColors::PRIMARY, rect);
        draw_line_chart(&painter, write_points, AppColors::DANGER, rect);

        // Legend
        let legend_y = rect.top() + 12.0;
        let legend_read = if is_disk {
            t!("monitor.legend-read")
        } else {
            t!("monitor.legend-rx")
        };
        let legend_write = if is_disk {
            t!("monitor.legend-write")
        } else {
            t!("monitor.legend-tx")
        };
        painter.text(
            egui::pos2(rect.left() + 34.0, legend_y),
            egui::Align2::LEFT_TOP,
            legend_read.as_ref(),
            egui::FontId::proportional(FontSize::TINY),
            AppColors::PRIMARY,
        );
        painter.text(
            egui::pos2(rect.left() + 90.0, legend_y),
            egui::Align2::LEFT_TOP,
            legend_write.as_ref(),
            egui::FontId::proportional(FontSize::TINY),
            AppColors::DANGER,
        );
    }
}

fn render_stats_grid(ui: &mut egui::Ui, samples: &[PerfSample]) {
    let last = match samples.last() {
        Some(s) => s,
        None => return,
    };
    let avg_cpu = if samples.is_empty() {
        0.0
    } else {
        samples.iter().map(|s| s.cpu_percent).sum::<f64>() / samples.len() as f64
    };
    let peak_cpu = samples.iter().map(|s| s.cpu_percent).fold(0.0f64, f64::max);

    ui.separator();
    ui.add_space(Spacing::XS);

    // Pre-format all stat values into a single reusable buffer to reduce per-frame allocations.
    let mut buf = String::with_capacity(128);

    egui::Grid::new("perf_stats_grid")
        .num_columns(4)
        .spacing([20.0, 4.0])
        .show(ui, |ui| {
            buf.clear();
            let _ = write!(buf, "{:.1}%", last.cpu_percent);
            stat_cell(ui, &t!("monitor.current-cpu"), &buf);
            buf.clear();
            let _ = write!(buf, "{:.1}%", avg_cpu);
            stat_cell(ui, &t!("monitor.avg-cpu"), &buf);
            buf.clear();
            let _ = write!(buf, "{:.1}%", peak_cpu);
            stat_cell(ui, &t!("monitor.peak-cpu"), &buf);
            buf.clear();
            let _ = write!(
                buf,
                "{} / {} MiB",
                last.memory_used_mib, last.memory_total_mib
            );
            stat_cell(ui, &t!("monitor.memory"), &buf);
            ui.end_row();

            stat_cell(
                ui,
                &t!("monitor.disk-read"),
                &format_bytes(last.disk_read_bytes),
            );
            stat_cell(
                ui,
                &t!("monitor.disk-write"),
                &format_bytes(last.disk_write_bytes),
            );
            stat_cell(ui, &t!("monitor.net-rx"), &format_bytes(last.net_rx_bytes));
            stat_cell(ui, &t!("monitor.net-tx"), &format_bytes(last.net_tx_bytes));
            ui.end_row();
        });
}

fn stat_cell(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(FontSize::TINY)
                .color(AppColors::TEXT_DIM),
        );
        ui.label(
            egui::RichText::new(value)
                .size(FontSize::LABEL)
                .strong()
                .color(AppColors::TEXT),
        );
    });
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// ===== Drawing helpers =====

fn remap(value: f64, in_min: f64, in_max: f64, out_min: f32, out_max: f32) -> f32 {
    let range = in_max - in_min;
    if range.abs() < 1e-10 {
        return (out_min + out_max) / 2.0;
    }
    out_min + ((value - in_min) / range) as f32 * (out_max - out_min)
}

fn draw_grid(painter: &egui::Painter, rect: egui::Rect, rows: usize, _labels: &[&str]) {
    let grid_color = egui::Color32::from_rgba_premultiplied(80, 80, 80, 40);
    for i in 0..rows {
        let y = rect.top() + 10.0 + (rect.height() - 30.0) * i as f32 / (rows - 1).max(1) as f32;
        painter.line_segment(
            [
                egui::pos2(rect.left() + 30.0, y),
                egui::pos2(rect.right() - 10.0, y),
            ],
            egui::Stroke::new(1.0, grid_color),
        );
    }
}

fn draw_line_chart(
    painter: &egui::Painter,
    points: &[egui::Pos2],
    color: egui::Color32,
    _rect: egui::Rect,
) {
    if points.len() < 2 {
        return;
    }

    // Draw the line
    for i in 1..points.len() {
        painter.line_segment([points[i - 1], points[i]], egui::Stroke::new(2.0, color));
    }

    // Draw dots at each point
    for point in points {
        painter.circle_filled(*point, 2.0, color);
    }
}
