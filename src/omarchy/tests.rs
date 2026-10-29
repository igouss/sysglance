use std::fs;

use super::{Palette, parse};

const EVERFOREST: &str = include_str!("everforest.toml");
const LATTE: &str = include_str!("catppuccin-latte.toml");

#[test]
fn a_dark_theme_maps_its_palette() {
    assert_eq!(
        parse(EVERFOREST),
        Some(Palette {
            dark: true,
            background: 0x2d_353b,
            surface: 0x34_3f44,
            border: 0x3d_484d,
            foreground: 0xd3_c6aa,
            muted: 0x9d_a9a0,
            faint: 0x4f_585e,
            accent: 0x7f_bbb3,
            normal: 0xa7_c080,
            low: 0x7f_bbb3,
            high: 0xdb_bc7f,
            critical: 0xe6_7e80,
        })
    );
}

#[test]
fn a_light_theme_puts_cards_on_its_darker_background() {
    let palette = parse(LATTE).unwrap();
    assert!(!palette.dark);
    assert_eq!(palette.surface, 0xe3_e4e8);
}

#[test]
fn a_theme_without_background_or_foreground_is_rejected() {
    assert_eq!(parse("mode = \"dark\"\nforeground = \"#ffffff\"\n"), None);
    assert_eq!(parse(""), None);
}

#[test]
fn missing_colours_fall_back_to_the_foreground() {
    let palette = parse("background = \"#000000\"\nforeground = \"#ffffff\"\n").unwrap();
    assert!(palette.dark);
    assert_eq!(
        (palette.surface, palette.critical, palette.border),
        (0x00_0000, 0xff_ffff, 0xff_ffff)
    );
}

#[test]
fn malformed_colours_are_ignored() {
    let palette = parse(
        "background = \"#000000\"\nforeground = \"#ffffff\"\nred = \"#12345\"\ngreen = \"red\"\n",
    )
    .unwrap();
    assert_eq!((palette.critical, palette.normal), (0xff_ffff, 0xff_ffff));
}

#[test]
#[ignore = "reads the themes installed under /usr/share/omarchy/themes"]
fn every_installed_theme_parses() {
    let mut parsed = 0;
    for entry in fs::read_dir("/usr/share/omarchy/themes").unwrap() {
        let path = entry.unwrap().path().join("colors.toml");
        let text = fs::read_to_string(&path).unwrap();
        assert!(parse(&text).is_some(), "{} does not parse", path.display());
        parsed += 1;
    }
    assert!(parsed >= 20, "parsed only {parsed} themes");
}
