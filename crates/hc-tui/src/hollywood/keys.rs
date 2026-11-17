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
