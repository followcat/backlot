//! `studio new --interactive` (U2) scripted end to end through
//! `run_io`: the prompts answer from lines, the draft's git and
//! runner probes come from the `Io` seams, and the write itself is
//! the ordinary `studio new` one against a scratch state dir — a
//! real git repository under `files/` like the dry-run tests use.

use std::fs;
use std::path::PathBuf;

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::cli_testkit::*;
use crate::wizard::{Io, WizardArgs, create_draft, run_io};

/// A git repository under `files/` — `draft::check` asks git
/// itself, so the wizard's answer must be a real one.
fn repo(scratch: &Scratch, name: &str) -> String {
    let dir = scratch.files.join(name);
    fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-b", "main"]);
    fs::write(dir.join("f"), "base\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-m", "base"]);
    dir.to_string_lossy().into_owned()
}

/// Scripted seams: `lines` feed every prompt in order, `fake` is
/// the only installed runner, `edit` swaps in `edited` when set,
/// and the prompts are collected to assert on.
fn io<'a>(lines: &'a mut &[u8], out: &'a mut Vec<u8>, edited: Option<&'a str>) -> Io<'a> {
    Io {
        input: lines,
        out,
        git_repo: Box::new(|_| true),
        installed: Box::new(|r| r == "fake"),
        edit: Box::new(move |file| match edited {
            Some(text) => fs::write(file, text).map_err(|e| e.to_string()),
            None => Err("no editor".into()),
        }),
        pick_repo: None,
    }
}

fn args() -> WizardArgs {
    WizardArgs {
        title: None,
        repo: None,
        cast_file: None,
        dry_run: false,
    }
}

/// Draft files live in the shared temp dir — runs that could
/// observe another test's half-finished file serialize here.
static DRAFT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn run(
    common: Common,
    a: WizardArgs,
    env: &Env,
    lines: &[u8],
    edited: Option<&str>,
) -> (Result<String, Fail>, String) {
    let _lock = DRAFT_LOCK.lock().unwrap();
    let mut input = lines;
    let mut out = Vec::new();
    let res = {
        let mut io = io(&mut input, &mut out, edited);
        run_io(common, a, env, &mut io)
    };
    (res, String::from_utf8(out).unwrap())
}

#[test]
fn every_prompt_defaults_and_accepts() {
    let (scratch, env) = scratch();
    // A repo named `acme-shop`: the id default is its directory
    // name, the title default the id, the cast the drafted `fake`
    // runner — every prompt answered with a bare newline.
    let repo = repo(&scratch, "acme-shop");
    let answers = format!("{repo}\n\n\n\nn\n");
    let (res, out) = run(Common::default(), args(), &env, answers.as_bytes(), None);
    let out_str = res.unwrap();
    assert!(out_str.contains("studio.created"), "{out_str}");
    assert!(out.contains("repository"), "{out}");
    assert!(out.contains("studio [acme-shop]"), "{out}");
    assert!(out.contains("title [acme-shop]"), "{out}");
    assert!(out.contains("runner = \"fake\""), "{out}");
    assert!(out.contains("start the stage"), "{out}");
    // The studio exists in the scratch ledger.
    assert!(
        scratch.dir.join("studios/acme-shop/events.jsonl").is_file(),
        "no studio written"
    );
    // The draft's temporary file is gone after the write.
    assert!(
        fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .all(|e| !e.file_name().to_string_lossy().starts_with("backlot-cast-")),
        "draft temp file left behind"
    );
}

#[test]
fn flags_answer_their_own_questions() {
    let (scratch, env) = scratch();
    let repo = repo(&scratch, "flag-repo");
    let cast = fixture(&scratch, "cast.toml", CAST_FAKE);
    let a = WizardArgs {
        title: Some("Titled".into()),
        repo: Some(repo),
        cast_file: Some(PathBuf::from(cast)),
        dry_run: false,
    };
    let common = Common {
        studio: Some("flag-shop".into()),
        ..Common::default()
    };
    // Only the stage offer remains to answer.
    let (res, out) = run(common, a, &env, b"n\n", None);
    assert!(res.unwrap().contains("studio.created"));
    assert!(!out.contains("repository"), "{out}");
    assert!(scratch.dir.join("studios/flag-shop/events.jsonl").is_file());
}

#[test]
fn a_bad_id_reprompts_until_valid() {
    let (scratch, env) = scratch();
    let repo = repo(&scratch, "acme-shop");
    let answers = format!("{repo}\nBad Name!\nok-shop\n\n\nn\n");
    let (res, out) = run(Common::default(), args(), &env, answers.as_bytes(), None);
    assert!(res.unwrap().contains("studio.created"));
    assert!(out.contains("bad studio name"), "{out}");
    assert!(scratch.dir.join("studios/ok-shop/events.jsonl").is_file());
}

#[test]
fn eof_refuses_without_writing() {
    let (scratch, env) = scratch();
    let (res, _out) = run(Common::default(), args(), &env, b"", None);
    match res {
        Err(Fail::Refused(m)) => assert!(m.contains("input ended"), "{m}"),
        other => panic!("expected refusal, got {other:?}"),
    }
    assert!(!scratch.dir.join("studios").exists());
}

#[test]
fn a_refused_cast_writes_nothing() {
    let (scratch, env) = scratch();
    let repo = repo(&scratch, "acme-shop");
    let answers = format!("{repo}\n\n\nrefuse\n");
    let (res, _out) = run(Common::default(), args(), &env, answers.as_bytes(), None);
    match res {
        Err(Fail::Refused(m)) => assert!(m.contains("cast refused"), "{m}"),
        other => panic!("expected refusal, got {other:?}"),
    }
    assert!(!scratch.dir.join("studios").exists());
}

#[test]
fn an_edited_cast_is_what_gets_written() {
    let (scratch, env) = scratch();
    let repo = repo(&scratch, "acme-shop");
    // `e` opens the editor; the injected one writes a cast whose
    // producer runs on a different runner model; `a` then accepts.
    let edited = CAST_FAKE.replace("model = \"m\"", "model = \"edited-m\"");
    let answers = format!("{repo}\n\n\ne\na\nn\n");
    let (res, out) = run(
        Common::default(),
        args(),
        &env,
        answers.as_bytes(),
        Some(&edited),
    );
    assert!(res.unwrap().contains("studio.created"));
    assert!(out.contains("edited-m"), "{out}");
    assert!(scratch.dir.join("studios/acme-shop/events.jsonl").is_file());
}

#[test]
fn a_repo_that_is_not_git_is_refused_like_the_dry_run() {
    let (scratch, env) = scratch();
    let plain = fixture(&scratch, "plain-dir", "x");
    let answers = format!("{plain}\n\n\na\n");
    let (res, _out) = run(Common::default(), args(), &env, answers.as_bytes(), None);
    match res {
        Err(Fail::Refused(m)) => assert!(m.contains("not a git repository"), "{m}"),
        other => panic!("expected refusal, got {other:?}"),
    }
    assert!(!scratch.dir.join("studios").exists());
}

#[test]
fn no_runner_installed_refuses_the_draft() {
    let _lock = DRAFT_LOCK.lock().unwrap();
    let (scratch, env) = scratch();
    let repo = repo(&scratch, "acme-shop");
    let answers = format!("{repo}\n\n\n");
    let mut input = answers.as_bytes();
    let mut out = Vec::new();
    let mut io = Io {
        input: &mut input,
        out: &mut out,
        git_repo: Box::new(|_| true),
        installed: Box::new(|_| false),
        edit: Box::new(|_| Err("no editor".into())),
        pick_repo: None,
    };
    let res = run_io(Common::default(), args(), &env, &mut io);
    match res {
        Err(Fail::Refused(m)) => assert!(m.contains("no supported runner"), "{m}"),
        other => panic!("expected refusal, got {other:?}"),
    }
    assert!(!scratch.dir.join("studios").exists());
}

#[test]
fn dry_run_and_interactive_do_not_combine() {
    let (_scratch, env) = scratch();
    let a = WizardArgs {
        dry_run: true,
        ..args()
    };
    let (res, _out) = run(Common::default(), a, &env, b"", None);
    match res {
        Err(Fail::Usage(m)) => assert!(m.contains("--dry-run"), "{m}"),
        other => panic!("expected usage error, got {other:?}"),
    }
}

#[test]
fn the_reserved_name_is_refused() {
    let (scratch, env) = scratch();
    let repo = repo(&scratch, "acme-shop");
    let common = Common {
        studio: Some("hollywood".into()),
        ..Common::default()
    };
    let answers = format!("{repo}\n\na\n");
    let (res, _out) = run(common, args(), &env, answers.as_bytes(), None);
    match res {
        Err(Fail::Refused(m)) => assert!(m.contains("reserved"), "{m}"),
        other => panic!("expected refusal, got {other:?}"),
    }
    assert!(!scratch.dir.join("studios").exists());
}

/// U5: on a terminal the `pick_repo` seam answers the repository
/// question — the line prompt never prints, and the picked path
/// flows through the same checks as a typed answer.
#[test]
fn a_terminal_pick_replaces_the_line_prompt() {
    let _lock = DRAFT_LOCK.lock().unwrap();
    let (scratch, env) = scratch();
    let repo = repo(&scratch, "acme-shop");
    let picked = PathBuf::from(&repo);
    let answers = b"\n\na\nn\n".as_slice();
    let mut input = answers;
    let mut out = Vec::new();
    let mut io = Io {
        input: &mut input,
        out: &mut out,
        git_repo: Box::new(|_| true),
        installed: Box::new(|r| r == "fake"),
        edit: Box::new(|_| Err("no editor".into())),
        pick_repo: Some(Box::new(move |_| Ok(picked.clone()))),
    };
    let res = run_io(Common::default(), args(), &env, &mut io);
    drop(io);
    assert!(res.unwrap().contains("studio.created"));
    let out = String::from_utf8(out).unwrap();
    assert!(!out.contains("repository"), "{out}");
    assert!(out.contains("studio [acme-shop]"), "{out}");
    assert!(scratch.dir.join("studios/acme-shop/events.jsonl").is_file());
}

/// U5: the picker's cancel refuses with "cancelled" — nothing is
/// written, like a refused cast.
#[test]
fn a_cancelled_pick_writes_nothing() {
    let (scratch, env) = scratch();
    let answers = b"\n\n\n\n".as_slice();
    let mut input = answers;
    let mut out = Vec::new();
    let mut io = Io {
        input: &mut input,
        out: &mut out,
        git_repo: Box::new(|_| true),
        installed: Box::new(|r| r == "fake"),
        edit: Box::new(|_| Err("no editor".into())),
        pick_repo: Some(Box::new(|_| Err(Fail::refused("cancelled")))),
    };
    let res = run_io(Common::default(), args(), &env, &mut io);
    match res {
        Err(Fail::Refused(m)) => assert!(m.contains("cancelled"), "{m}"),
        other => panic!("expected refusal, got {other:?}"),
    }
    assert!(!scratch.dir.join("studios").exists());
}

/// U5: a core that cannot be reached leaves the in-use group
/// absent — the picker works the same, it just has nothing to
/// offer on top.
#[test]
fn an_unreachable_core_leaves_no_in_use_group() {
    let (_scratch, env) = scratch();
    // A session name with no socket: the connect fails fast and
    // the group is empty — it never touches a real session.
    let common = Common {
        session: Some("u5-scratch-none".into()),
        ..Common::default()
    };
    assert!(crate::wizard::in_use(&env, &common).is_empty());
}

/// A draft-shaped cast on the `fake` runner — the same roles the
/// wizard drafts, written out for `--cast` and edited-cast tests.
const CAST_FAKE: &str = r#"
[producer]
runner = "fake"
model = "m"
effort = "medium"
permission_profile = "standard"

[director]
runner = "fake"
model = "m"
effort = "medium"
permission_profile = "standard"

[writer]
runner = "fake"
model = "m"
effort = "medium"
permission_profile = "standard"

[[candidates]]
name = "actor-a"
runner = "fake"
model = "m"
effort = "medium"
permission_profile = "standard"

[[candidates]]
name = "actor-b"
runner = "fake"
model = "m"
effort = "medium"
permission_profile = "standard"
"#;

/// U2 round 1: a name someone else already took — planted or leaked —
/// is never truncated: the draft moves to the next free number and
/// the file is born `0600`, never following a planted symlink.
#[test]
fn a_planted_draft_name_is_never_overwritten() {
    let (scratch, _env) = scratch();
    let dir = &scratch.files;
    let planted = dir.join(format!("backlot-cast-{}-0.toml", std::process::id()));
    fs::write(&planted, "planted").unwrap();
    let made = create_draft(dir, "drafted").unwrap();
    assert_ne!(made, planted, "the planted name was taken");
    assert_eq!(made.extension().and_then(|e| e.to_str()), Some("toml"));
    assert_eq!(fs::read_to_string(&planted).unwrap(), "planted");
    assert_eq!(fs::read_to_string(&made).unwrap(), "drafted");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&made).unwrap().permissions().mode() & 0o777,
            0o600,
            "the draft file must be owner-only"
        );
    }
}
