//! `backlot studio list`: the discovery half of the cross-session
//! lookup — every studio under every core's state directory in one
//! read, with the state its ledger shows. `BACKLOT_STATE_DIR` pins
//! the listing to that one directory.

use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::Event;
use backlot_model::event::{ActorIdentity, EventKind, RoleConfigs, StudioMode};

use crate::cli::Env;
use crate::cli_testkit::ok;
use crate::store;
use crate::test_support::TestDir;

/// `<root>/backlot/<core>` — the core directory under a fake XDG
/// state home.
fn core(root: &TestDir, id: &str) -> PathBuf {
    root.join("backlot").join(id)
}

/// A plain shell: an XDG state home, no `BACKLOT_STATE_DIR` pin.
fn shell(root: &TestDir) -> Env {
    let mut vars = std::collections::BTreeMap::new();
    vars.insert("XDG_STATE_HOME".into(), root.to_string_lossy().into_owned());
    Env { vars }
}

fn identity() -> ActorIdentity {
    ActorIdentity {
        runner: "runner".into(),
        runner_version: None,
        model: "model".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

/// A `studio.created` event's kind with this title and bound path.
fn created(title: &str, repo: &str) -> EventKind {
    EventKind::StudioCreated {
        title: title.into(),
        repo: repo.into(),
        members: Vec::new(),
        no_git: false,
        mode: StudioMode::Standard,
        roles: Box::new(RoleConfigs {
            producer: identity(),
            director: identity(),
            writer: identity(),
        }),
        candidates: Vec::new(),
        rework_cap: 2,
        stall_limit_s: 600,
        nudge_after_s: 180,
        escalate_after_s: 600,
        check_env: Vec::new(),
        checks: Vec::new(),
        task_budget_s: None,
        limits: Default::default(),
        max_actors: backlot_model::event::default_max_actors(),
        push: Default::default(),
        design_gate: backlot_model::event::DesignGate::Off,
    }
}

/// One ledger event at a chosen clock — recency is the last event's.
fn ev(seq: u64, ts_ms: u64, kind: EventKind) -> Event {
    let mut e = backlot_model::testkit::ev(seq, "user", kind);
    e.ts_ms = ts_ms;
    e
}

/// Write a studio's `events.jsonl` under `core_dir`.
fn write_ledger(core_dir: &Path, name: &str, events: &[Event]) {
    let dir = store::studio_dir(core_dir, name);
    fs::create_dir_all(&dir).unwrap();
    let mut text = String::new();
    for e in events {
        text.push_str(&serde_json::to_string(e).unwrap());
        text.push('\n');
    }
    fs::write(dir.join("events.jsonl"), text).unwrap();
}

/// Two cores, two studios: both are listed, each with its own core
/// id and the state its ledger shows, newest activity first.
#[test]
fn two_cores_list_together_with_their_states() {
    let root = TestDir::new("backlot-xdg");
    write_ledger(
        &core(&root, "core-a"),
        "acme-shop",
        &[ev(1, 1_000, created("Shop", "/home/user/acme-shop"))],
    );
    write_ledger(
        &core(&root, "core-b"),
        "beta-tools",
        &[
            ev(1, 1_000, created("Tools", "/home/user/beta-tools")),
            ev(
                2,
                2_000,
                EventKind::StudioFinished {
                    reason: "done".into(),
                },
            ),
        ],
    );
    let env = shell(&root);

    let json = ok(&env, "studio list --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let rows = v["studios"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{json}");
    // The studio whose ledger moved last comes first.
    assert_eq!(rows[0]["name"], "beta-tools", "{json}");
    assert_eq!(rows[0]["state"], "finished", "{json}");
    assert_eq!(rows[0]["core"], "core-b", "{json}");
    assert_eq!(rows[0]["title"], "Tools", "{json}");
    assert_eq!(rows[0]["repo"], "/home/user/beta-tools", "{json}");
    assert_eq!(rows[1]["name"], "acme-shop", "{json}");
    assert_eq!(rows[1]["state"], "live", "{json}");
    assert_eq!(rows[1]["core"], "core-a", "{json}");
    assert_eq!(rows[1]["title"], "Shop", "{json}");
    assert_eq!(rows[1]["repo"], "/home/user/acme-shop", "{json}");

    let table = ok(&env, "studio list");
    for want in [
        "acme-shop",
        "beta-tools",
        "core-a",
        "core-b",
        "live",
        "finished",
        "Shop",
        "/home/user/beta-tools",
    ] {
        assert!(table.contains(want), "table misses '{want}': {table}");
    }
}

/// A pinned `BACKLOT_STATE_DIR` lists that one directory only — role
/// terminals and scripted runs stay scoped.
#[test]
fn a_pinned_state_dir_lists_one_directory() {
    let root = TestDir::new("backlot-xdg");
    let core_a = core(&root, "core-a");
    write_ledger(
        &core_a,
        "acme-shop",
        &[ev(1, 1_000, created("Shop", "/home/user/acme-shop"))],
    );
    write_ledger(
        &core(&root, "core-b"),
        "beta-tools",
        &[ev(1, 1_000, created("Tools", "/home/user/beta-tools"))],
    );

    let mut env = shell(&root);
    env.vars.insert(
        "BACKLOT_STATE_DIR".into(),
        core_a.to_string_lossy().into_owned(),
    );
    let json = ok(&env, "studio list --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let rows = v["studios"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{json}");
    assert_eq!(rows[0]["name"], "acme-shop", "{json}");
    // A pinned base is no core directory, so it names no core id.
    assert!(rows[0]["core"].is_null(), "{json}");
    assert_eq!(v["pinned"], true, "{json}");

    let table = ok(&env, "studio list");
    assert!(table.contains("acme-shop"), "{table}");
    assert!(!table.contains("beta-tools"), "{table}");
}

/// A ledger that cannot be read is still listed — as `failed`, with
/// no title or path to claim.
#[test]
fn a_broken_ledger_lists_as_failed() {
    let root = TestDir::new("backlot-xdg");
    let dir = store::studio_dir(&core(&root, "core-a"), "broken");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("events.jsonl"), "not json\n").unwrap();
    let env = shell(&root);

    let json = ok(&env, "studio list --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let row = &v["studios"][0];
    assert_eq!(row["name"], "broken", "{json}");
    assert_eq!(row["state"], "failed", "{json}");
    assert!(row["title"].is_null(), "{json}");
    assert!(row["repo"].is_null(), "{json}");

    assert!(ok(&env, "studio list").contains("failed"));
}

/// No studio anywhere is a plain line, and an empty JSON list.
#[test]
fn no_studios_is_an_empty_list() {
    let root = TestDir::new("backlot-xdg");
    fs::create_dir_all(core(&root, "core-a")).unwrap();
    let env = shell(&root);

    assert!(ok(&env, "studio list").contains("no studios"));
    let json = ok(&env, "studio list --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["studios"].as_array().unwrap().len(), 0, "{json}");
    assert_eq!(v["pinned"], false, "{json}");
}

/// The table keeps a core id short; the JSON keeps it whole.
#[test]
fn the_table_shortens_the_core_id() {
    let root = TestDir::new("backlot-xdg");
    let id = "core-aaaabbbbcccc";
    write_ledger(
        &core(&root, id),
        "acme-shop",
        &[ev(1, 1_000, created("Shop", "/home/user/acme-shop"))],
    );
    let env = shell(&root);

    let json = ok(&env, "studio list --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["studios"][0]["core"], id, "{json}");

    let table = ok(&env, "studio list");
    assert!(table.contains("core-aaa"), "{table}");
    assert!(!table.contains(id), "the table shows the long id: {table}");
}

/// A pinned read names the local core in the table: `core.json`'s
/// name when the state directory carries one, else `local` — never a
/// bare `-`.
#[test]
fn the_table_names_the_local_core() {
    let root = TestDir::new("backlot-xdg");
    let core_a = core(&root, "core-a");
    write_ledger(
        &core_a,
        "acme-shop",
        &[ev(1, 1_000, created("Shop", "/home/user/acme-shop"))],
    );
    let mut env = shell(&root);
    env.vars.insert(
        "BACKLOT_STATE_DIR".into(),
        core_a.to_string_lossy().into_owned(),
    );

    let table = ok(&env, "studio list");
    let row = table
        .lines()
        .find(|line| line.starts_with("acme-shop"))
        .unwrap_or_else(|| panic!("no acme-shop row: {table}"));
    assert!(
        row.contains(" local "),
        "an unbound state is local: {table}"
    );

    fs::write(
        core_a.join("core.json"),
        r#"{"id":"core-a","name":"host-a"}"#,
    )
    .unwrap();
    let table = ok(&env, "studio list");
    let row = table
        .lines()
        .find(|line| line.starts_with("acme-shop"))
        .unwrap_or_else(|| panic!("no acme-shop row: {table}"));
    assert!(
        row.contains(" host-a "),
        "core.json names the core: {table}"
    );
    assert!(!row.contains(" local "), "{table}");
}

/// The same rule on a scanned read: each core's own `core.json` names
/// the `core` cell instead of the bare directory id.
#[test]
fn a_scanned_cores_document_names_the_core_cell() {
    let root = TestDir::new("backlot-xdg");
    let core_a = core(&root, "core-a");
    write_ledger(
        &core_a,
        "acme-shop",
        &[ev(1, 1_000, created("Shop", "/home/user/acme-shop"))],
    );
    fs::write(
        core_a.join("core.json"),
        r#"{"id":"core-a","name":"host-a"}"#,
    )
    .unwrap();
    let env = shell(&root);

    let table = ok(&env, "studio list");
    let row = table
        .lines()
        .find(|line| line.starts_with("acme-shop"))
        .unwrap_or_else(|| panic!("no acme-shop row: {table}"));
    assert!(
        row.contains(" host-a "),
        "core.json names the core: {table}"
    );
    // The JSON still carries the directory id, not the display name.
    let json = ok(&env, "studio list --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["studios"][0]["core"], "core-a", "{json}");
}

/// `studio list --help` says what the default read covers and how a
/// remote read is asked for.
#[test]
fn studio_list_help_names_the_local_and_remote_reads() {
    let root = TestDir::new("backlot-xdg");
    let env = shell(&root);
    let help = ok(&env, "studio list --help");
    assert!(help.contains("local state directory"), "{help}");
    assert!(help.contains("--remote <host>"), "{help}");
    assert!(
        help.contains("backlot --remote host-b studio list"),
        "{help}"
    );
}

/// The help's leading-flag form is real: `backlot --remote host-b
/// studio list` parses into the same command as the trailing form.
#[test]
fn a_leading_remote_flag_parses_before_the_command() {
    let words: Vec<String> = ["--remote", "host-b", "studio", "list"]
        .iter()
        .map(|w| w.to_string())
        .collect();
    let (common, cmd) = crate::args::parse(&words).expect("the documented form parses");
    assert_eq!(common.remote.as_deref(), Some("host-b"));
    assert!(matches!(
        cmd,
        crate::args::Cmd::StudioList {
            json: false,
            archived: false
        }
    ));
}
