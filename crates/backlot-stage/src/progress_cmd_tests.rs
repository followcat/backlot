//! Integration & unit tests for D5: `backlot progress`, liveness
//! `deepest_descendant` (`doing`), `live.json`, and `backlot status`
//! text + `--json`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use backlot_model::event::{Event, EventBy, EventKind, LEDGER_VERSION, MAX_DOING_CHARS};
use backlot_model::hollywood::HollywoodKind;
use backlot_model::state::replay;
use backlot_model::summary::{RoleDoing, RoleProgress, Work};
use backlot_model::testkit::*;

use crate::cli::{Env, Fail, run};
use crate::cli_testkit::*;
use crate::live::{LiveView, from_summary, read_fresh, write};
use crate::liveness::{Evidence, ObserveInput};
use crate::liveness_probe::ProcInfo;
use crate::render::status;
use crate::store;

fn args(s: &str) -> Vec<String> {
    s.split(' ').map(String::from).collect()
}

fn argv_ok(env: &Env, argv: &[&str]) -> String {
    let owned: Vec<String> = argv.iter().map(|s| (*s).to_string()).collect();
    match run(&owned, env) {
        Ok(out) => out,
        Err(Fail::Usage(m)) => panic!("usage error on '{argv:?}': {m}"),
        Err(Fail::Refused(m) | Fail::Core(m)) => panic!("refused on '{argv:?}': {m}"),
    }
}

fn clone_env(env: &Env) -> Env {
    Env {
        vars: env.vars.clone(),
    }
}

fn proc_node(pid: u32, ppid: u32, depth: u32, argv: &str) -> ProcInfo {
    ProcInfo {
        pid,
        ppid,
        depth,
        cpu_jiffies: 10,
        child_cpu_jiffies: 0,
        io_bytes: 0,
        wchan: String::new(),
        argv: argv.to_string(),
    }
}

fn obs<'a>(now_ms: u64, pid: Option<u32>, screen: &'a str) -> ObserveInput<'a> {
    ObserveInput {
        now_ms,
        runner: "fake",
        eligible: true,
        raw_screen: screen,
        raw_same_since_ms: now_ms,
        signal: None,
        terminal_working: false,
        activity_revision: None,
        local_machine: true,
        pid,
        work_roots: &[],
        ledger: None,
    }
}

#[test]
fn progress_cli_requires_role_env_and_appends_normalized_event() {
    let (scratch, base_env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &base_env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );

    // Outside role env: usage error (exit 2 shape: Fail::Usage) naming env vars.
    let mut no_env = Env {
        vars: BTreeMap::from([(
            "BACKLOT_STATE_DIR".into(),
            base_env.vars["BACKLOT_STATE_DIR"].clone(),
        )]),
    };
    let err = match run(&args("progress hi"), &no_env) {
        Err(Fail::Usage(m)) => m,
        other => panic!("expected Fail::Usage outside role env, got {other:?}"),
    };
    assert!(
        err.contains("BACKLOT_ROLE") && err.contains("BACKLOT_STUDIO"),
        "{err}"
    );

    // Passing --studio / --as flags without role env is still outside a role env.
    let err = match run(
        &args("progress hi --studio acme-shop --as producer"),
        &no_env,
    ) {
        Err(Fail::Usage(m)) => m,
        other => panic!("expected Fail::Usage with flags only, got {other:?}"),
    };
    assert!(err.contains("BACKLOT_ROLE"), "{err}");

    // Only BACKLOT_ROLE set (non-hollywood) -> missing BACKLOT_STUDIO.
    no_env.vars.insert("BACKLOT_ROLE".into(), "producer".into());
    let err = match run(&args("progress hi"), &no_env) {
        Err(Fail::Usage(m)) => m,
        other => panic!("expected Fail::Usage without BACKLOT_STUDIO, got {other:?}"),
    };
    assert!(err.contains("BACKLOT_STUDIO"), "{err}");

    // Producer in role env: records role.progress with task = None and control chars stripped.
    let mut role_env = clone_env(&base_env);
    role_env
        .vars
        .insert("BACKLOT_ROLE".into(), "producer".into());
    let out = argv_ok(&role_env, &["progress", "checking\x07\nthe\tplan  "]);
    assert!(out.contains("role.progress"), "{out}");

    // `--file` works like `tell`, capping at 200 chars.
    let mut long_body = "x".repeat(220);
    long_body.push_str("\nsecond line\x00");
    let pfile = fixture(&scratch, "prog.txt", &long_body);
    argv_ok(&role_env, &["progress", "--file", &pfile]);

    // Actor in role env: records its task.
    let req = fixture(&scratch, "req.md", "# spec\n");
    ok(
        &base_env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    ok(
        &base_env,
        &format!(
            "task new --title T --kind k --acceptance-file {} --as director",
            fixture(&scratch, "acc.md", "ok")
        ),
    );
    ok(
        &base_env,
        "cast --task t1 --actor actor-a --reason r --as director",
    );
    let mut actor_env = clone_env(&base_env);
    actor_env
        .vars
        .insert("BACKLOT_ROLE".into(), "actor-t1-c1".into());
    actor_env.vars.insert("BACKLOT_TASK".into(), "t1".into());
    argv_ok(&actor_env, &["progress", "running the parity suite"]);

    let (events, _) = store::load_events(&store::studio_dir(&scratch.dir, "acme-shop")).unwrap();
    let prog_events: Vec<&Event> = events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::RoleProgress { .. }))
        .collect();
    assert_eq!(prog_events.len(), 3);
    assert_eq!(
        prog_events[0].kind,
        EventKind::RoleProgress {
            role: "producer".into(),
            task: None,
            text: "checking the plan".into(),
        }
    );
    if let EventKind::RoleProgress { text, .. } = &prog_events[1].kind {
        assert_eq!(text.chars().count(), 200);
        assert!(!text.chars().any(char::is_control));
    } else {
        unreachable!();
    }
    assert_eq!(
        prog_events[2].kind,
        EventKind::RoleProgress {
            role: "actor-t1-c1".into(),
            task: Some("t1".into()),
            text: "running the parity suite".into(),
        }
    );

    // CLI help works.
    let help = ok(&base_env, "progress --help");
    assert!(help.contains("progress"), "{help}");

    // Hollywood role env records onto hollywood ledger cleanly without BACKLOT_STUDIO.
    let mut hw_env = no_env;
    hw_env
        .vars
        .insert("BACKLOT_ROLE".into(), "hollywood".into());
    let hw_out = argv_ok(&hw_env, &["progress", "checking acme-shop"]);
    assert!(hw_out.contains("role.progress"), "{hw_out}");
    let (hevents, _) = store::load_hevents(&store::hollywood_dir(&scratch.dir)).unwrap();
    assert!(matches!(
        &hevents[0].kind,
        HollywoodKind::RoleProgress { text } if text == "checking acme-shop"
    ));
}

#[test]
fn liveness_tracks_deepest_descendant_cmd_and_since_ms() {
    let procs = Arc::new(Mutex::new(vec![
        proc_node(100, 1, 0, "claude"),
        proc_node(101, 100, 1, "cargo test"),
    ]));
    let procs_r = Arc::clone(&procs);
    let read_procs: Box<crate::liveness_probe::ProcRead> =
        Box::new(move |_| procs_r.lock().unwrap().clone());
    let read_work: Box<crate::liveness_diff::WorkAreaRead> = Box::new(|_, _, _| Default::default());
    let mut git_fn = |_: &[&str], _: &str| crate::gitcmd::GitOut {
        code: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    let git: &mut crate::workareas::GitRun<'_> = &mut git_fn;

    let mut ev = Evidence::new(&obs(10_000, Some(100), "s"), &*read_procs, &*read_work, git);
    assert_eq!(ev.deepest_cmd(), Some(("cargo test", 10_000)));

    // Same argv 10s later keeps original since_ms.
    ev.observe(&obs(20_000, Some(100), "s"), &*read_procs, &*read_work, git);
    assert_eq!(ev.deepest_cmd(), Some(("cargo test", 10_000)));

    // Deeper or new descendant replaces argv and stamps new since_ms.
    *procs.lock().unwrap() = vec![
        proc_node(100, 1, 0, "claude"),
        proc_node(101, 100, 1, "cargo test"),
        proc_node(102, 101, 2, "rustc --test"),
    ];
    ev.observe(&obs(30_000, Some(100), "s"), &*read_procs, &*read_work, git);
    assert_eq!(ev.deepest_cmd(), Some(("rustc --test", 30_000)));

    // Descendant gone clears deepest_cmd.
    *procs.lock().unwrap() = vec![proc_node(100, 1, 0, "claude")];
    ev.observe(&obs(40_000, Some(100), "s"), &*read_procs, &*read_work, git);
    assert_eq!(ev.deepest_cmd(), None);
}

#[test]
fn live_and_status_text_and_json_include_progress_and_doing() {
    let (scratch, base_env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &base_env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let req = fixture(&scratch, "req.md", "# spec\n");
    ok(
        &base_env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    ok(
        &base_env,
        &format!(
            "task new --title T --kind k --acceptance-file {} --as director",
            fixture(&scratch, "acc.md", "ok")
        ),
    );
    ok(
        &base_env,
        "cast --task t1 --actor actor-a --reason r --as director",
    );

    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    // Record RoleStarted for producer and actor-t1-c1 and RoleProgress on actor.
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(store::Pending {
            event: backlot_model::NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "producer".into(),
                    terminal: Some("t1".into()),
                    actor: Some(actor("prod")),
                    session_id: None,
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(store::Pending {
            event: backlot_model::NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "actor-t1-c1".into(),
                    terminal: Some("t2".into()),
                    actor: Some(actor("a")),
                    session_id: None,
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(store::Pending {
            event: backlot_model::NewEvent::new(
                EventBy::role("actor-t1-c1"),
                EventKind::RoleProgress {
                    role: "actor-t1-c1".into(),
                    task: Some("t1".into()),
                    text: "running the parity suite".into(),
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    let (events, _) = store::load_events(&dir).unwrap();
    let state = replay(&events);
    let now_ms = crate::live::now_ms();

    let mut actor_started = state.roles["actor-t1-c1"].clone();
    actor_started.since_ms = now_ms.saturating_sub(47 * 60_000);
    let mut patched_state = state.clone();
    patched_state
        .roles
        .insert("actor-t1-c1".into(), actor_started);
    let p_ts = now_ms.saturating_sub(12 * 60_000);
    patched_state.progress.insert(
        "actor-t1-c1".into(),
        RoleProgress {
            text: "running the parity suite".into(),
            ts_ms: p_ts,
        },
    );

    let doing_map = BTreeMap::from([(
        ("acme-shop".to_string(), "actor-t1-c1".to_string()),
        RoleDoing {
            cmd: "cargo test".into(),
            since_ms: now_ms.saturating_sub(3 * 60_000),
        },
    )]);
    let work = BTreeMap::from([
        (
            ("acme-shop".to_string(), "actor-t1-c1".to_string()),
            Work::Working,
        ),
        (
            ("acme-shop".to_string(), "producer".to_string()),
            Work::Waiting,
        ),
    ]);
    let doc = backlot_model::summary::summary_with_doing(
        &[("acme-shop".into(), patched_state.clone())],
        &BTreeMap::new(),
        &work,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &doing_map,
        now_ms,
    );
    let live = from_summary(&doc, now_ms);
    write(&scratch.dir, &live).unwrap();
    let fresh = read_fresh(&scratch.dir, now_ms).unwrap();
    assert_eq!(fresh.roles.len(), 2);
    let live_view = LiveView::read(&scratch.dir, now_ms);

    let text = status(
        "acme-shop",
        &patched_state,
        &Default::default(),
        now_ms,
        &live_view,
    );
    assert!(
        text.contains("working 47m · \"running the parity suite\" 12m ago · cargo test 3m"),
        "unexpected working status line:\n{text}"
    );
    let prod_line = text.lines().find(|l| l.contains("producer")).unwrap();
    assert!(prod_line.contains("· waiting"), "{prod_line}");
    assert!(!prod_line.contains("ago"), "{prod_line}");

    // Real CLI `backlot status --json` includes progress + doing.
    let json_str = ok(&base_env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(
        v["roles"]["actor-t1-c1"]["progress"]["text"].as_str(),
        Some("running the parity suite"),
        "{json_str}"
    );
    assert_eq!(
        v["roles"]["actor-t1-c1"]["doing"]["cmd"].as_str(),
        Some("cargo test"),
        "{json_str}"
    );

    // Unused import guard for MAX_DOING_CHARS check
    let _ = MAX_DOING_CHARS;
    let _ = LEDGER_VERSION;
}
