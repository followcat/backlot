//! The layer's toggle binding (D15a): `alt+y` direct by default, a
//! `[hollywood] key` value overriding it as a direct binding
//! (`alt+y`) or a chord (`prefix+h`). A value that does not parse, a
//! direct binding on an unmodified printable key, or a binding that
//! collides with a `[keys]` studio key (direct or chord tail) or the
//! panel's chord tail is refused: the notice names the file and never
//! echoes the value, and the binding falls back to `alt+y`.

use std::path::Path;

use crate::config::KeyCombo;
use crate::panel::StudioKeys;

/// The resolved binding — matched like the studio keys are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HollywoodKey {
    /// A direct binding with no prefix — `alt+y` by default.
    Direct(KeyCombo),
    /// A `prefix+` chord — the tail combo matched while armed.
    Chord(KeyCombo),
}

impl HollywoodKey {
    /// The fallback: `alt+y` parses unconditionally.
    pub(crate) fn fallback() -> HollywoodKey {
        HollywoodKey::Direct(crate::config::parse_key_combo("alt+y").expect("alt+y parses"))
    }
}

/// The call sheet's resolved pair: the direct binding (`alt+y` by
/// default) and the prefix chord tail (`i` by default). Each is its
/// own slot — `[keys] call_sheet` owns the direct one, `[keys]
/// prefix_call_sheet` the chord — and `""` turns one off without
/// touching the other. `[hollywood] key` still fills whichever slots
/// the `[keys]` table leaves open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CallSheetKeys {
    /// A direct binding with no prefix — `None` is off.
    pub direct: Option<KeyCombo>,
    /// A chord tail matched while the prefix is armed.
    pub chord: Option<KeyCombo>,
}

impl CallSheetKeys {
    /// The label for hints and the help overlay: `alt+y / ctrl+b i`
    /// when both bind, the bound one alone otherwise. `prefix` is the
    /// configured prefix's label.
    pub(crate) fn label(&self, prefix: &str) -> String {
        let mut parts = Vec::new();
        if let Some(direct) = self.direct {
            parts.push(crate::config::format_key_combo(direct));
        }
        if let Some(chord) = self.chord {
            parts.push(format!(
                "{prefix} {}",
                crate::config::format_key_combo(chord)
            ));
        }
        if parts.is_empty() {
            "—".to_string()
        } else {
            parts.join(" / ")
        }
    }

    /// Whether the press is this pair: the direct combo always, the
    /// chord tail only while the prefix is armed.
    pub(crate) fn press(&self, matched: &crate::config::TerminalKey, armed: bool) -> bool {
        use crate::config::terminal_key_matches_combo as is;
        self.direct.is_some_and(|c| is(matched, c))
            || (armed && self.chord.is_some_and(|c| is(matched, c)))
    }
}

/// What a slot resolution shares: the collision inputs and where
/// the refusal notices land.
struct SlotCx<'a> {
    keys: &'a StudioKeys,
    panel_tail: Option<KeyCombo>,
    path: &'a Path,
    notices: &'a mut Vec<String>,
}

impl SlotCx<'_> {
    /// A refused value: the notice names the file and the key, never
    /// echoes the value, and the slot falls back to its default.
    fn refuse(&mut self, name: &str, why: &str, default: &str) -> Option<KeyCombo> {
        self.notices
            .push(format!("{:?} ignored: invalid {name} ({why})", self.path));
        crate::config::parse_key_combo(default)
    }
}

/// One slot's resolution: `""` is off, an explicit value validates,
/// absent falls back to `[hollywood] key`'s form then the default.
fn slot(
    raw: Option<&str>,
    fallback: HollywoodKey,
    default: &str,
    chord: bool,
    cx: &mut SlotCx<'_>,
) -> Option<KeyCombo> {
    let Some(raw) = raw else {
        return match fallback {
            HollywoodKey::Direct(c) if !chord => Some(c),
            HollywoodKey::Chord(c) if chord => Some(c),
            _ => crate::config::parse_key_combo(default),
        };
    };
    if raw.trim().is_empty() {
        return None;
    }
    let name = if chord {
        "keys.prefix_call_sheet"
    } else {
        "keys.call_sheet"
    };
    let Some(combo) = crate::config::parse_key_combo(raw.trim()) else {
        return cx.refuse(name, "unparseable", default);
    };
    if !chord && crate::config::is_unmodified_printable(combo) {
        return cx.refuse(name, "a bare printable key types", default);
    }
    if let Some(hit) = collides_with_keys(combo, cx.keys) {
        return cx.refuse(name, &format!("collides with keys.{hit}"), default);
    }
    if chord && cx.panel_tail == Some(combo) {
        return cx.refuse(name, "collides with the panel chord", default);
    }
    Some(combo)
}

/// Resolve the toggle's pair: `[keys] call_sheet` and
/// `[keys] prefix_call_sheet`, each slot falling back to the matching
/// form of `[hollywood] key`, then to `alt+y` / `i`.
pub(crate) fn resolve_toggle(
    direct_raw: Option<&str>,
    chord_raw: Option<&str>,
    hollywood_raw: Option<&str>,
    keys: &StudioKeys,
    panel_tail: Option<KeyCombo>,
    path: &Path,
) -> (CallSheetKeys, Vec<String>) {
    let (hollywood_key, notice) = resolve_key(hollywood_raw, keys, panel_tail, path);
    let mut notices = notice.into_iter().collect::<Vec<_>>();
    let mut cx = SlotCx {
        keys,
        panel_tail,
        path,
        notices: &mut notices,
    };
    let direct = slot(direct_raw, hollywood_key, "alt+y", false, &mut cx);
    let chord = slot(chord_raw, hollywood_key, "i", true, &mut cx);
    (CallSheetKeys { direct, chord }, notices)
}

/// Whether `combo` is one of the four `[keys]` studio bindings —
/// checked the same for a direct binding and a chord tail: the studio
/// keys intercept before the prefix machine sees the tail, so a
/// colliding tail would never reach the chord.
fn collides_with_keys(combo: KeyCombo, keys: &StudioKeys) -> Option<&'static str> {
    let hit = |c: Option<KeyCombo>| c == Some(combo);
    if hit(keys.previous_studio) {
        Some("previous_studio")
    } else if hit(keys.next_studio) {
        Some("next_studio")
    } else if hit(keys.previous_role) {
        Some("previous_role")
    } else if hit(keys.next_role) {
        Some("next_role")
    } else if hit(keys.focus_lot) {
        Some("focus_lot")
    } else {
        None
    }
}

/// Resolve `[hollywood] key` against the studio keys and the panel's
/// chord tail. `(binding, notice)` — a refused value falls back to
/// `alt+y` so a valid `[hollywood]` section keeps its toggle.
pub(crate) fn resolve_key(
    raw: Option<&str>,
    keys: &StudioKeys,
    panel_tail: Option<KeyCombo>,
    path: &Path,
) -> (HollywoodKey, Option<String>) {
    let Some(raw) = raw.filter(|k| !k.trim().is_empty()) else {
        return (HollywoodKey::fallback(), None);
    };
    let notice = |why: &str| Some(format!("{path:?} ignored: invalid hollywood.key ({why})"));
    let (chord, tail) = match raw.trim().strip_prefix("prefix+") {
        Some(rest) => (true, rest),
        None => (false, raw.trim()),
    };
    let Some(combo) = crate::config::parse_key_combo(tail) else {
        return (HollywoodKey::fallback(), notice("unparseable"));
    };
    if !chord && crate::config::is_unmodified_printable(combo) {
        return (
            HollywoodKey::fallback(),
            notice("a bare printable key types"),
        );
    }
    if let Some(name) = collides_with_keys(combo, keys) {
        return (
            HollywoodKey::fallback(),
            notice(&format!("collides with keys.{name}")),
        );
    }
    if chord && panel_tail == Some(combo) {
        return (
            HollywoodKey::fallback(),
            notice("collides with the panel chord"),
        );
    }
    let key = if chord {
        HollywoodKey::Chord(combo)
    } else {
        HollywoodKey::Direct(combo)
    };
    (key, None)
}
