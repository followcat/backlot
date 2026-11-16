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
