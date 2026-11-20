//! The agy adapter's pieces: the launch argv (`--mode`,
//! `--effort`, `--model`, `--settings`, `-i` fresh only), the
//! per-session settings file's `toolPermission` splice, and the
//! `conversations/<uuid>.db` session lookup — a byte match on the
//! workspace's `file://` URI, no sqlite read.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::agy::{AgyPrep, prepare, session_at, session_in_argv, settings_content};
use super::{ActorIdentity, LaunchCtx, Prep, plan};
use crate::test_support::TestDir;

fn tmp(tag: &str) -> TestDir {
    TestDir::new(&format!("bl-agy-{tag}"))
}

fn ctx<'a>(env: &'a BTreeMap<String, String>, cwd: &'a str) -> LaunchCtx<'a> {
    LaunchCtx {
        terminal: "bl-s-actor-t1-c1",
        cwd,
        prompt: None,
        resume: None,
        studio_dir: "/home/user/state/studios/acme-shop",
        stage_env: env,
        role: "actor-t1-c1",
        checks: &[],
    }
}

fn ident(effort: Option<&str>) -> ActorIdentity {
    ActorIdentity {
        runner: "agy".into(),
        runner_version: None,
        model: "gemini-4-neon-high".into(),
        effort: effort.map(str::to_string),
        permission_profile: "standard".into(),
    }
}

/// A fresh launch's argv: `--mode accept-edits` for either
/// profile, `--effort`/`--model` from the identity, `--settings`
/// the workdir file, and `-i` carrying the prompt. The plan's
/// prep names the same file and the session is post-launch.
#[test]
fn plan_shapes_the_fresh_launch() {
    let env = BTreeMap::new();
    let mut c = ctx(&env, "/home/user/acme-shop");
    c.prompt = Some("do the thing");
    for profile in ["standard", "worktree"] {
        let l = plan(
            &ActorIdentity {
                permission_profile: profile.into(),
                ..ident(Some("high"))
            },
            &c,
        )
        .unwrap();
        assert_eq!(
            l.argv,
            [
                "agy",
                "--mode",
                "accept-edits",
                "--effort",
                "high",
                "--model",
                "gemini-4-neon-high",
                "--settings",
                "/home/user/acme-shop/.agy-settings.json",
                "-i",
                "do the thing",
            ],
            "{profile}"
        );
        assert!(matches!(l.session, super::Session::PostLaunch));
        let Some(Prep::Agy(p)) = l.prep else {
            panic!("agy launch carries its prep")
        };
        assert_eq!(
            p.settings,
            Path::new("/home/user/acme-shop/.agy-settings.json")
        );
    }
}

/// A resume swaps `-i` for `--conversation <id>` and carries no
/// prompt — the stage sends it after the composer opens, devin's
/// rule. The settings file still rides: prep runs on resumes too.
#[test]
fn plan_shapes_the_resume() {
    let env = BTreeMap::new();
    let mut c = ctx(&env, "/home/user/acme-shop");
    c.resume = Some("0373977e-3d0c-4628-9218-13ba9695cd55");
    c.prompt = Some("ignored");
    let l = plan(&ident(Some("max")), &c).unwrap();
    assert_eq!(
        l.argv,
        [
            "agy",
            "--conversation",
            "0373977e-3d0c-4628-9218-13ba9695cd55",
            "--mode",
            "accept-edits",
            "--effort",
            "max",
            "--model",
            "gemini-4-neon-high",
            "--settings",
            "/home/user/acme-shop/.agy-settings.json",
        ]
    );
    assert!(l.prep.is_some());
}

/// An unknown permission profile is the plan refusal; an identity
/// with no effort still plans — the cast rule requires one, so a
/// missing value only means a manually-written cast.
#[test]
fn plan_refuses_a_bad_profile_and_keeps_going_without_effort() {
    let env = BTreeMap::new();
    let c = ctx(&env, "/home/user/acme-shop");
    let err = plan(
        &ActorIdentity {
            permission_profile: "yolo".into(),
            ..ident(Some("low"))
        },
        &c,
    )
    .unwrap_err();
    assert_eq!(err.reason, super::PROFILE_NOT_SUPPORTED);
    let l = plan(&ident(None), &c).unwrap();
    assert!(l.argv.windows(2).any(|w| w == ["--effort", ""]));
}

/// The settings file's bytes: `{"toolPermission":"always-proceed"}`
/// fresh, or the member spliced byte-exact into an existing object
/// — every other member keeps its whitespace and order.
#[test]
fn settings_splices_tool_permission_only() {
    let fresh = settings_content(None).unwrap();
    assert_eq!(fresh, b"{\"toolPermission\":\"always-proceed\"}\n");
    // An existing object keeps its other members verbatim.
    let existing = b"{\n  \"model\": \"gemini-4-neon\",\n  \"toolPermission\": \"prompt\",\n  \"verbosity\": \"low\"\n}";
    let spliced = settings_content(Some(existing)).unwrap();
    let text = String::from_utf8(spliced).unwrap();
    assert!(text.contains("\"model\": \"gemini-4-neon\""), "{text}");
    assert!(
        text.contains("\"toolPermission\": \"always-proceed\""),
        "{text}"
    );
    assert!(!text.contains("\"prompt\""), "{text}");
    assert!(text.contains("\"verbosity\": \"low\""), "{text}");
    // A non-object file is healed, not preserved.
    assert_eq!(
        settings_content(Some(b"not json\n")).unwrap(),
        b"{\"toolPermission\":\"always-proceed\"}\n"
    );
}

/// `prepare` writes `<cwd>/.agy-settings.json` mode 600 before the
/// launch — agy reads it once at startup, so a failure is an
/// attention, never a half-allowed role.
#[test]
fn prepare_writes_the_session_settings() {
    let dir = tmp("prep");
    let settings = dir.join("work/.agy-settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let p = AgyPrep {
        settings: settings.clone(),
    };
    assert!(!prepare(&p).unwrap());
    let text = fs::read_to_string(&settings).unwrap();
    assert!(text.contains("always-proceed"), "{text}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&settings).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

/// The session id a live agy role's launch minted: the newest
/// `<uuid>.db` under `conversations/` created at or after the
/// role's start whose bytes name `file://<cwd>` — no other role's
/// id, nothing older, nothing from another workdir.
#[test]
fn session_at_finds_the_roles_conversation_db() {
    let dir = tmp("sessions");
    let conv = dir.join("conversations");
    fs::create_dir_all(&conv).unwrap();
    let started_ms = 1_000_000;
    let db = |name: &str, cwd: &str| {
        let p = conv.join(format!("{name}.db"));
        fs::write(&p, format!("sqlite-ish file://{cwd}/ blob")).unwrap();
        p
    };
    // A db for the role's cwd, minted at start — the hit.
    let mine = db(
        "aaaaaaaa-0000-0000-0000-000000000001",
        "/home/user/acme-shop",
    );
    // Another workdir's db and an untagged one are skipped.
    db("bbbbbbbb-0000-0000-0000-000000000002", "/home/user/other");
    fs::write(
        conv.join("cccccccc-0000-0000-0000-000000000003.db"),
        "no uri",
    )
    .unwrap();
    // A non-db file is skipped.
    fs::write(conv.join("notes.txt"), "file:///home/user/acme-shop").unwrap();
    let id = session_at(&conv, "/home/user/acme-shop", started_ms, &BTreeSet::new()).unwrap();
    assert_eq!(id, "aaaaaaaa-0000-0000-0000-000000000001");
    // Already claimed by another role — none left.
    let taken = BTreeSet::from([id.clone()]);
    assert_eq!(
        session_at(&conv, "/home/user/acme-shop", started_ms, &taken),
        None
    );
    // A far-future start finds nothing.
    let far = file_time(&mine) + 10_000_000;
    assert_eq!(
        session_at(&conv, "/home/user/acme-shop", far, &BTreeSet::new()),
        None
    );
    // A different cwd finds its own.
    assert_eq!(
        session_at(&conv, "/home/user/other", started_ms, &BTreeSet::new()),
        Some("bbbbbbbb-0000-0000-0000-000000000002".to_string())
    );
}

/// `file://` URIs percent-encode what a path cannot hold: a cwd
/// with a space must still match — the db writes `%20`, the match
/// encodes the cwd the same way.
#[test]
fn session_at_matches_an_encoded_uri() {
    let dir = tmp("sessions-pct");
    let conv = dir.join("conversations");
    fs::create_dir_all(&conv).unwrap();
    fs::write(
        conv.join("dddddddd-0000-0000-0000-000000000004.db"),
        "blob file:///home/user/acme%20shop/blob",
    )
    .unwrap();
    assert_eq!(
        session_at(&conv, "/home/user/acme shop", 0, &BTreeSet::new()),
        Some("dddddddd-0000-0000-0000-000000000004".to_string())
    );
}

/// The resumed terminal's id sits after `--conversation`; `-c` is
/// a boolean (continue the most recent) and carries none.
#[test]
fn the_resumed_argv_names_its_session() {
    let argv = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        session_in_argv(&argv(&["agy", "--conversation", "abc-1", "-i", "p"])),
        Some("abc-1".to_string())
    );
    assert_eq!(session_in_argv(&argv(&["agy", "-c"])), None);
    assert_eq!(session_in_argv(&argv(&["agy", "--continue"])), None);
}

/// A file's birth time, or its mtime where birth time is
/// unsupported — the same clock `session_at` reads.
fn file_time(path: &Path) -> u64 {
    let md = fs::metadata(path).unwrap();
    let t = md.created().or_else(|_| md.modified()).unwrap();
    t.duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
