use std::fs;

use std::{ffi::OsString, path::PathBuf};

use super::{Palette, colours_in, family, parse};

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
fn colours_live_under_the_state_directory() {
    assert_eq!(
        colours_in(Some(OsString::from("/s")), Some(OsString::from("/h"))),
        Some(PathBuf::from("/s/omarchy/current/theme/colors.toml"))
    );
}

#[test]
fn without_a_state_directory_colours_live_under_home() {
    assert_eq!(
        colours_in(None, Some(OsString::from("/h"))),
        Some(PathBuf::from(
            "/h/.local/state/omarchy/current/theme/colors.toml"
        ))
    );
}

#[test]
fn without_state_or_home_there_is_no_theme() {
    assert_eq!(colours_in(None, None), None);
}

#[test]
fn the_font_is_the_trimmed_output_of_a_successful_run() {
    assert_eq!(
        family(true, b"JetBrainsMono Nerd Font\n"),
        Some("JetBrainsMono Nerd Font".to_owned())
    );
}

#[test]
fn a_failed_or_empty_run_has_no_font() {
    assert_eq!(family(false, b"JetBrainsMono Nerd Font\n"), None);
    assert_eq!(family(true, b"  \n"), None);
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
