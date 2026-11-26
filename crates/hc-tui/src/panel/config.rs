//! The backlot client's own configuration — separate from herdr's
//! `config.toml`, which is still read exactly as before. Missing file
//! means defaults; a malformed file means defaults plus one notice
//! carrying only the parse position (never file text).

use std::path::{Path, PathBuf};

use crate::config::KeyCombo;

/// `[dock] width` is the dock's expanded width (clamped 28–48,
/// default 36); a `[panel] width` from an older file still feeds it.
/// `[sidebar] width` (clamped 20–60) applies only in studio mode —
/// outside it herdr's `ui.sidebar_width` rules the sidebar exactly as
/// before. `[keys]` binds the four direct studio keys plus the call
/// sheet's own pair, and `[hollywood] start` is the bar's start
/// policy. The dock's toggle key is `y` (chorded as `prefix+y`) from
/// `[panel] key`, or `[keys] prefix_dock` in a newer file.
#[derive(Debug, Clone)]
pub(crate) struct PanelConfig {
    /// Expanded width after clamping.
    pub width: u16,
    /// Whether a `[dock]`/`[panel]` width in the file set `width` —
    /// an explicit width wins over the spec's per-width tier.
    pub width_explicit: bool,
    /// The chord tail — matched like a herdr keybind string.
    pub key: String,
    /// Studio-mode sidebar width after clamping.
    pub sidebar_width: u16,
    /// The four direct studio navigation keys.
    pub keys: StudioKeys,
    /// `[hollywood].start` — when the bar opens by itself.
    pub call_sheet_start: CallSheetStart,
    /// `[keys].call_sheet` raw — `""` turns the direct key off.
    pub keys_call_sheet: Option<String>,
    /// `[keys].prefix_call_sheet` raw — `""` turns the chord off.
    pub keys_prefix_call_sheet: Option<String>,
    /// `[hollywood].key` read leniently — survives a client-only
    /// `[hollywood]` table the stage's strict parser drops.
    pub hollywood_key: Option<String>,
    /// Any `[hollywood]` table exists — the pill shows for it even
    /// without an agent (`runner`/`model`).
    pub hollywood_table: bool,
}

/// The bar's start policy (`[hollywood] start`): the default is
/// `collapsed` — the bar no longer opens expanded at start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CallSheetStart {
    /// Always the pill only — the bar waits for the toggle.
    #[default]
    Collapsed,
    /// Expanded only while U > 0 — and again on the edge up.
    WhenWaiting,
    /// Always expanded at start.
    Expanded,
}

impl CallSheetStart {
    /// The `[hollywood] start` spelling — what the settings write.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Collapsed => "collapsed",
            Self::WhenWaiting => "when_waiting",
            Self::Expanded => "expanded",
        }
    }
}

pub(crate) const WIDTH_MIN: u16 = 28;
pub(crate) const WIDTH_MAX: u16 = 48;
const WIDTH_DEFAULT: u16 = 36;

pub(crate) const SIDEBAR_WIDTH_MIN: u16 = 20;
pub(crate) const SIDEBAR_WIDTH_MAX: u16 = 60;
const SIDEBAR_WIDTH_DEFAULT: u16 = 26;

/// A studio key a `[keys]` entry names — `alt+arrow` by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StudioNav {
    PreviousStudio,
    NextStudio,
    PreviousRole,
    NextRole,
}

const DEFAULT_PREVIOUS_STUDIO: &str = "alt+pageup";
const DEFAULT_NEXT_STUDIO: &str = "alt+pagedown";
const DEFAULT_PREVIOUS_ROLE: &str = "alt+up";
const DEFAULT_NEXT_ROLE: &str = "alt+down";
const DEFAULT_FOCUS_LOT: &str = "alt+l";

/// The direct studio keys: `alt+pageup/pagedown` move between studios,
/// `alt+up/down` between the current studio's people, and `alt+l`
/// focuses the lot on its last row. `None` is an unbound key
/// (`""` in the file).
#[derive(Debug, Clone)]
pub(crate) struct StudioKeys {
    pub previous_studio: Option<KeyCombo>,
    pub next_studio: Option<KeyCombo>,
    pub previous_role: Option<KeyCombo>,
    pub next_role: Option<KeyCombo>,
    pub focus_lot: Option<KeyCombo>,
}

impl Default for StudioKeys {
    fn default() -> StudioKeys {
        StudioKeys {
            previous_studio: crate::config::parse_key_combo(DEFAULT_PREVIOUS_STUDIO),
            next_studio: crate::config::parse_key_combo(DEFAULT_NEXT_STUDIO),
            previous_role: crate::config::parse_key_combo(DEFAULT_PREVIOUS_ROLE),
            next_role: crate::config::parse_key_combo(DEFAULT_NEXT_ROLE),
            focus_lot: crate::config::parse_key_combo(DEFAULT_FOCUS_LOT),
        }
    }
}

impl StudioKeys {
    /// Whether `key` matches the configured `focus_lot` binding.
    pub(crate) fn focus_lot_matches(&self, key: &crate::config::TerminalKey) -> bool {
        self.focus_lot
            .is_some_and(|combo| crate::config::terminal_key_matches_combo(key, combo))
    }

    /// The studio action a key combo resolves to — checked before
    /// herdr's own direct bindings, so a studio key wins in studio
    /// mode even when it shadows one.
    pub(crate) fn nav(&self, key: &crate::config::TerminalKey) -> Option<StudioNav> {
        let matched = |combo: Option<KeyCombo>| {
            combo.is_some_and(|combo| crate::config::terminal_key_matches_combo(key, combo))
        };
        if matched(self.previous_studio) {
            Some(StudioNav::PreviousStudio)
        } else if matched(self.next_studio) {
            Some(StudioNav::NextStudio)
        } else if matched(self.previous_role) {
            Some(StudioNav::PreviousRole)
        } else if matched(self.next_role) {
            Some(StudioNav::NextRole)
        } else {
            None
        }
    }

    /// The footer's two hint halves — `("alt PgUp/PgDn studio", "↑↓ role")`
    /// by default — so the caller can drop the half whose navigation
    /// has nowhere to go (wide-01: `studio` only across ≥2 studios,
    /// `role` only across ≥2 cast entries) or fit them as separate tiers.
    pub(crate) fn shortcut_parts(&self) -> (String, String) {
        use crossterm::event::KeyCode;
        let four = [
            self.previous_studio,
            self.next_studio,
            self.previous_role,
            self.next_role,
        ];
        // A pair of keys that reads well side by side under the same
        // modifier: the four arrows (no gap), and PageUp/PageDown (a
        // slash, so `PgUp/PgDn` never reads as one word).
        let pair_label = |combo: Option<KeyCombo>| {
            combo.and_then(|(code, _)| match code {
                KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => {
                    Some((code_label(code), ""))
                }
                KeyCode::PageUp => Some(("PgUp".to_string(), "/")),
                KeyCode::PageDown => Some(("PgDn".to_string(), "/")),
                _ => None,
            })
        };
        let mods = four
            .iter()
            .filter_map(|combo| combo.map(|c| c.1))
            .fold(None, |seen, mods| match seen {
                None => Some(mods),
                Some(seen) if seen == mods => Some(seen),
                Some(_) => Some(crossterm::event::KeyModifiers::empty()),
            });
        if let (Some(ps), Some(ns), Some(pr), Some(nr)) = (
            pair_label(self.previous_studio),
            pair_label(self.next_studio),
            pair_label(self.previous_role),
            pair_label(self.next_role),
        ) && let Some(mods) = mods.filter(|m| !m.is_empty())
        {
            return (
                format!("{} {}{}{} studio", mods_label(mods), ps.0, ps.1, ns.0),
                format!("{}{}{} role", pr.0, pr.1, nr.0),
            );
        }
        let pair = |a: Option<KeyCombo>, b: Option<KeyCombo>| {
            let labels: Vec<String> = [a, b]
                .into_iter()
                .flatten()
                .map(|(code, mods)| format!("{}{}", mods_label(mods), code_label(code)))
                .collect();
            if labels.is_empty() {
                "—".to_string()
            } else {
                labels.join("/")
            }
        };
        (
            format!("{} studio", pair(self.previous_studio, self.next_studio)),
            format!("{} role", pair(self.previous_role, self.next_role)),
        )
    }

    /// The unfocused footer's width tiers (R3-M13 / wide-01): the
    /// combined hint, then each surviving half alone — nothing when
    /// neither navigation has anywhere to go.
    pub(crate) fn footer_tiers(&self, studio_nav: bool, role_nav: bool) -> Vec<String> {
        let (studio, role) = self.shortcut_parts();
        let mut tiers = Vec::new();
        if studio_nav && role_nav {
            tiers.push(format!("{studio}  {role}"));
        }
        if studio_nav {
            tiers.push(studio);
        }
        if role_nav {
            tiers.push(role);
        }
        tiers
    }
}

/// A modifier set's label for the shortcuts row (`alt`, `ctrl`, …).
pub(crate) fn mods_label(mods: crossterm::event::KeyModifiers) -> &'static str {
    use crossterm::event::KeyModifiers as M;
    match mods {
        m if m == M::ALT => "alt",
        m if m == M::CONTROL => "ctrl",
        m if m == M::SHIFT => "shift",
        m if m == (M::CONTROL | M::ALT) => "ctrl+alt",
        m if m == (M::CONTROL | M::SHIFT) => "ctrl+shift",
        m if m == (M::ALT | M::SHIFT) => "alt+shift",
        m if m == M::SUPER => "super",
        _ => "mod",
    }
}

/// A key code's label for the pair fallback (`↑`, `pageup`, `x`, …).
pub(crate) fn code_label(code: crossterm::event::KeyCode) -> String {
    use crossterm::event::KeyCode;
    match code {
        KeyCode::Up => "↑".to_string(),
        KeyCode::Down => "↓".to_string(),
        KeyCode::Left => "←".to_string(),
        KeyCode::Right => "→".to_string(),
        KeyCode::Char(c) => c.to_string(),
        other => format!("{other:?}").to_lowercase(),
    }
}

impl Default for PanelConfig {
    fn default() -> PanelConfig {
        PanelConfig {
            width: WIDTH_DEFAULT,
            width_explicit: false,
            key: "y".into(),
            sidebar_width: SIDEBAR_WIDTH_DEFAULT,
            keys: StudioKeys::default(),
            call_sheet_start: CallSheetStart::default(),
            keys_call_sheet: None,
            keys_prefix_call_sheet: None,
            hollywood_key: None,
            hollywood_table: false,
        }
    }
}

#[derive(serde::Deserialize)]
struct File {
    panel: Option<Section>,
    sidebar: Option<SidebarSection>,
    keys: Option<KeysSection>,
    dock: Option<DockSection>,
    hollywood: Option<HollywoodSection>,
}

#[derive(serde::Deserialize)]
struct Section {
    width: Option<i64>,
    key: Option<String>,
}

#[derive(serde::Deserialize)]
struct SidebarSection {
    width: Option<i64>,
}

#[derive(serde::Deserialize)]
struct DockSection {
    width: Option<i64>,
}

/// `[hollywood]` read leniently — only the client's own keys. A
/// client-only table (`key`/`start` without `runner`) parses fine
/// here even though the stage's strict reader skips it.
#[derive(serde::Deserialize)]
struct HollywoodSection {
    start: Option<String>,
    key: Option<String>,
}

#[derive(serde::Deserialize)]
struct KeysSection {
    previous_studio: Option<String>,
    next_studio: Option<String>,
    previous_role: Option<String>,
    next_role: Option<String>,
    /// `prev_studio`/`prev_role` spellings from the spec's table.
    prev_studio: Option<String>,
    prev_role: Option<String>,
    focus_lot: Option<String>,
    call_sheet: Option<String>,
    prefix_call_sheet: Option<String>,
    prefix_dock: Option<String>,
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

/// One studio key value: missing → the default; `""` → unbound; a
/// value that does not parse → the default and the file's one notice.
fn studio_key(
    raw: Option<String>,
    default: &str,
    name: &str,
    path: &Path,
    notice: &mut Option<String>,
) -> Option<KeyCombo> {
    match raw {
        None => crate::config::parse_key_combo(default),
        Some(value) if value.trim().is_empty() => None,
        Some(value) => match crate::config::parse_key_combo(&value) {
            Some(combo) => Some(combo),
            None => {
                *notice = Some(format!("{path:?} ignored: invalid keys.{name}"));
                crate::config::parse_key_combo(default)
            }
        },
    }
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
            config.width_explicit = true;
        }
        if let Some(key) = panel.key.filter(|k| !k.trim().is_empty()) {
            if crate::config::parse_key_combo(&key).is_some() {
                config.key = key;
            } else {
                // A key that does not parse falls back to `y` — the
                // notice names the file, never the value.
                return (config, Some(format!("{path:?} ignored: invalid panel.key")));
            }
        }
    }
    if let Some(width) = parsed.dock.and_then(|d| d.width) {
        config.width = width.clamp(1, i64::from(u16::MAX)) as u16;
        config.width_explicit = true;
    }
    config.width = config.width.clamp(WIDTH_MIN, WIDTH_MAX);
    if let Some(width) = parsed.sidebar.and_then(|s| s.width) {
        config.sidebar_width = (width.clamp(1, i64::from(u16::MAX)) as u16)
            .clamp(SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX);
    }
    if let Some(hollywood) = parsed.hollywood {
        config.hollywood_table = true;
        config.hollywood_key = hollywood.key;
        if let Some(start) = hollywood.start.filter(|s| !s.trim().is_empty()) {
            match start.trim() {
                "collapsed" => config.call_sheet_start = CallSheetStart::Collapsed,
                "when_waiting" => config.call_sheet_start = CallSheetStart::WhenWaiting,
                "expanded" => config.call_sheet_start = CallSheetStart::Expanded,
                _ => {
                    return (
                        config,
                        Some(format!("{path:?} ignored: invalid hollywood.start")),
                    );
                }
            }
        }
    }
    if let Some(keys) = parsed.keys {
        let mut notice = None;
        config.keys.previous_studio = studio_key(
            keys.previous_studio.or(keys.prev_studio),
            DEFAULT_PREVIOUS_STUDIO,
            "previous_studio",
            path,
            &mut notice,
        );
        config.keys.next_studio = studio_key(
            keys.next_studio,
            DEFAULT_NEXT_STUDIO,
            "next_studio",
            path,
            &mut notice,
        );
        config.keys.previous_role = studio_key(
            keys.previous_role.or(keys.prev_role),
            DEFAULT_PREVIOUS_ROLE,
            "previous_role",
            path,
            &mut notice,
        );
        config.keys.next_role = studio_key(
            keys.next_role,
            DEFAULT_NEXT_ROLE,
            "next_role",
            path,
            &mut notice,
        );
        config.keys.focus_lot = studio_key(
            keys.focus_lot,
            DEFAULT_FOCUS_LOT,
            "focus_lot",
            path,
            &mut notice,
        );
        if notice.is_some() {
            return (config, notice);
        }
        config.keys_call_sheet = keys.call_sheet;
        config.keys_prefix_call_sheet = keys.prefix_call_sheet;
        if let Some(tail) = keys.prefix_dock.filter(|k| !k.trim().is_empty()) {
            if crate::config::parse_key_combo(&tail).is_some() {
                config.key = tail;
            } else {
                return (
                    config,
                    Some(format!("{path:?} ignored: invalid keys.prefix_dock")),
                );
            }
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
