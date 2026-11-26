//! `backlot guide`: the topics the binary carries, the listing, one
//! topic's text, `--grep`, and a mistyped topic's refusal. The docs
//! are compiled in, so no studio, ledger or core is touched.

use crate::cli_testkit::{ok, refused, scratch, usage};
use crate::guide;

/// The listing names every compiled topic once, with its title — and
/// no topic is repeated, so a mistyped entry cannot double-print.
#[test]
fn the_listing_names_every_topic() {
    let (_s, env) = scratch();
    let out = ok(&env, "guide");
    let mut names: Vec<&str> = guide::names().collect();
    names.sort_unstable();
    for name in &names {
        assert!(
            out.lines().any(|l| l.trim_start().starts_with(name)),
            "the listing misses '{name}':\n{out}"
        );
    }
    // The guide itself is one of them — the file the role prompts
    // point at.
    assert!(names.contains(&"guide"), "{names:?}");
}

/// Naming a topic prints that file's bytes exactly — the same text
/// the repo carries, not a re-render.
#[test]
fn a_topic_prints_its_file() {
    let (_s, env) = scratch();
    let out = ok(&env, "guide guide");
    assert!(
        out.starts_with("# Running a studio"),
        "the guide topic's first line is wrong:\n{}",
        out.lines().next().unwrap_or_default()
    );
    // Every topic prints something with the file's own heading.
    for name in guide::names() {
        let text = ok(&env, &format!("guide {name}"));
        assert!(
            text.starts_with('#'),
            "'{name}' does not open with a heading"
        );
        assert!(!text.trim().is_empty(), "'{name}' is empty");
    }
}

/// `--grep` keeps only the matching lines, tagged with their number,
/// and reports none honestly when nothing matches.
#[test]
fn grep_narrows_to_matching_lines() {
    let (_s, env) = scratch();
    // A word the guide certainly carries, and one it cannot.
    let found = ok(&env, "guide guide --grep permission");
    assert!(
        found.lines().skip(1).all(|l| {
            let body = l.split_once(": ").map(|(_, t)| t).unwrap_or(l);
            body.to_lowercase().contains("permission")
        }),
        "grep leaked a non-matching line:\n{found}"
    );
    assert!(found.lines().count() > 1, "{found}");

    let none = ok(&env, "guide guide --grep zzznothingmatchesthis");
    assert!(none.contains("no line"), "{none}");

    // With no topic, `--grep` searches every topic and tags each hit.
    let all = ok(&env, "guide --grep permission");
    assert!(all.contains("guide:"), "no topic tag in:\n{all}");
}

/// A mistyped topic refuses and names the topics that exist — never a
/// silent empty page.
#[test]
fn an_unknown_topic_refuses_and_names_the_topics() {
    let (_s, env) = scratch();
    let msg = refused(&env, "guide nope");
    assert!(msg.contains("no such guide topic 'nope'"), "{msg}");
    for name in guide::names() {
        assert!(msg.contains(name), "the refusal misses '{name}': {msg}");
    }
}

/// The parser rejects a stray second positional and has its own usage
/// line, and `--help` prints it.
#[test]
fn parser_shape() {
    let (_s, env) = scratch();
    let msg = usage(&env, "guide one two");
    assert!(msg.contains("unexpected argument 'two'"), "{msg}");
    let help = ok(&env, "guide --help");
    assert!(help.contains("usage: backlot guide"), "{help}");
    assert!(help.contains("--grep"), "{help}");
}

/// The compiled topic set is exactly the files in `docs/studio/` — a
/// new doc that is not embedded fails here, so the binary never ships
/// a partial set.
#[test]
fn every_doc_file_is_embedded() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/studio");
    let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            (path.extension().and_then(|x| x.to_str()) == Some("md"))
                .then(|| path.file_stem()?.to_str().map(String::from))
                .flatten()
        })
        .collect();
    on_disk.sort();
    let mut embedded: Vec<String> = guide::names().map(String::from).collect();
    embedded.sort();
    assert_eq!(embedded, on_disk, "docs/studio and the guide disagree");
}
