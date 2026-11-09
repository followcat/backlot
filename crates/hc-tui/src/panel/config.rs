//! The backlot client's own configuration — separate from herdr's
//! `config.toml`, which is still read exactly as before. Missing file
//! means defaults; a malformed file means defaults plus one notice
//! carrying only the parse position (never file text).

use std::path::{Path, PathBuf};

/// `[panel]` defaults: expanded width 30 (clamped 24–60), toggle key
/// `y` (chorded as `prefix+y`; the reference leaves `y` unbound).
#[derive(Debug, Clone)]
pub(crate) struct PanelConfig {
    /// Expanded width after clamping.
    pub width: u16,
    /// The chord tail — matched like a herdr keybind string.
    pub key: String,
}

pub(crate) const WIDTH_MIN: u16 = 24;
pub(crate) const WIDTH_MAX: u16 = 60;
const WIDTH_DEFAULT: u16 = 30;

impl Default for PanelConfig {
    fn default() -> PanelConfig {
        PanelConfig {
            width: WIDTH_DEFAULT,
            key: "y".into(),
        }
    }
}

#[derive(serde::Deserialize)]
struct File {
    panel: Option<Section>,
}

#[derive(serde::Deserialize)]
struct Section {
    width: Option<i64>,
    key: Option<String>,
}

/// The config's own path: `$XDG_CONFIG_HOME/backlot/config.toml`,
/// `~/.config/backlot/config.toml` without it.
pub(crate) fn config_path(env: &hc_proto::endpoint::Env) -> PathBuf {
    let get = |name: &str| {
        env.get(std::ffi::OsStr::new(name))
            .and_then(|v| v.clone().into_string().ok())
            .filter(|v| !v.is_empty())
    };
    let home = get("HOME").unwrap_or_else(|| "/".into());
    let root = get("XDG_CONFIG_HOME").unwrap_or_else(|| format!("{home}/.config"));
    PathBuf::from(root).join("backlot").join("config.toml")
}

/// Load the file: `(config, notice)`. A missing file is defaults. A
/// malformed file is defaults plus one notice naming only the line
/// and column — the file's text never reaches the screen.
pub(crate) fn load(path: &Path) -> (PanelConfig, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return (PanelConfig::default(), None), // missing is defaults
    };
    let parsed: File = match toml::from_str(&text) {
        Ok(file) => file,
        Err(e) => {
            let notice = e
                .span()
                .map(|span| position_of(&text, span.start))
                .map(|(line, col)| format!("{path:?} ignored: invalid TOML at {line}:{col}"))
                .unwrap_or_else(|| format!("{path:?} ignored: invalid TOML"));
            return (PanelConfig::default(), Some(notice));
        }
    };
    let mut config = PanelConfig::default();
    if let Some(panel) = parsed.panel {
        if let Some(width) = panel.width {
            config.width = width.clamp(1, i64::from(u16::MAX)) as u16;
            config.width = config.width.clamp(WIDTH_MIN, WIDTH_MAX);
        }
        if let Some(key) = panel.key.filter(|k| !k.trim().is_empty()) {
            config.key = key;
        }
    }
    (config, None)
}

/// A byte offset's 1-based `(line, column)` in `text`.
fn position_of(text: &str, byte: usize) -> (usize, usize) {
    let head = &text[..byte.min(text.len())];
    let line = head.chars().filter(|c| *c == '\n').count() + 1;
    let col = head.rsplit('\n').next().unwrap_or(head).chars().count() + 1;
    (line, col)
}
