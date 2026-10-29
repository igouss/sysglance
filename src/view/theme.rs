use std::{fs, time::Duration};

use gpui::{App, Font, Global, Hsla, Pixels, font};
use gpui_component::{ActiveTheme, Theme, ThemeMode};
use sysglance_core::level::Level;

use crate::omarchy;

const POLL: Duration = Duration::from_secs(1);

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
            faint: cx
                .try_global::<Faint>()
                .map_or_else(|| theme.muted_foreground.opacity(0.55), |faint| faint.0),
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

struct Faint(Hsla);

impl Global for Faint {}

fn hsla(hex: u32) -> Hsla {
    gpui::rgb(hex).into()
}

fn apply(palette: Option<omarchy::Palette>, font: Option<&str>, cx: &mut App) {
    let Some(palette) = palette else {
        Theme::change(ThemeMode::Dark, None, cx);
        return;
    };
    Theme::change(
        if palette.dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
    Theme::update(cx, |theme| {
        let colours = &mut theme.colors;
        colours.background = hsla(palette.background);
        colours.foreground = hsla(palette.foreground);
        colours.secondary = hsla(palette.surface);
        colours.secondary_foreground = hsla(palette.foreground);
        colours.popover = hsla(palette.surface);
        colours.popover_foreground = hsla(palette.foreground);
        colours.border = hsla(palette.border);
        colours.selection = hsla(palette.border);
        colours.muted_foreground = hsla(palette.muted);
        colours.accent = hsla(palette.accent);
        colours.primary = hsla(palette.accent);
        colours.ring = hsla(palette.accent);
        colours.success = hsla(palette.normal);
        colours.info = hsla(palette.low);
        colours.warning = hsla(palette.high);
        colours.danger = hsla(palette.critical);
        colours.scrollbar_thumb = hsla(palette.muted).opacity(0.5);
        if let Some(font) = font {
            theme.font_family = font.to_owned().into();
            theme.mono_font_family = font.to_owned().into();
        }
    });
    cx.set_global(Faint(hsla(palette.faint)));
}

pub fn follow_omarchy(cx: &mut App) {
    let Some(path) = omarchy::colours_path() else {
        apply(None, None, cx);
        return;
    };
    let read = move || fs::read_to_string(&path).ok();
    let mut seen = read();
    apply(
        seen.as_deref().and_then(omarchy::parse),
        omarchy::font().as_deref(),
        cx,
    );
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(POLL).await;
            let now = read();
            if now != seen {
                seen = now;
                let palette = seen.as_deref().and_then(omarchy::parse);
                cx.update(|cx| apply(palette, omarchy::font().as_deref(), cx));
            }
        }
    })
    .detach();
}
