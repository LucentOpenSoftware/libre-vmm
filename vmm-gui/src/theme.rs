//! Visual theme — clean, modern look inspired by professional VM managers.
//!
//! Each "Box" type has its own color accent, allowing the UI to visually
//! distinguish between Standard (blue), Hardware Lab (teal/green), and
//! Power User (amber/orange) modes.

use eframe::egui;
use egui::{Color32, FontFamily, FontId, Rounding, Stroke, TextStyle, Visuals};
use vmm_core::qemu_archs::BoxType;

/// Colors used throughout the app.
pub struct AppColors;

#[allow(dead_code)]
impl AppColors {
    // Default accent (Standard / Box 1) — Professional Blue
    pub const PRIMARY: Color32 = Color32::from_rgb(45, 120, 210);
    pub const PRIMARY_HOVER: Color32 = Color32::from_rgb(55, 140, 230);

    pub const SUCCESS: Color32 = Color32::from_rgb(46, 160, 67);
    pub const WARNING: Color32 = Color32::from_rgb(210, 153, 34);
    pub const DANGER: Color32 = Color32::from_rgb(207, 34, 46);
    pub const MUTED: Color32 = Color32::from_rgb(130, 140, 155);

    pub const BG_DARK: Color32 = Color32::from_rgb(30, 33, 40);
    pub const BG_PANEL: Color32 = Color32::from_rgb(38, 42, 52);
    pub const BG_CARD: Color32 = Color32::from_rgb(46, 51, 63);
    pub const BG_HOVER: Color32 = Color32::from_rgb(55, 60, 75);

    pub const TEXT: Color32 = Color32::from_rgb(220, 225, 235);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(150, 158, 172);

    pub const RUNNING: Color32 = Color32::from_rgb(46, 160, 67);
    pub const PAUSED: Color32 = Color32::from_rgb(210, 153, 34);
    pub const OFF: Color32 = Color32::from_rgb(130, 140, 155);
    pub const CRASHED: Color32 = Color32::from_rgb(207, 34, 46);

    pub const STROKE_SUBTLE: Color32 = Color32::from_rgb(55, 60, 75);
    pub const CONSOLE_BG: Color32 = Color32::from_rgb(15, 15, 15);
    pub const STAR_COLOR: Color32 = Color32::from_rgb(255, 200, 50);
    pub const CARD_SELECTED_BG: Color32 = Color32::from_rgb(35, 50, 45);
    pub const BANNER_BG: Color32 = Color32::from_rgb(50, 40, 20);
}

/// Standard spacing constants for consistent layout.
pub struct Spacing;

impl Spacing {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
}

/// Font size constants for typographic consistency.
pub struct FontSize;

#[allow(dead_code)]
impl FontSize {
    pub const BRAND: f32 = 28.0;
    pub const MENU_BRAND: f32 = 15.0;
    pub const PAGE_TITLE: f32 = 24.0;
    pub const HEADING: f32 = 16.0;
    pub const SUBHEADING: f32 = 14.0;
    pub const BODY: f32 = 13.0;
    pub const LABEL: f32 = 12.0;
    pub const SMALL: f32 = 11.0;
    pub const TINY: f32 = 10.0;
    pub const CAPTION: f32 = 9.0;
}

/// Unicode icon constants for UI elements.
#[allow(dead_code)]
pub struct Icons;

#[allow(dead_code)]
impl Icons {
    pub const STAR_FILLED: &str = "\u{2605}";
    pub const STAR_EMPTY: &str = "\u{2606}";
    pub const PLAY: &str = "\u{25B6}";
    pub const STOP: &str = "\u{25A0}";
    pub const PAUSE: &str = "\u{23F8}";
    pub const REFRESH: &str = "\u{1F504}";
    pub const SEARCH: &str = "\u{1F50D}";
    pub const FOLDER: &str = "\u{1F4C1}";
    pub const FILE: &str = "\u{1F4C4}";
    pub const SATELLITE: &str = "\u{1F4E1}";
    pub const ARROW_RIGHT: &str = "\u{25B8}";
    pub const ARROW_BACK: &str = "\u{21B6}";
    pub const ARROW_UP: &str = "\u{2191}";
}

/// Standard rounding constants for UI elements.
pub struct ThemeRounding;

#[allow(dead_code)]
impl ThemeRounding {
    pub const BUTTON: f32 = 6.0;
    pub const BUTTON_SMALL: f32 = 4.0;
    pub const CARD: f32 = 8.0;
    pub const FRAME: f32 = 6.0;
}

/// Standard grid spacing `[horizontal, vertical]`.
pub const GRID_SPACING: [f32; 2] = [12.0, 8.0];

/// Per-Box-type accent colors.
/// Each Box type has its own primary and hover accent for visual identity.
pub struct BoxColors;

#[allow(dead_code)]
impl BoxColors {
    /// Standard (Box 1): Professional Blue
    pub const STANDARD_PRIMARY: Color32 = Color32::from_rgb(45, 120, 210);
    pub const STANDARD_HOVER: Color32 = Color32::from_rgb(55, 140, 230);
    pub const STANDARD_SUBTLE: Color32 = Color32::from_rgb(35, 80, 160);

    /// Hardware Lab (Box 2): Engineering Teal/Green
    pub const LAB_PRIMARY: Color32 = Color32::from_rgb(0, 170, 140);
    pub const LAB_HOVER: Color32 = Color32::from_rgb(20, 200, 165);
    pub const LAB_SUBTLE: Color32 = Color32::from_rgb(0, 120, 100);

    /// Power User (Box 3): Amber/Orange
    pub const POWER_PRIMARY: Color32 = Color32::from_rgb(230, 150, 30);
    pub const POWER_HOVER: Color32 = Color32::from_rgb(245, 170, 50);
    pub const POWER_SUBTLE: Color32 = Color32::from_rgb(180, 110, 20);

    /// Get the primary accent color for a box type.
    pub fn primary(box_type: &BoxType) -> Color32 {
        match box_type {
            BoxType::Standard => Self::STANDARD_PRIMARY,
            BoxType::HardwareLab => Self::LAB_PRIMARY,
            BoxType::PowerUser => Self::POWER_PRIMARY,
        }
    }

    /// Get the hover accent color for a box type.
    pub fn hover(box_type: &BoxType) -> Color32 {
        match box_type {
            BoxType::Standard => Self::STANDARD_HOVER,
            BoxType::HardwareLab => Self::LAB_HOVER,
            BoxType::PowerUser => Self::POWER_HOVER,
        }
    }

    /// Get the subtle/dark accent color for a box type.
    pub fn subtle(box_type: &BoxType) -> Color32 {
        match box_type {
            BoxType::Standard => Self::STANDARD_SUBTLE,
            BoxType::HardwareLab => Self::LAB_SUBTLE,
            BoxType::PowerUser => Self::POWER_SUBTLE,
        }
    }

    /// Background tint for the sidebar when a box type is active.
    pub fn sidebar_tint(box_type: &BoxType) -> Color32 {
        let c = Self::primary(box_type);
        // Very subtle tint — mix 8% of accent into the dark background
        Color32::from_rgb(
            (30u16 + (c.r() as u16 * 8 / 100)) as u8,
            (33u16 + (c.g() as u16 * 8 / 100)) as u8,
            (40u16 + (c.b() as u16 * 8 / 100)) as u8,
        )
    }

    /// Menu bar accent stripe color for the active box type.
    pub fn accent_stripe(box_type: &BoxType) -> Color32 {
        Self::primary(box_type)
    }
}

/// Apply the Libre VMM theme to an egui context.
pub fn apply_theme(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();

    visuals.panel_fill = AppColors::BG_DARK;
    visuals.window_fill = AppColors::BG_PANEL;
    visuals.extreme_bg_color = AppColors::BG_DARK;
    visuals.faint_bg_color = AppColors::BG_CARD;

    visuals.widgets.noninteractive.bg_fill = AppColors::BG_CARD;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, AppColors::TEXT);
    visuals.widgets.noninteractive.rounding = Rounding::same(6.0);

    visuals.widgets.inactive.bg_fill = AppColors::BG_CARD;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, AppColors::TEXT);
    visuals.widgets.inactive.rounding = Rounding::same(6.0);

    visuals.widgets.hovered.bg_fill = AppColors::BG_HOVER;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, AppColors::TEXT);
    visuals.widgets.hovered.rounding = Rounding::same(6.0);

    visuals.widgets.active.bg_fill = AppColors::PRIMARY;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.active.rounding = Rounding::same(6.0);

    visuals.selection.bg_fill = AppColors::PRIMARY.linear_multiply(0.3);
    visuals.selection.stroke = Stroke::new(1.0, AppColors::PRIMARY);

    visuals.window_rounding = Rounding::same(8.0);
    visuals.window_stroke = Stroke::new(1.0, Color32::from_rgb(60, 65, 80));

    ctx.set_visuals(visuals);

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "default".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/Inter-Regular.ttf"
        ))),
    );

    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(22.0, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(14.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(12.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(13.0, FontFamily::Monospace),
    );
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);

    ctx.set_style(style);
}

/// Apply box-type accent colors to the egui context.
/// Call this after apply_theme() to overlay the box-specific accent.
pub fn apply_box_accent(ctx: &egui::Context, box_type: &BoxType) {
    let primary = BoxColors::primary(box_type);
    let primary_hover = BoxColors::hover(box_type);

    let mut visuals = ctx.style().visuals.clone();
    visuals.widgets.active.bg_fill = primary;
    visuals.selection.bg_fill = primary.linear_multiply(0.3);
    visuals.selection.stroke = Stroke::new(1.0, primary);
    // Hyperlink color follows the box accent
    visuals.hyperlink_color = primary_hover;
    ctx.set_visuals(visuals);
}

/// Standardized button builders for consistent styling across the app.
///
/// Use these instead of constructing `egui::Button` with ad-hoc fill colors —
/// they enforce the design system's primary/danger/muted color palette and
/// consistent rounding.
#[allow(dead_code)]
pub mod buttons {
    use super::{AppColors, FontSize, ThemeRounding};
    use eframe::egui;
    use egui::{Button, Color32, RichText, Rounding};

    /// Primary action button — accent fill, white text, button rounding.
    /// Use for the main affirmative action in a view (Save, Apply, Create, Start).
    pub fn primary(text: impl Into<String>) -> Button<'static> {
        Button::new(
            RichText::new(text.into())
                .color(Color32::WHITE)
                .size(FontSize::BODY),
        )
        .fill(AppColors::PRIMARY)
        .rounding(Rounding::same(ThemeRounding::BUTTON))
    }

    /// Destructive button — danger fill, white text. Use for Delete, Force-Stop, Remove.
    pub fn danger(text: impl Into<String>) -> Button<'static> {
        Button::new(
            RichText::new(text.into())
                .color(Color32::WHITE)
                .size(FontSize::BODY),
        )
        .fill(AppColors::DANGER)
        .rounding(Rounding::same(ThemeRounding::BUTTON))
    }

    /// Success-styled button — green fill, white text. Use for Connect, Resume, Confirm-positive.
    pub fn success(text: impl Into<String>) -> Button<'static> {
        Button::new(
            RichText::new(text.into())
                .color(Color32::WHITE)
                .size(FontSize::BODY),
        )
        .fill(AppColors::SUCCESS)
        .rounding(Rounding::same(ThemeRounding::BUTTON))
    }

    /// Muted secondary button — transparent fill, dim text. Use for Cancel, Close, Back.
    pub fn muted(text: impl Into<String>) -> Button<'static> {
        Button::new(
            RichText::new(text.into())
                .color(AppColors::TEXT_DIM)
                .size(FontSize::BODY),
        )
        .fill(Color32::TRANSPARENT)
        .rounding(Rounding::same(ThemeRounding::BUTTON))
    }
}

/// Check if the user pressed Escape this frame — for dialog close handling.
/// Use as: `if theme::escape_pressed(ctx) { state.open = false; }`
#[allow(dead_code)]
pub fn escape_pressed(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::Escape))
}
