//! backlot's own look. hc-tui takes `[theme]` from herdr's `config.toml`;
//! backlot has its own `[theme]` in `~/.config/backlot/config.toml`, with
//! the built-in `backlot` theme as the default, so the two clients are
//! told apart at a glance. `[theme] source = "herdr"` gives the look of
//! hc-tui back: herdr's `[theme]` is then read exactly as before.
//!
//! The hooks are small on purpose: `overlay` runs after every load of
//! herdr's config (start and reload) and rewrites `config.theme`; the
//! Settings overlay reads its list and its save path from `Shell`.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use ratatui::style::Color;

use crate::chrome::palette::Palette;
use crate::config::{LoadedConfig, THEME_NAMES, ThemeConfig};

/// The built-in theme's name, and the default of backlot's `[theme] name`.
pub(crate) const NAME: &str = "backlot";

/// The Settings list while backlot's own `[theme]` is in force:
/// `backlot` first, then the reference's themes in their order.
static OWN_THEME_NAMES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    std::iter::once(NAME)
        .chain(THEME_NAMES.iter().copied())
        .collect()
});

impl Palette {
    /// backlot — a premiere at night: marquee gold on midnight blue,
    /// cream text. The default of the backlot client.
    pub fn backlot() -> Self {
        Self {
            accent: Color::Rgb(240, 178, 74),
            panel_bg: Color::Rgb(17, 20, 29),
            // Opaque and a step darker than the panes: in a
            // translucent terminal the sidebar must still be a surface
            // of its own, not a window onto what is behind it.
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
        }
    }
}

/// What the shell keeps between loads.
#[derive(Debug, Clone, Default)]
pub struct Shell {
    /// backlot's config file. `None` until the client sets it (tests,
    /// embedders): herdr's `[theme]` is then read as hc-tui reads it.
    pub(crate) path: Option<PathBuf>,
    /// True while backlot's own `[theme]` is in force.
    pub(crate) own: bool,
    /// True when `[theme] source = "herdr"` was configured explicitly.
    pub(crate) source_herdr: bool,
}

impl Shell {
    /// A shell that reads backlot's `[theme]` from `path`.
    pub(crate) fn at(path: PathBuf) -> Shell {
        Shell {
            path: Some(path),
            own: false,
            source_herdr: false,
        }
    }

    /// The Settings overlay's theme list.
    pub(crate) fn names(&self) -> &'static [&'static str] {
        if self.own {
            OWN_THEME_NAMES.as_slice()
        } else {
            THEME_NAMES
        }
    }

    /// The file a theme picked in Settings is saved to: backlot's own
    /// while its `[theme]` is in force, herdr's otherwise.
    pub(crate) fn save_path<'a>(&'a self, herdr: &'a Path) -> &'a Path {
        match (&self.path, self.own) {
            (Some(path), true) => path,
            _ => herdr,
        }
    }

    /// Run after every load of herdr's config, before it is projected:
    /// reads backlot's file again and puts its `[theme]` over herdr's.
    /// Returns the state for the shell built from `loaded`.
    pub(crate) fn overlay(&self, loaded: &mut LoadedConfig) -> Shell {
        let Some(path) = &self.path else {
            return self.clone();
        };
        let (own, notice) = read(path);
        let Some(theme) = own else {
            // `source = "herdr"`: nothing of herdr's load is touched.
            return Shell {
                path: Some(path.clone()),
                own: false,
                source_herdr: true,
            };
        };
        // herdr's `[theme]` is out of play: neither its diagnostics nor
        // its `invalid` mark, nor the legacy `ui.accent`, reach the theme.
        loaded
            .diagnostics
            .retain(|line| !line.starts_with("unknown theme name "));
        loaded.invalid_sections.retain(|name| name != "theme");
        loaded.diagnostics.extend(notice);
        loaded.diagnostics.extend(theme.diagnostics());
        loaded.config.theme = theme;
        loaded.config.ui.accent = "cyan".into();
        Shell {
            path: Some(path.clone()),
            own: true,
            source_herdr: false,
        }
    }
}

/// backlot's `[theme]`: `None` when it says `source = "herdr"`. A
/// missing file, a file with no `[theme]`, and a `[theme]` that cannot
/// be used all give the `backlot` theme; the last adds one notice,
/// which never carries the file's text.
fn read(path: &Path) -> (Option<ThemeConfig>, Option<String>) {
    let default = || ThemeConfig {
        name: Some(NAME.into()),
        ..ThemeConfig::default()
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return (Some(default()), None);
    };
    // A file that is not TOML at all is already reported by the panel's
    // own load of the same file.
    let Ok(file) = text.parse::<toml::Value>() else {
        return (Some(default()), None);
    };
    let Some(table) = file.get("theme") else {
        return (Some(default()), None);
    };
    let unusable = || Some(format!("{path:?}: [theme] is not valid; using \"{NAME}\""));
    let mut notice = None;
    match table.get("source") {
        None => {}
        Some(toml::Value::String(source)) if source == "herdr" => return (None, None),
        Some(toml::Value::String(source)) if source == NAME => {}
        Some(_) => {
            notice = Some(
                "theme.source must be \"backlot\" or \"herdr\"; using \"backlot\"".to_string(),
            );
        }
    }
    match table.clone().try_into::<ThemeConfig>() {
        Ok(mut theme) => {
            if theme.name.is_none() {
                theme.name = Some(NAME.into());
            }
            (Some(theme), notice)
        }
        Err(_) => (Some(default()), unusable()),
    }
}

#[cfg(test)]
#[path = "own_theme_tests.rs"]
mod tests;
