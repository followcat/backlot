//! The user docs cannot drift: every fenced `backlot` command line in
//! `docs/studio/guide.md` and the README's "Run a studio" section goes
//! through the real argument parser (placeholders filled with neutral
//! values), the guide's example cast file parses through the same
//! `studio new` build the command uses, and every attention reason the
//! stage raises appears in the guide's table.

use std::fs;
use std::path::PathBuf;

use backlot_model::event::EventKind;
use backlot_model::state::StudioState;

use crate::args::{self, Cmd};
use crate::cli::Caller;
use crate::commands;
use crate::reasons;
use crate::test_support::TestDir;

fn tmp() -> TestDir {
    TestDir::new("backlot-docs-test")
}

/// A file in the repository root, from the crate's manifest dir.
fn repo_file(rel: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// `(info, body)` per fenced block.
fn fenced_blocks(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut open = false;
    let mut info = String::new();
    let mut body = String::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            if open {
                out.push((info.clone(), body.clone()));
                open = false;
            } else {
                open = true;
                info = t.trim_start_matches('`').to_string();
                body.clear();
            }
            continue;
        }
        if open {
            body.push_str(line);
            body.push('\n');
        }
    }
    out
}

/// Command lines starting `backlot ` inside `text`'s fenced blocks.
/// A bare `backlot` line opens the TUI and is not a studio command.
fn backlot_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (_, body) in fenced_blocks(text) {
        for line in body.lines() {
            let t = line.trim();
            if t.starts_with("backlot ") {
                out.push(t.to_string());
            }
        }
    }
    out
}

/// The text between `heading` and the next `## ` section.
fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let Some(start) = text.find(heading) else {
        panic!("no section '{heading}'");
    };
    let rest = &text[start + heading.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    &rest[..end]
}

/// Shell-style words: whitespace splits, quotes group.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut w = String::new();
    let mut quote = None;
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => w.push(c),
            (None, '"' | '\'') => quote = Some(c),
            (None, c) if c.is_whitespace() => {
                if !w.is_empty() {
                    words.push(std::mem::take(&mut w));
                }
            }
            (None, c) => w.push(c),
        }
    }
    if !w.is_empty() {
        words.push(w);
    }
    words
}

/// Neutral stand-ins for `<placeholder>` spans and `a|b` alternations,
/// so a documented command parses exactly like a typed one. A `<...>`
/// the table does not know becomes `x`, which fails the test whenever
/// the flag's value is typed — add it to the table, never to a branch.
fn substitute(token: &str) -> String {
    let token = token.trim_end_matches('.');
    let token = if token.contains('|') && !token.contains('<') {
        token.split('|').next().unwrap_or(token)
    } else {
        token
    };
    let Some(start) = token.find('<') else {
        return token.to_string();
    };
    let end = token[start + 1..]
        .find('>')
        .map(|i| start + 1 + i + 1)
        .unwrap_or(token.len());
    let inner = &token[start + 1..end - 1];
    let value = match inner {
        "n" | "seq" | "delivery" | "event" => "1",
        "id" | "task" => "t1",
        "studio" | "name" => "acme-shop",
        "role key" | "person" | "role" => "actor-t1-c1",
        "actor" | "actor name" => "actor-a",
        "commits" | "A..B" => "a1..b2",
        "key=value" => "k=v",
        "level" => "normal",
        "kind" => "plan",
        "decision" => "allow",
        "verdict" => "accepted",
        "core-id" => "core",
        "dur" => "24h",
        "md|json" => "md",
        _ => "x",
    };
    format!("{}{}{}", &token[..start], value, &token[end..])
}

/// Every command line goes through the real parser after substitution.
fn assert_commands_parse(source: &str, lines: &[String]) {
    assert!(!lines.is_empty(), "{source} has no backlot command lines");
    for line in lines {
        let words: Vec<String> = shell_words(line)
            .into_iter()
            .skip(1)
            .map(|w| substitute(&w))
            .collect();
        for w in &words {
            assert!(!w.contains('<'), "{source}: `{line}` keeps a placeholder");
        }
        assert!(
            args::parse(&words).is_ok(),
            "{source}: `{line}` does not parse"
        );
    }
}

/// Every `backlot ...` line in a fenced block of the guide parses
/// through the real argument parser.
#[test]
fn guide_commands_parse() {
    let guide = repo_file("docs/studio/guide.md");
    assert_commands_parse("guide.md", &backlot_lines(&guide));
}

/// Same for the Hollywood doc — its propose/card/tell lines.
#[test]
fn hollywood_commands_parse() {
    let doc = repo_file("docs/studio/hollywood.md");
    assert_commands_parse("hollywood.md", &backlot_lines(&doc));
}

/// Same for the single-server doc — its `server run|start|stop|adopt|service` lines.
#[test]
fn server_doc_commands_parse() {
    let doc = repo_file("docs/studio/server.md");
    assert_commands_parse("server.md", &backlot_lines(&doc));
}

/// Same for the README's "Run a studio" section.
#[test]
fn readme_run_a_studio_commands_parse() {
    let readme = repo_file("README.md");
    assert_commands_parse(
        "README.md",
        &backlot_lines(section(&readme, "## Run a studio")),
    );
}

/// The guide's example cast file parses through the same build
/// `studio new` runs — a `studio.created` event comes out the far end.
#[test]
fn guide_cast_file_parses_through_studio_new() {
    let guide = repo_file("docs/studio/guide.md");
    let cast = fenced_blocks(&guide)
        .into_iter()
        .find(|(info, body)| info == "toml" && body.contains("[producer]"))
        .map(|(_, body)| body)
        .expect("guide.md has no cast example");
    let dir = tmp();
    let cast_file = dir.join("cast.toml");
    fs::write(&cast_file, cast).unwrap();
    let caller = Caller {
        studio: "acme-shop".into(),
        role: "user".into(),
        task: None,
        session: None,
        via: None,
    };
    let env = crate::cli::Env {
        vars: Default::default(),
    };
    let build = commands::build(
        &caller,
        Cmd::StudioNew {
            title: Some("Shop".into()),
            repo: Some("/home/user/acme-shop".into()),
            cast_file: Some(cast_file),
            dry_run: false,
            interactive: false,
            mode: None,
            init: false,
            no_git: false,
            ask: None,
        },
        &env,
        dir.as_ref(),
    )
    .expect("the guide's cast file fails the `studio new` build");
    let pending = build(&StudioState::default(), 1).unwrap();
    assert!(matches!(
        pending.event.kind,
        EventKind::StudioCreated { .. }
    ));
}

/// `docs/studio/runners.md`'s generated spans — the capability table
/// and the approval-keys table — are rendered from the runner caps.
/// The test regenerates each span and fails when the file has drifted;
/// `BACKLOT_UPDATE_DOCS=1 cargo test runner_caps_docs` rewrites it.
#[test]
fn runner_caps_docs_are_generated_from_the_caps() {
    use crate::runners::caps_doc;

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/studio/runners.md");
    let text = fs::read_to_string(&path).expect("runners.md");
    let rendered = caps_doc::replace_generated(
        &text,
        caps_doc::CAPS_BEGIN,
        caps_doc::CAPS_END,
        &caps_doc::capabilities_table(),
    )
    .and_then(|text| {
        caps_doc::replace_generated(
            &text,
            caps_doc::APPROVAL_BEGIN,
            caps_doc::APPROVAL_END,
            &caps_doc::approval_table(),
        )
    })
    .expect("runners.md carries both generated spans");
    if std::env::var_os("BACKLOT_UPDATE_DOCS").is_some() {
        fs::write(&path, &rendered).expect("write runners.md");
        return;
    }
    assert_eq!(
        rendered, text,
        "docs/studio/runners.md's generated tables drifted from the caps; run \
         BACKLOT_UPDATE_DOCS=1 cargo test runner_caps_docs_are_generated_from_the_caps"
    );
    // Every runner has a signal-matrix row and a work-state row, agy
    // included; the rows name the runner the way the prose does.
    for heading in ["## Signal matrix", "## Work states"] {
        let table = section(&text, heading);
        for caps in backlot_model::caps::ALL {
            let label = caps.label.trim_matches('`');
            assert!(
                table.lines().any(|l| {
                    l.trim_start_matches('|')
                        .trim_start()
                        .trim_start_matches('`')
                        .starts_with(label)
                }),
                "{heading} misses {label}"
            );
        }
    }
}

/// Every attention reason the stage can raise is in the guide's table.
#[test]
fn every_attention_reason_is_documented() {
    let guide = repo_file("docs/studio/guide.md");
    let table = section(&guide, "## What needs attention");
    for reason in reasons::ALL {
        assert!(
            table.contains(&format!("`{reason}`")),
            "guide.md's attention table misses `{reason}`"
        );
    }
}

/// Every reason's class is in the panel's table, and the class there is
/// the one the shared function returns.
#[test]
fn attention_classes_are_documented() {
    use backlot_model::attention::AttentionClass;
    let panel = repo_file("docs/studio/panel.md");
    let table = section(&panel, "## Person states and attentions");
    let documented = |reason: &str| {
        let word = match AttentionClass::of(reason, false) {
            AttentionClass::NeedsYou => "needs you",
            AttentionClass::Stuck => "stuck",
            AttentionClass::Info => "info",
        };
        let key = if reason.ends_with(" prep failed") {
            "<runner> prep failed".to_string()
        } else {
            format!("`{reason}`")
        };
        table
            .lines()
            .any(|line| line.contains(&key) && line.contains(&format!("| {word} |")))
    };
    for reason in reasons::ALL {
        assert!(documented(reason), "panel.md's table misses `{reason}`");
    }
    assert!(
        documented(backlot_model::event::FOLDER_TRUST),
        "panel.md's table misses the folder-trust dialog"
    );
}
