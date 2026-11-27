//! backlot's own `[theme]`: the palette, the file it is read from and
//! what `source = "herdr"` leaves alone.
use super::*;
use crate::chrome::palette::{ThemeRuntimeConfig, resolve_effective_theme};
use crate::config::Config;
use crate::test_support::TestDir;

/// A scratch `backlot/config.toml` with `text` (`None`: no file).
fn own_file(text: Option<&str>) -> (TestDir, PathBuf) {
    let dir = TestDir::new("backlot-own-theme");
    let path = dir.join("config.toml");
    if let Some(text) = text {
        std::fs::write(&path, text).unwrap();
    }
    (dir, path)
}

/// herdr's config as loaded, with a theme of its own.
fn herdr(theme: &str) -> LoadedConfig {
    let mut config = Config::default();
    config.theme.name = Some(theme.into());
    LoadedConfig {
        config,
        diagnostics: Vec::new(),
        invalid_sections: Vec::new(),
    }
}

fn palette_of(loaded: &LoadedConfig) -> (Palette, String) {
    resolve_effective_theme(&ThemeRuntimeConfig::from_config(&loaded.config), None)
}

#[test]
fn the_backlot_sidebar_is_opaque_and_one_step_darker() {
    // A translucent terminal showed the window behind the sidebar:
    // the theme's own background is opaque now, a step darker than
    // the panes so the two surfaces read apart.
    let sum = |c: Color| match c {
        Color::Rgb(r, g, b) => u32::from(r) + u32::from(g) + u32::from(b),
        _ => panic!("an opaque RGB background, got {c:?}"),
    };
    for (palette, expected_sidebar) in [
        (Palette::backlot(), Color::Rgb(12, 10, 15)),
        (Palette::backlot_day(), Color::Rgb(234, 220, 195)),
        (Palette::backlot_midnight(), Color::Rgb(12, 14, 21)),
    ] {
        assert_eq!(palette.sidebar_bg, expected_sidebar);
        assert_ne!(palette.sidebar_bg, Color::Reset);
        assert!(sum(palette.sidebar_bg) < sum(palette.panel_bg));
    }
}

#[test]
fn backlot_is_a_theme_of_its_own_and_not_one_of_the_references() {
    assert_eq!(Palette::from_name("backlot"), Some(Palette::backlot()));
    assert_eq!(Palette::from_name("Backlot"), Some(Palette::backlot()));
    assert_eq!(Palette::from_name("babylon"), Some(Palette::backlot()));
    assert_eq!(Palette::from_name("Babylon"), Some(Palette::backlot()));
    assert_eq!(
        Palette::from_name("backlot-day"),
        Some(Palette::backlot_day())
    );
    assert_eq!(
        Palette::from_name("babylon-day"),
        Some(Palette::backlot_day())
    );
    assert_eq!(
        Palette::from_name("Babylon Day"),
        Some(Palette::backlot_day())
    );
    assert_eq!(
        Palette::from_name("backlot-midnight"),
        Some(Palette::backlot_midnight())
    );
    assert_eq!(
        Palette::from_name("Backlot_Midnight"),
        Some(Palette::backlot_midnight())
    );
    for own_name in [NAME, DAY_NAME, MIDNIGHT_NAME] {
        assert!(
            !THEME_NAMES.contains(&own_name),
            "the reference list stays as it is ({own_name})"
        );
    }
    // Told apart at a glance: no reference theme shares the backlot
    // themes' accent or panel background.
    for own in [
        Palette::backlot(),
        Palette::backlot_day(),
        Palette::backlot_midnight(),
    ] {
        for name in THEME_NAMES {
            let other = Palette::from_name(name).unwrap();
            assert_ne!(other.accent, own.accent, "{name}");
            assert_ne!(other.panel_bg, own.panel_bg, "{name}");
        }
    }
}

#[test]
fn the_own_list_is_backlot_then_the_references_in_order() {
    let own = Shell {
        path: Some("/scratch/config.toml".into()),
        own: true,
        source_herdr: false,
    };
    assert_eq!(&own.names()[..3], &[NAME, DAY_NAME, MIDNIGHT_NAME]);
    assert_eq!(&own.names()[3..], THEME_NAMES);
    assert_eq!(Shell::default().names(), THEME_NAMES);
}

#[test]
fn with_no_file_the_theme_is_backlot_whatever_herdr_says() {
    let (_root, path) = own_file(None);
    let mut loaded = herdr("nord");
    loaded.config.ui.accent = "green".into();
    let shell = Shell::at(path.clone()).overlay(&mut loaded);
    assert!(shell.own);
    let (palette, name) = palette_of(&loaded);
    assert_eq!(name, NAME);
    assert_eq!(
        palette,
        Palette::backlot(),
        "herdr's legacy accent stays out"
    );
    assert_eq!(shell.save_path(Path::new("/scratch/herdr.toml")), path);
}

#[test]
fn a_file_with_no_theme_table_is_the_same_as_no_file() {
    let (_root, path) = own_file(Some("[panel]\nwidth = 40\n"));
    let mut loaded = herdr("nord");
    assert!(Shell::at(path).overlay(&mut loaded).own);
    assert_eq!(palette_of(&loaded).1, NAME);
    assert!(loaded.diagnostics.is_empty());
}

#[test]
fn the_own_table_picks_any_theme_and_its_overrides() {
    let (_root, path) = own_file(Some(
        "[theme]\nname = \"dracula\"\n\n[theme.custom]\naccent = \"#010203\"\n",
    ));
    let mut loaded = herdr("nord");
    assert!(Shell::at(path).overlay(&mut loaded).own);
    let (palette, name) = palette_of(&loaded);
    assert_eq!(name, "dracula");
    assert_eq!(palette.accent, Color::Rgb(1, 2, 3));
    assert_eq!(palette.panel_bg, Palette::dracula().panel_bg);
}

#[test]
fn overrides_without_a_name_go_on_top_of_backlot() {
    let (_root, path) = own_file(Some("[theme.custom]\naccent = \"#010203\"\n"));
    let mut loaded = herdr("nord");
    Shell::at(path).overlay(&mut loaded);
    let (palette, name) = palette_of(&loaded);
    assert_eq!(name, NAME);
    assert_eq!(palette.accent, Color::Rgb(1, 2, 3));
    assert_eq!(palette.text, Palette::backlot().text);
}

#[test]
fn source_herdr_leaves_herdrs_load_untouched() {
    let (_root, path) = own_file(Some("[theme]\nsource = \"herdr\"\nname = \"dracula\"\n"));
    let mut loaded = herdr("nord");
    loaded.config.ui.accent = "green".into();
    loaded.diagnostics.push("unknown theme name x".into());
    loaded.invalid_sections.push("theme".into());
    let shell = Shell::at(path).overlay(&mut loaded);
    assert!(!shell.own);
    assert_eq!(loaded.config.theme.name.as_deref(), Some("nord"));
    assert_eq!(loaded.config.ui.accent, "green");
    assert_eq!(loaded.diagnostics, ["unknown theme name x"]);
    assert_eq!(loaded.invalid_sections, ["theme"]);
    assert_eq!(shell.names(), THEME_NAMES);
    let herdr = Path::new("/scratch/herdr.toml");
    assert_eq!(shell.save_path(herdr), herdr);
}

#[test]
fn herdrs_theme_trouble_does_not_reach_the_own_theme() {
    let (_root, path) = own_file(None);
    let mut loaded = herdr("no-such-theme");
    loaded
        .diagnostics
        .push("unknown theme name theme.name = \"no-such-theme\"; using \"catppuccin\"".into());
    loaded.diagnostics.push("unknown key ui.nope".into());
    loaded.invalid_sections.push("theme".into());
    Shell::at(path).overlay(&mut loaded);
    assert_eq!(loaded.diagnostics, ["unknown key ui.nope"]);
    assert!(loaded.invalid_sections.is_empty());
}

#[test]
fn an_unknown_own_theme_name_is_reported_like_herdrs() {
    let (_root, path) = own_file(Some("[theme]\nname = \"no-such-theme\"\n"));
    let mut loaded = herdr("nord");
    Shell::at(path).overlay(&mut loaded);
    assert_eq!(loaded.diagnostics.len(), 1, "{:?}", loaded.diagnostics);
    assert!(loaded.diagnostics[0].starts_with("unknown theme name theme.name"));
}

#[test]
fn an_unknown_source_is_backlot_with_a_notice() {
    let (_root, path) = own_file(Some("[theme]\nsource = \"hc-tui\"\nname = \"nord\"\n"));
    let mut loaded = herdr("dracula");
    assert!(Shell::at(path).overlay(&mut loaded).own);
    assert_eq!(palette_of(&loaded).1, "nord");
    assert_eq!(
        loaded.diagnostics,
        ["theme.source must be \"backlot\" or \"herdr\"; using \"backlot\""]
    );
}

#[test]
fn a_table_that_cannot_be_used_is_backlot_and_the_notice_carries_no_file_text() {
    let (_root, path) = own_file(Some("[theme]\nname = 5\nauto_switch = \"secret-word\"\n"));
    let mut loaded = herdr("nord");
    assert!(Shell::at(path).overlay(&mut loaded).own);
    assert_eq!(palette_of(&loaded).1, NAME);
    assert_eq!(loaded.diagnostics.len(), 1);
    assert!(loaded.diagnostics[0].ends_with("[theme] is not valid; using \"backlot\""));
    assert!(!loaded.diagnostics[0].contains("secret-word"));
}

#[test]
fn a_file_that_is_not_toml_is_backlot_and_left_to_the_panels_notice() {
    let (_root, path) = own_file(Some("[theme\nname = \"nord\"\n"));
    let mut loaded = herdr("nord");
    assert!(Shell::at(path).overlay(&mut loaded).own);
    assert_eq!(palette_of(&loaded).1, NAME);
    assert!(loaded.diagnostics.is_empty());
}

#[test]
fn a_shell_with_no_path_reads_herdrs_theme_as_hc_tui_does() {
    let mut loaded = herdr("nord");
    let shell = Shell::default().overlay(&mut loaded);
    assert!(!shell.own);
    assert_eq!(palette_of(&loaded).1, "nord");
}

#[test]
fn backlot_midnight_matches_the_original_backlot_token_for_token() {
    let expected = Palette {
        accent: Color::Rgb(240, 178, 74),
        panel_bg: Color::Rgb(17, 20, 29),
        sidebar_bg: Color::Rgb(12, 14, 21),
        active_row_bg: Color::Rgb(27, 32, 48),
        selection_bg: Color::Rgb(44, 53, 80),
        surface0: Color::Rgb(38, 45, 66),
        surface1: Color::Rgb(57, 66, 92),
        surface_dim: Color::Rgb(23, 27, 40),
        overlay0: Color::Rgb(111, 120, 147),
        overlay1: Color::Rgb(136, 146, 173),
        text: Color::Rgb(236, 229, 211),
        subtext0: Color::Rgb(185, 179, 164),
        mauve: Color::Rgb(207, 162, 232),
        green: Color::Rgb(143, 214, 148),
        yellow: Color::Rgb(242, 222, 121),
        red: Color::Rgb(242, 96, 124),
        blue: Color::Rgb(130, 180, 245),
        teal: Color::Rgb(107, 209, 196),
        peach: Color::Rgb(242, 139, 98),
    };
    assert_eq!(Palette::backlot_midnight(), expected);
}

#[test]
fn babylon_night_and_day_match_the_design_table() {
    let night = Palette {
        accent: Color::Rgb(0xf2, 0xb8, 0x4b),
        panel_bg: Color::Rgb(0x12, 0x0f, 0x16),
        sidebar_bg: Color::Rgb(0x0c, 0x0a, 0x0f),
        active_row_bg: Color::Rgb(0x2d, 0x24, 0x33),
        selection_bg: Color::Rgb(0x3e, 0x31, 0x46),
        surface0: Color::Rgb(0x2f, 0x26, 0x36),
        surface1: Color::Rgb(0x4e, 0x3f, 0x56),
        surface_dim: Color::Rgb(0x19, 0x14, 0x1f),
        overlay0: Color::Rgb(0x80, 0x73, 0x84),
        overlay1: Color::Rgb(0xa2, 0x93, 0xa4),
        text: Color::Rgb(0xf2, 0xe8, 0xd5),
        subtext0: Color::Rgb(0xc9, 0xbb, 0xa6),
        mauve: Color::Rgb(0xdf, 0x92, 0xc4),
        blue: Color::Rgb(0x82, 0xb6, 0xf5),
        teal: Color::Rgb(0x62, 0xd4, 0xc1),
        peach: Color::Rgb(0xf2, 0x93, 0x57),
        yellow: Color::Rgb(0xf6, 0xd9, 0x73),
        green: Color::Rgb(0x7c, 0xd6, 0x90),
        red: Color::Rgb(0xee, 0x52, 0x66),
    };
    let day = Palette {
        accent: Color::Rgb(0x8c, 0x53, 0x0a),
        panel_bg: Color::Rgb(0xf6, 0xec, 0xd9),
        sidebar_bg: Color::Rgb(0xea, 0xdc, 0xc3),
        active_row_bg: Color::Rgb(0xdf, 0xd0, 0xb4),
        selection_bg: Color::Rgb(0xd4, 0xba, 0x90),
        surface0: Color::Rgb(0xdf, 0xd0, 0xb5),
        surface1: Color::Rgb(0xbf, 0xa9, 0x89),
        surface_dim: Color::Rgb(0xec, 0xe0, 0xc8),
        overlay0: Color::Rgb(0x75, 0x65, 0x54),
        overlay1: Color::Rgb(0x5c, 0x4d, 0x3e),
        text: Color::Rgb(0x26, 0x1b, 0x14),
        subtext0: Color::Rgb(0x4e, 0x3d, 0x30),
        mauve: Color::Rgb(0x8c, 0x2d, 0x6b),
        blue: Color::Rgb(0x1f, 0x53, 0x99),
        teal: Color::Rgb(0x14, 0x69, 0x60),
        peach: Color::Rgb(0xa3, 0x44, 0x0d),
        yellow: Color::Rgb(0x87, 0x55, 0x00),
        green: Color::Rgb(0x24, 0x6b, 0x3b),
        red: Color::Rgb(0xb8, 0x22, 0x35),
    };
    assert_eq!(Palette::backlot(), night);
    assert_eq!(Palette::backlot_day(), day);
}

#[test]
fn aliases_siblings_and_auto_switch_resolve_babylon_night_day_and_midnight() {
    use crate::chrome::palette::{HostAppearance, sibling_theme_names};
    use crate::config::canonical_theme_name;

    assert_eq!(canonical_theme_name("babylon"), Some("backlot"));
    assert_eq!(canonical_theme_name("babylon-day"), Some("backlot-day"));
    assert_eq!(
        canonical_theme_name("backlot-midnight"),
        Some("backlot-midnight")
    );

    for name in ["backlot", "babylon", "backlot-day", "babylon-day"] {
        assert_eq!(
            sibling_theme_names(name),
            ("backlot".to_string(), "backlot-day".to_string()),
            "{name}"
        );
    }
    assert_eq!(
        sibling_theme_names("backlot-midnight"),
        (
            "backlot-midnight".to_string(),
            "backlot-midnight".to_string()
        )
    );

    // Default `auto_switch = true` (no name or `backlot`/`babylon`) switches Night <-> Day.
    for text in [
        "[theme]\nauto_switch = true\n",
        "[theme]\nname = \"babylon\"\nauto_switch = true\n",
    ] {
        let (_root, path) = own_file(Some(text));
        let mut loaded = herdr("nord");
        Shell::at(path).overlay(&mut loaded);
        assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
        let runtime = ThemeRuntimeConfig::from_config(&loaded.config);
        let (dark_p, dark_name) = resolve_effective_theme(&runtime, Some(HostAppearance::Dark));
        let (light_p, light_name) = resolve_effective_theme(&runtime, Some(HostAppearance::Light));
        assert_eq!(dark_name, "backlot");
        assert_eq!(dark_p, Palette::backlot());
        assert_eq!(light_name, "backlot-day");
        assert_eq!(light_p, Palette::backlot_day());
    }

    // `backlot-midnight` has no light sibling, so `auto_switch` keeps it in both modes
    // unless `light_name` is set explicitly.
    let (_root, path) = own_file(Some(
        "[theme]\nname = \"backlot-midnight\"\nauto_switch = true\n",
    ));
    let mut loaded = herdr("nord");
    Shell::at(path).overlay(&mut loaded);
    let runtime = ThemeRuntimeConfig::from_config(&loaded.config);
    assert_eq!(
        resolve_effective_theme(&runtime, Some(HostAppearance::Dark)),
        (Palette::backlot_midnight(), "backlot-midnight".to_string())
    );
    assert_eq!(
        resolve_effective_theme(&runtime, Some(HostAppearance::Light)),
        (Palette::backlot_midnight(), "backlot-midnight".to_string())
    );

    let (_root, path) = own_file(Some(
        "[theme]\nname = \"backlot-midnight\"\nauto_switch = true\nlight_name = \"babylon-day\"\n",
    ));
    let mut loaded = herdr("nord");
    Shell::at(path).overlay(&mut loaded);
    let runtime = ThemeRuntimeConfig::from_config(&loaded.config);
    assert_eq!(
        resolve_effective_theme(&runtime, Some(HostAppearance::Light)),
        (Palette::backlot_day(), "backlot-day".to_string())
    );
}

#[test]
fn babylon_night_day_and_midnight_pass_surface_contrast_checks() {
    use crate::chrome::elevation::contrast_ratio;

    let rgb = |color: Color| match color {
        Color::Rgb(r, g, b) => (r, g, b),
        other => panic!("expected Rgb, got {other:?}"),
    };

    for (name, palette) in [
        ("backlot", Palette::backlot()),
        ("backlot-day", Palette::backlot_day()),
        ("backlot-midnight", Palette::backlot_midnight()),
    ] {
        let elev = palette.elevation();
        let surface = rgb(elev.surface);
        let text_cr = contrast_ratio(rgb(palette.text), surface);
        assert!(text_cr >= 12.0, "{name}: text CR={text_cr:.2}");
        for (tok, color) in [
            ("accent", palette.accent),
            ("subtext0", palette.subtext0),
            ("overlay1", palette.overlay1),
            ("mauve", palette.mauve),
            ("blue", palette.blue),
            ("teal", palette.teal),
            ("peach", palette.peach),
            ("yellow", palette.yellow),
            ("green", palette.green),
            ("red", palette.red),
        ] {
            let cr = contrast_ratio(rgb(color), surface);
            assert!(cr >= 4.5, "{name}.{tok} CR={cr:.2} < 4.5");
        }
    }
    let day = Palette::backlot_day();
    let day_surface = rgb(day.elevation().surface);
    assert!(
        contrast_ratio(rgb(day.accent), day_surface) >= 4.5,
        "backlot-day.accent must meet AA against elev.surface"
    );
    assert!(
        contrast_ratio(rgb(day.accent), rgb(day.panel_bg)) >= 5.3,
        "backlot-day.accent must remain high contrast on panel_bg"
    );
}
