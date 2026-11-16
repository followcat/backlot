//! Unit tests for the opening page's pure pieces — the wizard's
//! popup line. The App wiring (visibility, keys, the draw hook) is
//! covered in `mod_tests/opening.rs`.

use super::*;

#[test]
fn wizard_line_quotes_and_appends_endpoint() {
    assert_eq!(
        wizard_line("/usr/bin/backlot", &["--session".into(), "s1".into()]),
        "'/usr/bin/backlot' studio new --interactive '--session' 's1'"
    );
    assert_eq!(wizard_line("/b", &[]), "'/b' studio new --interactive");
}

#[test]
fn wizard_line_escapes_a_quote_and_a_space() {
    // POSIX single-quote escaping: `'` must come out as `'\''` — an
    // unescaped quote closes the word mid-path and the shell eats
    // the rest.
    assert_eq!(
        wizard_line("/u/it's backlot", &["--remote".into(), "host a'z".into()]),
        "'/u/it'\\''s backlot' studio new --interactive '--remote' 'host a'\\''z'"
    );
}
