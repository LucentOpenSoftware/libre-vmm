//! Resource Limits editor — CPU pinning, memory tuning, I/O throttle, network bandwidth.
//! Renders as a section in VM Settings or as a standalone floating panel.

use crate::app::LibreVmmApp;
use crate::theme::{AppColors, FontSize, Spacing, GRID_SPACING};
use eframe::egui;
use rust_i18n::t;
use vmm_core::resource_limits::{CpuPin, ResourceLimits};

/// Render the resource limits section inside the VM Settings editor.
/// Operates on the editing_config's resource_limits field.
pub fn render_section(app: &mut LibreVmmApp, ui: &mut egui::Ui) {
    // Extract values we need before borrowing mutably
    let (vcpu_count, memory_mib, _current_profile) = {
        let config = match app.editing_config() {
            Some(c) => c,
            None => return,
        };
        (
            config.vcpus,
            config.memory_mib,
            config.performance_profile.clone(),
        )
    };

    ui.label(
        egui::RichText::new(t!("reslim.heading"))
            .size(FontSize::HEADING)
            .strong()
            .color(AppColors::TEXT),
    );
    ui.label(
        egui::RichText::new(t!("reslim.subheading"))
            .color(AppColors::TEXT_DIM)
            .size(FontSize::LABEL),
    );
    ui.add_space(Spacing::SM);

    // ===== Performance Profile =====
    ui.label(egui::RichText::new(t!("reslim.profile")).strong());
    ui.horizontal(|ui| {
        let profiles = ["default", "gaming", "development", "server", "low-power"];
        let config = match app.editing_config_mut() {
            Some(c) => c,
            None => return,
        };
        egui::ComboBox::from_id_salt("perf_profile")
            .selected_text(&config.performance_profile)
            .width(150.0)
            .show_ui(ui, |ui| {
                for p in &profiles {
                    if ui
                        .selectable_value(&mut config.performance_profile, p.to_string(), *p)
                        .clicked()
                    {
                        apply_profile(&mut config.resource_limits, p, vcpu_count, memory_mib);
                    }
                }
            });
        ui.label(
            egui::RichText::new(profile_description(&config.performance_profile))
                .color(AppColors::TEXT_DIM)
                .size(FontSize::LABEL),
        );
    });

    ui.add_space(Spacing::MD);

    // Check states for collapsing headers (immutable borrow)
    let (cpu_open, mem_open, disk_open, net_open) = {
        let config = match app.editing_config() {
            Some(c) => c,
            None => return,
        };
        (
            config.resource_limits.cpu.has_any(),
            config.resource_limits.memory.has_any(),
            config.resource_limits.disk_io.has_any(),
            config.resource_limits.network.has_any(),
        )
    };

    // ===== CPU Limits =====
    egui::CollapsingHeader::new(egui::RichText::new(t!("reslim.cpu-tuning")).strong())
        .default_open(cpu_open)
        .show(ui, |ui| {
            let config = match app.editing_config_mut() {
                Some(c) => c,
                None => return,
            };
            render_cpu_limits(ui, &mut config.resource_limits, vcpu_count);
        });

    ui.add_space(Spacing::XS);

    // ===== Memory Limits =====
    egui::CollapsingHeader::new(egui::RichText::new(t!("reslim.memory-tuning")).strong())
        .default_open(mem_open)
        .show(ui, |ui| {
            let config = match app.editing_config_mut() {
                Some(c) => c,
                None => return,
            };
            render_memory_limits(ui, &mut config.resource_limits, memory_mib);
        });

    ui.add_space(Spacing::XS);

    // ===== Disk I/O Limits =====
    egui::CollapsingHeader::new(egui::RichText::new(t!("reslim.disk-io-throttle")).strong())
        .default_open(disk_open)
        .show(ui, |ui| {
            let config = match app.editing_config_mut() {
                Some(c) => c,
                None => return,
            };
            render_disk_io_limits(ui, &mut config.resource_limits);
        });

    ui.add_space(Spacing::XS);

    // ===== Network Bandwidth =====
    egui::CollapsingHeader::new(egui::RichText::new(t!("reslim.network-bandwidth")).strong())
        .default_open(net_open)
        .show(ui, |ui| {
            let config = match app.editing_config_mut() {
                Some(c) => c,
                None => return,
            };
            render_network_limits(ui, &mut config.resource_limits);
        });

    ui.add_space(Spacing::SM);

    // Summary
    let summary = {
        let config = match app.editing_config() {
            Some(c) => c,
            None => return,
        };
        config.resource_limits.summary()
    };
    if !summary.is_empty() {
        ui.separator();
        ui.label(
            egui::RichText::new(t!("reslim.active-limits"))
                .strong()
                .size(FontSize::LABEL),
        );
        for s in &summary {
            ui.label(
                egui::RichText::new(format!("  • {}", s))
                    .size(FontSize::LABEL)
                    .color(AppColors::TEXT_DIM),
            );
        }
    }
}

fn render_cpu_limits(ui: &mut egui::Ui, limits: &mut ResourceLimits, vcpu_count: u32) {
    egui::Grid::new("cpu_limits")
        .num_columns(2)
        .spacing(GRID_SPACING)
        .show(ui, |ui| {
            // CPU Shares
            ui.label(t!("reslim.cpu-shares"));
            let mut has_shares = limits.cpu.shares.is_some();
            ui.horizontal(|ui| {
                ui.checkbox(&mut has_shares, "");
                if has_shares {
                    let mut shares = limits.cpu.shares.unwrap_or(1024) as f32;
                    ui.add(egui::Slider::new(&mut shares, 64.0..=8192.0).text(t!("reslim.weight")));
                    limits.cpu.shares = Some(shares as u64);
                } else {
                    limits.cpu.shares = None;
                    ui.colored_label(AppColors::TEXT_DIM, t!("reslim.default-1024"));
                }
            });
            ui.end_row();

            // CPU Quota (percentage)
            ui.label(t!("reslim.cpu-limit"));
            let mut has_quota = limits.cpu.quota.is_some();
            ui.horizontal(|ui| {
                ui.checkbox(&mut has_quota, "");
                if has_quota {
                    let period = limits.cpu.period.unwrap_or(100_000) as f64;
                    let mut pct = limits
                        .cpu
                        .quota
                        .map(|q| {
                            if q > 0 {
                                (q as f64 / period) * 100.0
                            } else {
                                100.0
                            }
                        })
                        .unwrap_or(100.0) as f32;
                    ui.add(
                        egui::Slider::new(&mut pct, 1.0..=100.0 * vcpu_count as f32).suffix("%"),
                    );
                    limits.cpu.period = Some(100_000);
                    limits.cpu.quota = Some(((pct as f64 / 100.0) * 100_000.0) as i64);
                } else {
                    limits.cpu.quota = None;
                    limits.cpu.period = None;
                    ui.colored_label(AppColors::TEXT_DIM, t!("reslim.unlimited"));
                }
            });
            ui.end_row();

            // CPU Pinning
            ui.label(t!("reslim.cpu-pinning"));
            let host_cpus = vmm_core::resource_limits::host_cpu_count();
            ui.vertical(|ui| {
                if limits.cpu.pinning.is_empty() {
                    ui.colored_label(
                        AppColors::TEXT_DIM,
                        t!("reslim.none-host-cpus", n = host_cpus),
                    );
                    if ui.small_button(t!("reslim.add-pin")).clicked() {
                        limits.cpu.pinning.push(CpuPin {
                            vcpu: 0,
                            cpuset: vmm_core::resource_limits::all_cpus_set(),
                        });
                    }
                } else {
                    let mut to_remove = None;
                    for (i, pin) in limits.cpu.pinning.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(t!("reslim.vcpu-n", n = pin.vcpu));
                            ui.add(
                                egui::TextEdit::singleline(&mut pin.cpuset)
                                    .desired_width(80.0)
                                    .hint_text("0-3"),
                            );
                            if ui.small_button("✕").clicked() {
                                to_remove = Some(i);
                            }
                        });
                    }
                    if let Some(idx) = to_remove {
                        limits.cpu.pinning.remove(idx);
                    }
                    if limits.cpu.pinning.len() < vcpu_count as usize {
                        if ui.small_button(t!("reslim.add-pin-plus")).clicked() {
                            let next_vcpu = limits.cpu.pinning.len() as u32;
                            limits.cpu.pinning.push(CpuPin {
                                vcpu: next_vcpu,
                                cpuset: vmm_core::resource_limits::all_cpus_set(),
                            });
                        }
                    }
                }
            });
            ui.end_row();
        });
}

fn render_memory_limits(ui: &mut egui::Ui, limits: &mut ResourceLimits, memory_mib: u64) {
    let max_kib = memory_mib * 1024 * 2; // Allow up to 2x the configured memory

    egui::Grid::new("mem_limits")
        .num_columns(2)
        .spacing(GRID_SPACING)
        .show(ui, |ui| {
            // Hard limit
            ui.label(t!("reslim.hard-limit"));
            let mut has_hard = limits.memory.hard_limit_kib.is_some();
            ui.horizontal(|ui| {
                ui.checkbox(&mut has_hard, "");
                if has_hard {
                    let mut val =
                        limits.memory.hard_limit_kib.unwrap_or(memory_mib * 1024) as f32 / 1024.0;
                    ui.add(
                        egui::Slider::new(&mut val, 128.0..=(max_kib as f32 / 1024.0))
                            .suffix(" MiB"),
                    );
                    limits.memory.hard_limit_kib = Some((val * 1024.0) as u64);
                } else {
                    limits.memory.hard_limit_kib = None;
                    ui.colored_label(AppColors::TEXT_DIM, t!("reslim.unlimited"));
                }
            });
            ui.end_row();

            // Soft limit
            ui.label(t!("reslim.soft-limit"));
            let mut has_soft = limits.memory.soft_limit_kib.is_some();
            ui.horizontal(|ui| {
                ui.checkbox(&mut has_soft, "");
                if has_soft {
                    let mut val =
                        limits.memory.soft_limit_kib.unwrap_or(memory_mib * 1024) as f32 / 1024.0;
                    ui.add(
                        egui::Slider::new(&mut val, 128.0..=(max_kib as f32 / 1024.0))
                            .suffix(" MiB"),
                    );
                    limits.memory.soft_limit_kib = Some((val * 1024.0) as u64);
                } else {
                    limits.memory.soft_limit_kib = None;
                    ui.colored_label(AppColors::TEXT_DIM, t!("reslim.none"));
                }
            });
            ui.end_row();

            // Min guarantee (balloon floor)
            ui.label(t!("reslim.min-guarantee"));
            let mut has_min = limits.memory.min_guarantee_kib.is_some();
            ui.horizontal(|ui| {
                ui.checkbox(&mut has_min, "");
                if has_min {
                    let mut val =
                        limits.memory.min_guarantee_kib.unwrap_or(memory_mib * 512) as f32 / 1024.0;
                    ui.add(egui::Slider::new(&mut val, 64.0..=(memory_mib as f32)).suffix(" MiB"));
                    limits.memory.min_guarantee_kib = Some((val * 1024.0) as u64);
                } else {
                    limits.memory.min_guarantee_kib = None;
                    ui.colored_label(AppColors::TEXT_DIM, t!("reslim.none"));
                }
            });
            ui.end_row();
        });
}

fn render_disk_io_limits(ui: &mut egui::Ui, limits: &mut ResourceLimits) {
    egui::Grid::new("disk_io_limits")
        .num_columns(2)
        .spacing(GRID_SPACING)
        .show(ui, |ui| {
            // Total throughput
            ui.label(t!("reslim.throughput-limit"));
            let mut has_total = limits.disk_io.total_bytes_sec.is_some();
            ui.horizontal(|ui| {
                ui.checkbox(&mut has_total, "");
                if has_total {
                    let mut val =
                        limits.disk_io.total_bytes_sec.unwrap_or(100_000_000) as f32 / 1_048_576.0;
                    ui.add(egui::Slider::new(&mut val, 1.0..=2000.0).suffix(" MB/s"));
                    limits.disk_io.total_bytes_sec = Some((val * 1_048_576.0) as u64);
                } else {
                    limits.disk_io.total_bytes_sec = None;
                    ui.colored_label(AppColors::TEXT_DIM, t!("reslim.unlimited"));
                }
            });
            ui.end_row();

            // Total IOPS
            ui.label(t!("reslim.iops-limit"));
            let mut has_iops = limits.disk_io.total_iops_sec.is_some();
            ui.horizontal(|ui| {
                ui.checkbox(&mut has_iops, "");
                if has_iops {
                    let mut val = limits.disk_io.total_iops_sec.unwrap_or(5000) as f32;
                    ui.add(
                        egui::Slider::new(&mut val, 100.0..=100000.0)
                            .suffix(" IOPS")
                            .logarithmic(true),
                    );
                    limits.disk_io.total_iops_sec = Some(val as u64);
                } else {
                    limits.disk_io.total_iops_sec = None;
                    ui.colored_label(AppColors::TEXT_DIM, t!("reslim.unlimited"));
                }
            });
            ui.end_row();
        });
}

fn render_network_limits(ui: &mut egui::Ui, limits: &mut ResourceLimits) {
    egui::Grid::new("net_limits")
        .num_columns(2)
        .spacing(GRID_SPACING)
        .show(ui, |ui| {
            // Inbound
            ui.label(t!("reslim.inbound-limit"));
            let mut has_in = limits.network.inbound_average_kbps.is_some();
            ui.horizontal(|ui| {
                ui.checkbox(&mut has_in, "");
                if has_in {
                    let mut val = limits.network.inbound_average_kbps.unwrap_or(10240) as f32;
                    ui.add(
                        egui::Slider::new(&mut val, 64.0..=1048576.0)
                            .suffix(" KB/s")
                            .logarithmic(true),
                    );
                    limits.network.inbound_average_kbps = Some(val as u64);
                    limits.network.inbound_peak_kbps = Some((val * 1.5) as u64);
                    limits.network.inbound_burst_kb = Some((val * 0.1) as u64);
                } else {
                    limits.network.inbound_average_kbps = None;
                    limits.network.inbound_peak_kbps = None;
                    limits.network.inbound_burst_kb = None;
                    ui.colored_label(AppColors::TEXT_DIM, t!("reslim.unlimited"));
                }
            });
            ui.end_row();

            // Outbound
            ui.label(t!("reslim.outbound-limit"));
            let mut has_out = limits.network.outbound_average_kbps.is_some();
            ui.horizontal(|ui| {
                ui.checkbox(&mut has_out, "");
                if has_out {
                    let mut val = limits.network.outbound_average_kbps.unwrap_or(10240) as f32;
                    ui.add(
                        egui::Slider::new(&mut val, 64.0..=1048576.0)
                            .suffix(" KB/s")
                            .logarithmic(true),
                    );
                    limits.network.outbound_average_kbps = Some(val as u64);
                    limits.network.outbound_peak_kbps = Some((val * 1.5) as u64);
                    limits.network.outbound_burst_kb = Some((val * 0.1) as u64);
                } else {
                    limits.network.outbound_average_kbps = None;
                    limits.network.outbound_peak_kbps = None;
                    limits.network.outbound_burst_kb = None;
                    ui.colored_label(AppColors::TEXT_DIM, t!("reslim.unlimited"));
                }
            });
            ui.end_row();
        });
}

/// Apply a performance profile preset to resource limits.
fn apply_profile(limits: &mut ResourceLimits, profile: &str, _vcpu_count: u32, memory_mib: u64) {
    // Reset all limits first
    *limits = ResourceLimits::default();

    match profile {
        "gaming" => {
            // Max performance: high CPU shares, no throttling
            limits.cpu.shares = Some(2048);
            // Ensure minimum memory
            limits.memory.min_guarantee_kib = Some(memory_mib * 1024);
        },
        "development" => {
            // Balanced: moderate shares, slight memory flexibility
            limits.cpu.shares = Some(1024);
        },
        "server" => {
            // Predictable: guaranteed memory
            limits.memory.min_guarantee_kib = Some(memory_mib * 768); // 75% guaranteed
            limits.memory.hard_limit_kib = Some(memory_mib * 1024 * 2); // 2x cap
        },
        "low-power" => {
            // Battery saving: limit everything
            limits.cpu.shares = Some(256);
            limits.cpu.period = Some(100_000);
            limits.cpu.quota = Some(50_000); // 50% max CPU
            limits.memory.soft_limit_kib = Some(memory_mib * 768); // 75% soft limit
            limits.disk_io.total_bytes_sec = Some(50_000_000); // 50 MB/s
            limits.network.inbound_average_kbps = Some(5120); // 5 MB/s
            limits.network.outbound_average_kbps = Some(5120);
        },
        _ => {
            // "default" — no limits
        },
    }
}

fn profile_description(profile: &str) -> std::borrow::Cow<'static, str> {
    match profile {
        "gaming" => t!("reslim.profile-gaming"),
        "development" => t!("reslim.profile-development"),
        "server" => t!("reslim.profile-server"),
        "low-power" => t!("reslim.profile-low-power"),
        _ => t!("reslim.profile-default"),
    }
}
