mod app;
pub mod i18n;
mod spice;
mod theme;
mod views;
mod vnc;

use eframe::egui;
use tracing_subscriber::EnvFilter;

rust_i18n::i18n!("locales", fallback = "en");

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("vmm=info".parse().unwrap()))
        .init();

    // Initialize locale from saved preference or system default
    i18n::init_locale();

    // Allow overriding the UI scale via environment variable:
    //   LIBRE_VMM_SCALE=1.5 ./vmm-gui    (150% zoom)
    //   LIBRE_VMM_SCALE=0.75 ./vmm-gui   (75% zoom)
    // Or via command-line flag:
    //   ./vmm-gui --scale 1.25
    let scale_override = parse_scale_override();
    if let Some(scale) = scale_override {
        eprintln!("[vmm-gui] Using UI scale override: {:.2}x", scale);
    }

    let title = rust_i18n::t!("app.title");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title.as_ref())
            .with_inner_size([1200.0, 750.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "libre-vmm",
        options,
        Box::new(move |cc| {
            // Apply scale override from CLI/env before app initialization
            if let Some(scale) = scale_override {
                cc.egui_ctx.set_zoom_factor(scale);
            }
            Ok(Box::new(app::LibreVmmApp::new(cc)))
        }),
    )
}

/// Check for scale override from environment variable or CLI flag.
fn parse_scale_override() -> Option<f32> {
    // Check environment variable first
    if let Ok(val) = std::env::var("LIBRE_VMM_SCALE") {
        if let Ok(scale) = val.parse::<f32>() {
            if (0.25..=4.0).contains(&scale) {
                return Some(scale);
            }
        }
    }

    // Check CLI args: --scale <value>
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--scale" {
            if let Some(val) = args.get(i + 1) {
                if let Ok(scale) = val.parse::<f32>() {
                    if (0.25..=4.0).contains(&scale) {
                        return Some(scale);
                    }
                }
            }
        }
    }

    None
}
