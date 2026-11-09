//! The user docs cannot drift: every fenced `backlot` command line in
//! `docs/studio/guide.md` and the README's "Run a studio" section goes
//! through the real argument parser (placeholders filled with neutral
//! values), the guide's example cast file parses through the same
//! `studio new` build the command uses, and every attention reason the
//! stage raises appears in the guide's table.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use backlot_model::event::EventKind;
use backlot_model::state::StudioState;

use crate::args::{self, Cmd};
use crate::cli::Caller;
use crate::commands;
use crate::reasons;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backlot-docs-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
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
    };
    let build = commands::build(
        &caller,
        Cmd::StudioNew {
            title: "Shop".into(),
            repo: "/home/user/acme-shop".into(),
            cast_file,
        },
    )
    .expect("the guide's cast file fails the `studio new` build");
    let pending = build(&StudioState::default(), 1).unwrap();
    assert!(matches!(
        pending.event.kind,
        EventKind::StudioCreated { .. }
    ));
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
