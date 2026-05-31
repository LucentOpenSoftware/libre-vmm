//! Internationalization support — locale detection and persistence.

use std::path::PathBuf;

/// Supported languages.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Language {
    /// Use system locale.
    Auto,
    English,
    Spanish,
}

impl Language {
    pub fn code(&self) -> &'static str {
        match self {
            Language::Auto => "auto",
            Language::English => "en",
            Language::Spanish => "es",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code {
            "en" => Language::English,
            "es" => Language::Spanish,
            _ => Language::Auto,
        }
    }

    /// All available language choices (for settings UI).
    pub fn all() -> &'static [Language] {
        &[Language::Auto, Language::English, Language::Spanish]
    }
}

/// Path to the persisted language preference file.
fn lang_file() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("libre-vmm");
    config_dir.join("language")
}

/// Load the saved language preference from disk, or Auto if none saved.
pub fn load_language() -> Language {
    if let Ok(code) = std::fs::read_to_string(lang_file()) {
        Language::from_code(code.trim())
    } else {
        Language::Auto
    }
}

/// Save the language preference to disk.
pub fn save_language(lang: Language) {
    let path = lang_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, lang.code());
}

/// Detect the system locale and return "en" or "es" (or "en" as fallback).
fn detect_system_locale() -> &'static str {
    if let Some(locale) = sys_locale::get_locale() {
        let lower = locale.to_lowercase();
        if lower.starts_with("es") {
            return "es";
        }
    }
    "en"
}

/// Apply a language choice to rust-i18n.
pub fn apply_language(lang: Language) {
    let code = match lang {
        Language::Auto => detect_system_locale(),
        Language::English => "en",
        Language::Spanish => "es",
    };
    rust_i18n::set_locale(code);
}

/// Initialize locale on startup: load preference and apply it.
pub fn init_locale() {
    let lang = load_language();
    apply_language(lang);
}
