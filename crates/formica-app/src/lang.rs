//! Language selection and CJK font loading for the egui panel.
//!
//! The default egui fonts (Ubuntu-Light, NotoEmoji) contain no CJK glyphs, so
//! switching the panel to Chinese without extra work would render every label
//! as tofu boxes. On start-up this module scans the usual Windows font
//! locations (plus `FORMICA_CJK_FONT` for anything unusual) and registers the
//! first CJK-capable file it finds into egui's fallback list, so Latin text
//! keeps its current look and Chinese text falls back to a real font.
//!
//! The chosen language lives in the [`UiLang`] resource. `FORMICA_LANG` seeds
//! it; the panel's Language section can flip it live, because every widget
//! re-reads the table each frame.

use bevy::prelude::*;
use bevy_egui::egui;

use formica_core::i18n::{Lang, Strings};

/// Which language the panel currently speaks.
#[derive(Resource, Debug, Clone, Copy)]
pub struct UiLang {
    /// The active language.
    pub lang: Lang,
}

impl UiLang {
    /// The active string table.
    pub fn strings(&self) -> &'static Strings {
        self.lang.strings()
    }
}

impl Default for UiLang {
    fn default() -> Self {
        // `FORMICA_LANG` seeds the initial language; unparseable values keep
        // the English default rather than failing start-up.
        let lang = std::env::var("FORMICA_LANG")
            .ok()
            .and_then(|raw| Lang::parse(&raw))
            .unwrap_or_default();
        Self { lang }
    }
}

/// Candidate CJK font files, most common first. Windows ships Microsoft YaHei
/// on every desktop SKU; the others cover stripped-down installs and other
/// platforms.
const CJK_FONT_CANDIDATES: [&str; 8] = [
    r"C:\Windows\Fonts\msyh.ttc",    // Microsoft YaHei (Win 8+)
    r"C:\Windows\Fonts\msyh.ttf",    // Microsoft YaHei (older layout)
    r"C:\Windows\Fonts\simhei.ttf",  // SimHei
    r"C:\Windows\Fonts\simsun.ttc",  // SimSun
    r"C:\Windows\Fonts\simfang.ttf", // FangSong
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
];

/// Try to find and register a CJK-capable font into egui's fallback chain.
///
/// Idempotent in effect: it only *adds* a fallback, so if no CJK font exists
/// the panel simply keeps rendering boxes for Chinese - the same behaviour as
/// before this module existed, and English keeps working either way.
pub fn install_cjk_fallback(ctx: &egui::Context) {
    // An explicit override wins over the built-in candidates.
    let path = std::env::var("FORMICA_CJK_FONT").ok().or_else(|| {
        CJK_FONT_CANDIDATES
            .iter()
            .find(|p| std::path::Path::new(p).exists())
            .map(|p| p.to_string())
    });

    let Some(path) = path else {
        info!("no CJK font found; Chinese labels will render as boxes (set FORMICA_CJK_FONT)");
        return;
    };
    let Ok(bytes) = std::fs::read(&path) else {
        warn!("failed to read CJK font {path}");
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "formica_cjk".to_owned(),
        egui::FontData::from_owned(bytes).into(),
    );
    // Proportional and monospace both fall back to it; emoji stays untouched.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("formica_cjk".to_owned());
    }
    ctx.set_fonts(fonts);
    info!("registered CJK fallback font from {path}");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `FORMICA_LANG` must be parsed leniently: valid values switch, garbage
    /// keeps English, and the default is English.
    #[test]
    fn ui_lang_defaults_to_english() {
        assert_eq!(UiLang::default().lang, Lang::En);
        assert_eq!(UiLang::default().strings().run_section, "Run");
    }

    /// A `FORMICA_LANG=zh` environment must be honoured - this is what the
    /// scripted captures rely on. Env-var mutation in tests is racy across
    /// threads, so this only checks the parse half directly.
    #[test]
    fn formica_lang_values_parse() {
        assert_eq!(Lang::parse("zh"), Some(Lang::Zh));
        assert_eq!(Lang::parse("zh-cn"), Some(Lang::Zh));
        assert_eq!(Lang::parse("nope"), None);
    }
}
