//! The `[panel]` config file: defaults, the width clamp, the toggle
//! key and the malformed-file notice.
use crate::panel::{PanelConfig, WIDTH_MAX, WIDTH_MIN, config_path, load};
use std::ffi::OsString;
use std::io::Write;

fn env(entries: &[(&str, &str)]) -> hc_proto::endpoint::Env {
    entries
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect()
}

#[test]
fn a_missing_file_is_defaults() {
    let dir = std::env::temp_dir().join(format!("panel-none-{}", std::process::id()));
    let (config, notice) = load(&dir.join("config.toml"));
    assert!(notice.is_none());
    assert_eq!(config.width, 30);
    assert_eq!(config.key, "y");
}

#[test]
fn the_config_path_prefers_xdg_then_home() {
    let xdg = env(&[("XDG_CONFIG_HOME", "/tmp/xdg-test"), ("HOME", "/home/user")]);
    assert_eq!(
        config_path(&xdg),
        std::path::PathBuf::from("/tmp/xdg-test/backlot/config.toml")
    );
    let home = env(&[("HOME", "/home/user")]);
    assert_eq!(
        config_path(&home),
        std::path::PathBuf::from("/home/user/.config/backlot/config.toml")
    );
}

fn write(dir: &std::path::Path, text: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("config.toml");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
    path
}

#[test]
fn width_clamps_into_the_24_60_band() {
    let dir = std::env::temp_dir().join(format!("panel-w-{}", std::process::id()));
    for (given, want) in [(12, WIDTH_MIN), (40, 40), (90, WIDTH_MAX)] {
        let (config, notice) = load(&write(&dir, &format!("[panel]\nwidth = {given}\n")));
        assert!(notice.is_none());
        assert_eq!(config.width, want, "width {given}");
    }
}

#[test]
fn the_key_overrides_and_an_empty_one_keeps_the_default() {
    let dir = std::env::temp_dir().join(format!("panel-k-{}", std::process::id()));
    let (config, _) = load(&write(&dir, "[panel]\nkey = \"u\"\n"));
    assert_eq!(config.key, "u");
    let (config, _) = load(&write(&dir, "[panel]\nkey = \"\"\n"));
    assert_eq!(config.key, "y");
}

#[test]
fn an_unparseable_key_falls_back_to_y_with_a_notice() {
    let dir = std::env::temp_dir().join(format!("panel-badkey-{}", std::process::id()));
    let path = write(&dir, "[panel]\nkey = \"nope+\"\n");
    let (config, notice) = load(&path);
    assert_eq!(config.key, "y", "an invalid key keeps the default");
    let notice = notice.expect("the ignored key notices once");
    assert!(notice.contains("config.toml"), "{notice}");
    assert!(notice.contains("panel.key"), "{notice}");
    // The invalid value itself never reaches the screen.
    assert!(!notice.contains("nope"), "{notice}");
}

#[test]
fn a_malformed_file_is_defaults_plus_a_position() {
    let dir = std::env::temp_dir().join(format!("panel-bad-{}", std::process::id()));
    let path = write(&dir, "[panel\nwidth = ");
    let (config, notice) = load(&path);
    assert_eq!(config.width, PanelConfig::default().width);
    let notice = notice.expect("a malformed file notices once");
    // Only the position leaks — never the file's text.
    assert!(notice.contains(':'), "{notice}");
    let tail = notice.rsplit(':').next().unwrap();
    assert!(tail.trim().chars().all(|c| c.is_ascii_digit()), "{notice}");
    assert!(!notice.contains("[panel"), "{notice}");
    assert!(!notice.contains("width"), "{notice}");
}
