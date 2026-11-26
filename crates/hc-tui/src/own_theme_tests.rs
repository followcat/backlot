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
    let p = Palette::backlot();
    assert_eq!(p.sidebar_bg, Color::Rgb(12, 14, 21));
    assert_ne!(p.sidebar_bg, Color::Reset);
    let sum = |c: Color| match c {
        Color::Rgb(r, g, b) => u32::from(r) + u32::from(g) + u32::from(b),
        _ => panic!("an opaque RGB background, got {c:?}"),
    };
    assert!(sum(p.sidebar_bg) < sum(p.panel_bg));
}

#[test]
fn backlot_is_a_theme_of_its_own_and_not_one_of_the_references() {
    assert_eq!(Palette::from_name("backlot"), Some(Palette::backlot()));
    assert_eq!(Palette::from_name("Backlot"), Some(Palette::backlot()));
    assert!(
        !THEME_NAMES.contains(&NAME),
        "the reference list stays as it is"
    );
    // Told apart at a glance: no reference theme shares its accent or
    // the background of its bars and overlays.
    let own = Palette::backlot();
    for name in THEME_NAMES {
        let other = Palette::from_name(name).unwrap();
        assert_ne!(other.accent, own.accent, "{name}");
        assert_ne!(other.panel_bg, own.panel_bg, "{name}");
    }
}

#[test]
fn the_own_list_is_backlot_then_the_references_in_order() {
    let own = Shell {
        path: Some("/scratch/config.toml".into()),
        own: true,
        source_herdr: false,
    };
    assert_eq!(own.names()[0], NAME);
    assert_eq!(&own.names()[1..], THEME_NAMES);
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
