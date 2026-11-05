// Derived from herdr src/config/keybinds.rs (Apache-2.0).
use super::*;

#[derive(Clone)]
pub(crate) enum ParsedBinding {
    Single(ResolvedBinding),
    Range(Vec<ResolvedBinding>),
}

pub(crate) fn parse_binding_string(raw: &str) -> Option<ParsedBinding> {
    let trimmed = raw.trim();
    let (trigger_prefix, body) = if let Some(rest) = trimmed.strip_prefix("prefix+") {
        (true, rest)
    } else {
        (false, trimmed)
    };

    if let Some(range_modifiers) = parse_range_modifiers(body) {
        let bindings = (1..=9)
            .map(|idx| {
                let combo = (
                    KeyCode::Char(char::from_digit(idx, 10).unwrap_or('1')),
                    range_modifiers,
                );
                let key_label = format_key_combo(combo);
                ResolvedBinding {
                    trigger: if trigger_prefix {
                        BindingTrigger::Prefix(combo)
                    } else {
                        BindingTrigger::Direct(combo)
                    },
                    label: if trigger_prefix {
                        format!("prefix+{key_label}")
                    } else {
                        key_label
                    },
                }
            })
            .collect();
        return Some(ParsedBinding::Range(bindings));
    }

    let combo = parse_key_combo(body)?;
    let label = if trigger_prefix {
        format!("prefix+{}", format_key_combo(combo))
    } else {
        format_key_combo(combo)
    };
    Some(ParsedBinding::Single(ResolvedBinding {
        trigger: if trigger_prefix {
            BindingTrigger::Prefix(combo)
        } else {
            BindingTrigger::Direct(combo)
        },
        label,
    }))
}

/// Parse the configured prefix keys.
/// The reference `parse_prefix_keys`: parsed combos, an optional rejection
/// diagnostic when nothing parses (fallback applies), and per-entry
/// diagnostics for unparseable values.
pub(crate) fn parse_prefix_keys(
    config: &BindingConfig,
) -> (Vec<KeyCombo>, Option<String>, Vec<String>) {
    let mut combos: Vec<KeyCombo> = Vec::new();
    let mut diagnostics = Vec::new();

    for raw in config.values() {
        let raw = raw.trim();
        match parse_key_combo(raw) {
            Some(combo) if !combos.contains(&combo) => combos.push(combo),
            Some(_) => {}
            None => diagnostics.push(format!(
                "invalid keybinding: keys.prefix = {raw:?}; ignoring prefix"
            )),
        }
    }

    if combos.is_empty() {
        let reject = Some(format!(
            "invalid keybinding: keys.prefix; using fallback {}",
            format_key_combo(DEFAULT_PREFIX)
        ));
        return (vec![DEFAULT_PREFIX], reject, diagnostics);
    }

    (combos, None, diagnostics)
}

/// A direct binding on an unmodified printable key would intercept typing.
pub(crate) fn is_unmodified_printable(combo: KeyCombo) -> bool {
    matches!(combo.0, KeyCode::Char(ch) if !ch.is_control())
        && combo.1.difference(KeyModifiers::SHIFT).is_empty()
}

pub fn format_key_combo(binding: KeyCombo) -> String {
    let (code, modifiers) = binding;
    let mut parts = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".to_string());
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt".to_string());
    }
    if modifiers.contains(KeyModifiers::SHIFT) && !matches!(code, KeyCode::BackTab) {
        parts.push("shift".to_string());
    }
    if modifiers.contains(KeyModifiers::SUPER) {
        parts.push("super".to_string());
    }
    if modifiers.contains(KeyModifiers::HYPER) {
        parts.push("hyper".to_string());
    }
    if modifiers.contains(KeyModifiers::META) {
        parts.push("meta".to_string());
    }

    let key = match code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::BackTab => "shift+tab".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        _ => format!("{:?}", code).to_lowercase(),
    };

    if matches!(code, KeyCode::BackTab) {
        return if parts.is_empty() {
            key
        } else {
            format!("{}+{key}", parts.join("+"))
        };
    }

    parts.push(key);
    parts.join("+")
}

pub(super) fn parse_modifier_token(token: &str) -> Option<KeyModifiers> {
    match token.to_lowercase().as_str() {
        "ctrl" | "control" => Some(KeyModifiers::CONTROL),
        "shift" => Some(KeyModifiers::SHIFT),
        "alt" | "option" | "meta" => Some(KeyModifiers::ALT),
        "cmd" | "command" | "super" => Some(KeyModifiers::SUPER),
        "hyper" => Some(KeyModifiers::HYPER),
        _ => None,
    }
}

pub(super) fn parse_range_modifiers(s: &str) -> Option<KeyModifiers> {
    let mut modifiers = KeyModifiers::empty();
    let mut saw_range = false;
    for part in s.split('+') {
        let trimmed = part.trim();
        if trimmed == "1..9" {
            if saw_range {
                return None;
            }
            saw_range = true;
        } else {
            modifiers |= parse_modifier_token(trimmed)?;
        }
    }
    saw_range.then_some(modifiers)
}

pub(super) fn parse_modifier_combo(s: &str) -> Option<KeyModifiers> {
    let mut modifiers = KeyModifiers::empty();
    let parts: Vec<&str> = s.split('+').collect();
    if parts.is_empty() {
        return None;
    }

    for part in &parts {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            return None;
        }
        modifiers |= parse_modifier_token(trimmed)?;
    }

    if modifiers.is_empty() {
        None
    } else {
        Some(modifiers)
    }
}

pub(crate) fn parse_key_combo(s: &str) -> Option<KeyCombo> {
    let s = s.trim();
    // Labels are also used in endpoint profiles and command manifests. A literal
    // plus is the key after the final separator, not an empty key token.
    if s == "+" {
        return Some((KeyCode::Char('+'), KeyModifiers::empty()));
    }
    if let Some(modifiers) = s.strip_suffix("++") {
        return Some((KeyCode::Char('+'), parse_modifier_combo(modifiers)?));
    }
    let parts: Vec<&str> = s.split('+').collect();
    let mut modifiers = KeyModifiers::empty();
    let mut key_str: Option<&str> = None;

    for part in &parts {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Some(modifier) = parse_modifier_token(trimmed) {
            modifiers |= modifier;
        } else if key_str.is_some() {
            return None;
        } else {
            key_str = Some(trimmed);
        }
    }

    let key_str = key_str?;
    let single_char = single_key_char(key_str);
    let lower = key_str.to_lowercase();
    let code = match lower.as_str() {
        "space" | " " => KeyCode::Char(' '),
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" if modifiers.contains(KeyModifiers::SHIFT) => {
            modifiers.remove(KeyModifiers::SHIFT);
            KeyCode::BackTab
        }
        "tab" => KeyCode::Tab,
        "backspace" | "bs" => KeyCode::Backspace,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "minus" => KeyCode::Char('-'),
        "comma" => KeyCode::Char(','),
        "period" => KeyCode::Char('.'),
        "slash" => KeyCode::Char('/'),
        "backslash" => KeyCode::Char('\\'),
        "quote" => KeyCode::Char('\''),
        "double_quote" | "double-quote" => KeyCode::Char('"'),
        "semicolon" => KeyCode::Char(';'),
        "colon" => KeyCode::Char(':'),
        "percent" => KeyCode::Char('%'),
        "ampersand" => KeyCode::Char('&'),
        "backtick" => KeyCode::Char('`'),
        "plus" => KeyCode::Char('+'),
        _ if single_char.is_some() => {
            let ch = single_char?;
            if ch.is_ascii_uppercase() {
                modifiers |= KeyModifiers::SHIFT;
                KeyCode::Char(ch.to_ascii_lowercase())
            } else {
                KeyCode::Char(ch)
            }
        }
        s if s.starts_with('f') => s[1..].parse::<u8>().ok().map(KeyCode::F)?,
        _ => return None,
    };

    Some(normalize_key_combo((code, modifiers)))
}

pub(super) fn single_key_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let ch = chars.next()?;
    if chars.next().is_none() {
        Some(ch)
    } else {
        None
    }
}
