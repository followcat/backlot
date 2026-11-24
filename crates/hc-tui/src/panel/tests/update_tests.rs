//! S2: the update hint's version comparison — strict semver first,
//! the stage's build id when the versions are equal.

use crate::panel::update::newest;

fn candidate(version: &str) -> (String, Option<String>) {
    (version.to_owned(), None)
}

#[test]
fn a_newer_stage_or_install_wins() {
    let hint = newest("0.3.0", None, &[candidate("0.3.1")]).expect("a newer stage shows");
    assert_eq!(
        hint.text(),
        "backlot 0.3.1 installed — restart the client to update (q, then backlot)"
    );
    // An installed `--version` probe answer takes the same path.
    let hint = newest("0.3.0", None, &[candidate("0.4")]).expect("a newer install shows");
    assert!(hint.text().contains("backlot 0.4 installed"), "{hint:?}");
    // The newest of several sources is the one named.
    let hint = newest(
        "0.3.0",
        None,
        &[candidate("0.3.1"), candidate("0.3.4"), candidate("0.3.2")],
    )
    .expect("the newest wins");
    assert!(hint.text().contains("backlot 0.3.4 installed"), "{hint:?}");
}

#[test]
fn an_equal_or_older_version_never_shows() {
    assert!(
        newest("0.3.0", None, &[candidate("0.3.0")]).is_none(),
        "equal"
    );
    assert!(
        newest("0.3.0", None, &[candidate("0.2.9")]).is_none(),
        "an older stage never shows"
    );
    assert!(newest("0.3.0", None, &[candidate("garbage")]).is_none());
    assert!(newest("garbage", None, &[candidate("0.3.1")]).is_none());
}

#[test]
fn equal_versions_compare_the_published_build() {
    // The stage publishes a different build id of the same version.
    let stage = ("0.3.0".to_owned(), Some("build-b".to_owned()));
    let hint = newest("0.3.0", Some("build-a"), &[stage]).expect("a different build shows");
    assert!(
        hint.text().contains("backlot 0.3.0+build-b installed"),
        "{hint:?}"
    );
    // Same build — nothing to say.
    let same = ("0.3.0".to_owned(), Some("build-a".to_owned()));
    assert!(newest("0.3.0", Some("build-a"), &[same]).is_none());
    // Without a build on either side the versions alone decide.
    assert!(newest("0.3.0", None, &[candidate("0.3.0")]).is_none());
    let no_running_build = ("0.3.0".to_owned(), Some("build-b".to_owned()));
    assert!(newest("0.3.0", None, &[no_running_build]).is_none());
}
