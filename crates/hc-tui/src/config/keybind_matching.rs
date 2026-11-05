// Derived from herdr src/config/keybinds.rs (Apache-2.0).
use super::*;

pub fn normalize_key_combo((mut code, mut modifiers): KeyCombo) -> KeyCombo {
    if matches!(code, KeyCode::Tab) && modifiers.contains(KeyModifiers::SHIFT) {
        code = KeyCode::BackTab;
        modifiers.remove(KeyModifiers::SHIFT);
    } else if matches!(code, KeyCode::BackTab) {
        modifiers.remove(KeyModifiers::SHIFT);
    }
    (code, modifiers)
}

pub fn terminal_key_matches_combo(key: &TerminalKey, combo: KeyCombo) -> bool {
    key_parts_match_combo(key.code, key.modifiers, key.shifted_codepoint, combo)
}

pub(super) fn key_parts_match_combo(
    actual_code: KeyCode,
    actual_modifiers: KeyModifiers,
    shifted_codepoint: Option<u32>,
    combo: KeyCombo,
) -> bool {
    let (actual_code, actual_modifiers) = normalize_key_combo((actual_code, actual_modifiers));
    let (expected_code, expected_modifiers) = normalize_key_combo(combo);

    if actual_modifiers == expected_modifiers
        && key_codes_match(
            actual_code,
            actual_modifiers,
            expected_code,
            expected_modifiers,
            shifted_codepoint,
        )
    {
        return true;
    }

    let actual_without_shift = actual_modifiers.difference(KeyModifiers::SHIFT);
    actual_modifiers.contains(KeyModifiers::SHIFT)
        && actual_without_shift == expected_modifiers
        && shifted_char_matches_expected(actual_code, shifted_codepoint, expected_code)
        || legacy_shifted_ascii_letter_matches(
            actual_code,
            actual_modifiers,
            expected_code,
            expected_modifiers,
        )
}

pub(super) fn key_codes_match(
    actual: KeyCode,
    actual_modifiers: KeyModifiers,
    expected: KeyCode,
    expected_modifiers: KeyModifiers,
    shifted_codepoint: Option<u32>,
) -> bool {
    match (actual, expected) {
        (KeyCode::Char(actual), KeyCode::Char(expected))
            if actual.is_ascii_alphabetic() && expected.is_ascii_alphabetic() =>
        {
            actual == expected
                || actual_modifiers.contains(KeyModifiers::SHIFT)
                    && expected_modifiers.contains(KeyModifiers::SHIFT)
                    && actual.eq_ignore_ascii_case(&expected)
        }
        (KeyCode::Char(actual), KeyCode::Char(expected)) => {
            actual == expected
                || shifted_char_matches_expected(
                    KeyCode::Char(actual),
                    shifted_codepoint,
                    KeyCode::Char(expected),
                )
        }
        (actual, expected) => actual == expected,
    }
}

pub(super) fn legacy_shifted_ascii_letter_matches(
    actual_code: KeyCode,
    actual_modifiers: KeyModifiers,
    expected_code: KeyCode,
    expected_modifiers: KeyModifiers,
) -> bool {
    if actual_modifiers.contains(KeyModifiers::SHIFT) {
        return false;
    }
    let (KeyCode::Char(actual), KeyCode::Char(expected)) = (actual_code, expected_code) else {
        return false;
    };
    actual.is_ascii_uppercase()
        && expected.is_ascii_lowercase()
        && actual.to_ascii_lowercase() == expected
        && actual_modifiers | KeyModifiers::SHIFT == expected_modifiers
}

const SHIFTED_NUMBER_SYMBOLS: [(char, char); 9] = [
    ('1', '!'),
    ('2', '@'),
    ('3', '#'),
    ('4', '$'),
    ('5', '%'),
    ('6', '^'),
    ('7', '&'),
    ('8', '*'),
    ('9', '('),
];

pub(super) fn shifted_number_symbol(ch: char) -> Option<char> {
    SHIFTED_NUMBER_SYMBOLS
        .iter()
        .find_map(|(number, symbol)| (*symbol == ch).then_some(*number))
}

pub(super) fn indexed_shifted_number_matches(
    key: &TerminalKey,
    combo: KeyCombo,
    number: char,
) -> bool {
    let (expected_code, expected_modifiers) = normalize_key_combo(combo);
    matches!(expected_code, KeyCode::Char(expected) if expected == number)
        && expected_modifiers.contains(KeyModifiers::SHIFT)
        && key.modifiers == expected_modifiers.difference(KeyModifiers::SHIFT)
}

pub(super) fn shifted_char_matches_expected(
    actual_code: KeyCode,
    shifted_codepoint: Option<u32>,
    expected_code: KeyCode,
) -> bool {
    let KeyCode::Char(expected) = expected_code else {
        return false;
    };
    if let Some(shifted) = shifted_codepoint.and_then(char::from_u32) {
        return shifted == expected;
    }
    matches!(actual_code, KeyCode::Char(actual) if actual == expected && is_shifted_punctuation(expected))
}

pub(super) fn is_shifted_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '!' | '@'
            | '#'
            | '$'
            | '%'
            | '^'
            | '&'
            | '*'
            | '('
            | ')'
            | '_'
            | '+'
            | '{'
            | '}'
            | '|'
            | ':'
            | '"'
            | '<'
            | '>'
            | '?'
            | '~'
    )
}

pub fn format_prefix_combos(prefixes: &[KeyCombo]) -> String {
    if prefixes.is_empty() {
        return format_key_combo(DEFAULT_PREFIX);
    }
    prefixes
        .iter()
        .map(|combo| format_key_combo(*combo))
        .collect::<Vec<_>>()
        .join(" / ")
}
