use gpui::{App, Font, Hsla, Pixels, font};
use gpui_component::ActiveTheme;
use sysglance_core::level::Level;

#[derive(Clone)]
pub struct Palette {
    pub sans: Font,
    pub mono: Font,
    pub size: Pixels,
    pub surface: Hsla,
    pub border: Hsla,
    pub hover: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    normal: Hsla,
    low: Hsla,
    high: Hsla,
    critical: Hsla,
}

impl Palette {
    pub fn of(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            sans: font(theme.font_family.clone()),
            mono: font(theme.mono_font_family.clone()),
            size: theme.font_size * 0.875,
            surface: theme.secondary,
            border: theme.border,
            hover: theme.foreground.opacity(0.06),
            text: theme.foreground,
            muted: theme.muted_foreground,
            faint: theme.muted_foreground.opacity(0.55),
            normal: theme.success,
            low: theme.info,
            high: theme.warning,
            critical: theme.danger,
        }
    }

    pub const fn level(&self, level: Level) -> Hsla {
        match level {
            Level::Unrated => self.text,
            Level::Normal => self.normal,
            Level::Low => self.low,
            Level::High => self.high,
            Level::Critical => self.critical,
        }
    }
}
