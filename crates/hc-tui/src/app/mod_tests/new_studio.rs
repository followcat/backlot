//! U6b New Studio dialog: entry points, the step flow, the Esc
//! rule, the exact `studio new` argv on a remote machine, the
//! cast table, the `^E` reload and focus-on-create — through an
//! injected runner, never a spawned binary. Neutral fixtures only
//! (`host-b`, `acme-shop`, `beta-tools`, `/home/user`).

use super::endpoint_nav::app_multi;
use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput, UiEvent};
use crate::new_studio::{Probe, RepoKind, Step, Step1Pane};
use crate::sidebar::fixture::{machine, term};
use hc_proto::watch::{DocKey, Document};

fn key(app: &mut App, k: hc_vt::Key) {
    key_mods(app, k, hc_vt::Mods::NONE);
}

fn key_mods(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

/// A call the injected runner recorded: machine, argv, stdin.
type Recorded =
    std::sync::Arc<std::sync::Mutex<Vec<(Option<String>, Vec<String>, Option<String>)>>>;

fn base_answers() -> Vec<(&'static str, Result<String, String>)> {
    vec![
        ("--version", Ok("backlot 0.1.0\n".into())),
        ("repos --json", Ok(repos_json())),
        ("runners --json", Ok(runners_json())),
        ("studio new", Ok("studio 'acme-shop' created\n".into())),
        ("stage start", Ok("stage started\n".into())),
    ]
}

/// Install the dialog's runner seam on `sink`: every call records,
/// the answer is the longest matching prefix's canned output.
fn run_on(
    sink: Recorded,
    answers: Vec<(String, Result<String, String>)>,
) -> crate::app::new_studio::NsRun {
    std::sync::Arc::new(move |machine, argv, stdin| {
        sink.lock().unwrap().push((machine, argv.clone(), stdin));
        let joined = argv.join(" ");
        let out = answers
            .iter()
            .find(|(k, _)| joined.starts_with(k.as_str()))
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| Err(format!("unstubbed call: {joined}")));
        Box::pin(async move { out })
    })
}

/// Install the canned `base_answers` runner.
fn install_run(app: &mut App, answers: &[(&'static str, Result<String, String>)]) -> Recorded {
    let recorded: Recorded = Default::default();
    let answers: Vec<(String, Result<String, String>)> = answers
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    app.set_ns_run(run_on(recorded.clone(), answers));
    recorded
}

/// Let the spawned probe tasks send, then land every reply.
async fn drain_ns(app: &mut App) {
    for _ in 0..20 {
        tokio::task::yield_now().await;
        while let Ok(reply) = app.ns_rx.try_recv() {
            app.on_ns_reply(reply);
        }
    }
}

fn repos_json() -> String {
    serde_json::json!({
        "cwd": "/home/user",
        "entries": [
            { "name": "acme-shop", "path": "/home/user/acme-shop",
              "kind": "git_ready", "in_use_studio": null, "branch": "main" },
            { "name": "beta-tools", "path": "/home/user/beta-tools",
              "kind": "git_empty", "in_use_studio": null, "branch": null },
            { "name": "taken", "path": "/home/user/taken",
              "kind": "git_ready", "in_use_studio": "taken-st", "branch": null },
            { "name": "notes", "path": "/home/user/notes",
              "kind": "dir", "in_use_studio": null, "branch": null }
        ]
    })
    .to_string()
}

fn runners_json() -> String {
    serde_json::json!([
        { "runner": "fake", "status": "found", "version": "1",
          "models": ["m"], "efforts": ["low", "high"] },
        { "runner": "claude", "status": "missing", "version": null,
          "models": ["sonnet"], "efforts": ["low", "medium", "high"] }
    ])
    .to_string()
}

/// An app in studio mode with the dialog open, the Local machine's
/// version+repos already answered.
async fn dialog_app() -> (App, Recorded) {
    let mut app = app_multi(vec![term("t1")], vec![machine("host-b", "connected")]);
    app.set_plain(false);
    app.focused = Some("t1".into());
    let recorded = install_run(&mut app, &base_answers());
    app.new_studio_open(None);
    drain_ns(&mut app).await;
    (app, recorded)
}

/// Walk step 1 → 4: pick the repo, pass step 2, draft the cast.
async fn to_step4(app: &mut App) {
    // Repo pane, first row is `acme-shop` (git_ready) — Enter picks.
    // A preselected machine already sits on the Repos pane.
    if app.new_studio.as_ref().unwrap().pane == Step1Pane::Machines {
        key(app, hc_vt::Key::Tab);
    }
    key(app, hc_vt::Key::Enter);
    assert_eq!(app.new_studio.as_ref().unwrap().step, Step::IdTitle);
    key(app, hc_vt::Key::Enter);
    drain_ns(app).await;
    assert_eq!(app.new_studio.as_ref().unwrap().step, Step::Cast);
    key(app, hc_vt::Key::Enter);
    drain_ns(app).await;
    assert_eq!(app.new_studio.as_ref().unwrap().step, Step::CheckStart);
}

fn publish_summary(app: &mut App, machine: Option<&str>, value: serde_json::Value) {
    app.view.documents_mut().insert(
        DocKey {
            machine: machine.map(str::to_owned),
            name: "backlot".into(),
        },
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
}

// ---------- entry points ----------

#[tokio::test]
async fn every_entry_point_opens_the_dialog() {
    // The sidebar's `new · <machine>` preselects its machine.
    let mut app = app_multi(vec![term("t1")], vec![machine("host-b", "connected")]);
    app.set_plain(false);
    let recorded = install_run(&mut app, &base_answers());
    app.new_studio_open(Some("host-b".into()));
    drain_ns(&mut app).await;
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.machine_sel, 1, "host-b preselected");
    assert_eq!(d.pane, Step1Pane::Repos);
    assert!(
        recorded
            .lock()
            .unwrap()
            .iter()
            .any(|(m, argv, _)| m.as_deref() == Some("host-b")
                && argv.starts_with(&["--version".to_string()])),
        "the preselected machine got its version probe"
    );
    // `n` on the opening page lands on the same opener.
    app.new_studio = None;
    app.opening.settled = true;
    key(&mut app, hc_vt::Key::Char('n'));
    assert!(app.new_studio.is_some(), "n opened the dialog");
}

#[tokio::test]
async fn plain_never_opens_the_dialog() {
    let mut app = app();
    app.set_plain(true);
    app.new_studio_open(None);
    assert!(app.new_studio.is_none());
}

// ---------- step 1 ----------

#[tokio::test]
async fn machines_list_local_and_remote_with_states() {
    let (app, _) = dialog_app().await;
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.machines.len(), 2);
    assert_eq!(d.machines[0].label, "Local");
    assert!(matches!(d.machines[0].backlot, Probe::Done(ref v) if v == "0.1.0"));
    assert_eq!(d.machines[1].label, "host-b");
    assert!(d.machines[0].usable());
}

#[tokio::test]
async fn a_machine_without_backlot_disables_enter() {
    let mut app = app_multi(vec![term("t1")], vec![machine("host-c", "connected")]);
    app.set_plain(false);
    let recorded: Recorded = Default::default();
    let answers: Vec<(String, Result<String, String>)> = base_answers()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    let sink = recorded.clone();
    app.set_ns_run(std::sync::Arc::new(move |machine, argv, stdin| {
        sink.lock()
            .unwrap()
            .push((machine.clone(), argv.clone(), stdin));
        let joined = argv.join(" ");
        let out = if joined == "--version" && machine.as_deref() == Some("host-c") {
            Err("backlot is not installed on host-c".into())
        } else {
            answers
                .iter()
                .find(|(k, _)| joined.starts_with(k.as_str()))
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| Err(format!("unstubbed call: {joined}")))
        };
        Box::pin(async move { out })
    }));
    app.new_studio_open(None);
    drain_ns(&mut app).await;
    let d = app.new_studio.as_mut().unwrap();
    d.machine_sel = 1;
    assert!(!d.machine().unwrap().usable());
    let error = d.step1_enter_error().unwrap();
    assert!(error.contains("not installed"), "{error}");
}

#[tokio::test]
async fn a_git_empty_repo_is_disabled_with_its_hint() {
    let (mut app, _) = dialog_app().await;
    {
        let d = app.new_studio.as_mut().unwrap();
        d.pane = Step1Pane::Repos;
        d.repos.sel = 1; // beta-tools is git_empty.
        assert_eq!(d.repo().unwrap().kind, RepoKind::GitEmpty);
    }
    key(&mut app, hc_vt::Key::Enter);
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.step, Step::MachineRepo, "a git_empty repo stays");
    assert_eq!(
        d.step1_enter_error().as_deref(),
        Some("git repo has no commits — commit first")
    );
}

#[tokio::test]
async fn an_in_use_repo_warns_but_passes() {
    let (mut app, _) = dialog_app().await;
    let d = app.new_studio.as_mut().unwrap();
    d.pane = Step1Pane::Repos;
    d.repos.sel = 2; // taken — in use by taken-st.
    assert_eq!(d.repo().unwrap().in_use_studio.as_deref(), Some("taken-st"));
    assert!(
        d.step1_enter_error().is_none(),
        "in-use is a warning, not a gate"
    );
}

// ---------- step 2 ----------

#[tokio::test]
async fn step2_prefills_and_validates_the_id() {
    let (mut app, _) = dialog_app().await;
    app.new_studio.as_mut().unwrap().studios = vec!["taken".into()];
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::Enter);
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.step, Step::IdTitle);
    assert_eq!(d.id.as_str(), "acme-shop");
    assert_eq!(d.title.as_str(), "acme shop");
    assert!(crate::new_studio::id_error(d.id.as_str(), &d.studios).is_ok());
    // A duplicate on this machine refuses.
    assert!(crate::new_studio::id_error("taken", &d.studios).is_err());
    // Pattern: leading '-' is out, as is a cap.
    assert!(crate::new_studio::id_error("-x", &d.studios).is_err());
    assert!(crate::new_studio::id_error("UPPER", &d.studios).is_err());
    assert!(crate::new_studio::id_error("", &d.studios).is_err());
}

#[tokio::test]
async fn a_duplicate_id_blocks_the_step() {
    let (mut app, _) = dialog_app().await;
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::Enter);
    app.new_studio.as_mut().unwrap().studios = vec!["acme-shop".into()];
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        app.new_studio.as_ref().unwrap().step,
        Step::IdTitle,
        "the dup stays on step 2"
    );
}

// ---------- step 3 ----------

#[tokio::test]
async fn the_cast_table_cycles_and_edits() {
    let (mut app, _) = dialog_app().await;
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::Enter);
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.step, Step::Cast);
    assert_eq!(d.cast.rows.len(), 5, "three roles + two actors");
    assert_eq!(d.cast.rows[0].key, "producer");
    // Cycle the runner cell: fake → claude → fake.
    key(&mut app, hc_vt::Key::ArrowRight);
    {
        let d = app.new_studio.as_ref().unwrap();
        assert_eq!(d.cast.rows[0].runner, 1);
    }
    key(&mut app, hc_vt::Key::ArrowRight);
    {
        let d = app.new_studio.as_ref().unwrap();
        assert_eq!(d.cast.rows[0].runner, 0);
    }
    // `a` adds actor-c, `x` removes it; a fixed role cannot go.
    let before = app.new_studio.as_ref().unwrap().cast.rows.len();
    key(&mut app, hc_vt::Key::Char('a'));
    assert_eq!(app.new_studio.as_ref().unwrap().cast.rows.len(), before + 1);
    key(&mut app, hc_vt::Key::Char('x'));
    assert_eq!(app.new_studio.as_ref().unwrap().cast.rows.len(), before);
    {
        let d = app.new_studio.as_mut().unwrap();
        d.cast.sel = 0; // producer — a fixed role.
    }
    key(&mut app, hc_vt::Key::Char('x'));
    assert_eq!(app.new_studio.as_ref().unwrap().cast.rows.len(), before);
}

#[tokio::test]
async fn no_runners_found_shows_the_banner() {
    let mut app = app_multi(vec![term("t1")], vec![]);
    app.set_plain(false);
    let mut answers = base_answers();
    answers.retain(|(k, _)| *k != "runners --json");
    answers.push(("runners --json", Ok("[]".into())));
    install_run(&mut app, &answers);
    app.new_studio_open(None);
    drain_ns(&mut app).await;
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::Enter);
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    let backend = draw_to(&mut app, 120, 36);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("No runners"), "banner expected:\n{text}");
}

// ---------- step 4 ----------

#[tokio::test]
async fn create_sends_the_exact_argv_and_cast_on_stdin() {
    let (mut app, recorded) = dialog_app().await;
    to_step4(&mut app).await;
    // The dry run already fired — create next.
    recorded.lock().unwrap().clear();
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    let calls = recorded.lock().unwrap();
    let create = calls
        .iter()
        .find(|(_, argv, _)| argv.starts_with(&["studio".into(), "new".into()]))
        .expect("the create call");
    assert!(create.0.is_none(), "Local create");
    let argv = &create.1;
    let want = [
        "studio",
        "new",
        "--studio",
        "acme-shop",
        "--title",
        "acme shop",
        "--repo",
        "/home/user/acme-shop",
        "--cast",
        "-",
    ];
    assert_eq!(&argv[..want.len()], want, "{argv:?}");
    let cast = create.2.as_deref().expect("cast on stdin");
    assert!(cast.contains("[producer]"), "{cast}");
    assert!(cast.contains("runner = \"fake\""), "{cast}");
    // `stage start` follows on the same machine.
    assert!(
        calls
            .iter()
            .any(|(m, argv, _)| m.is_none() && argv.as_slice() == ["stage", "start"])
    );
    assert!(app.new_studio.is_none(), "success closes the dialog");
}

#[tokio::test]
async fn a_remote_create_runs_on_that_machine() {
    let mut app = app_multi(vec![term("t1")], vec![machine("host-b", "connected")]);
    app.set_plain(false);
    let recorded = install_run(&mut app, &base_answers());
    app.new_studio_open(Some("host-b".into()));
    drain_ns(&mut app).await;
    to_step4(&mut app).await;
    recorded.lock().unwrap().clear();
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    let calls = recorded.lock().unwrap();
    assert!(
        calls
            .iter()
            .filter(|(_, argv, _)| argv.starts_with(&["studio".into(), "new".into()]))
            .all(|(m, _, _)| m.as_deref() == Some("host-b")),
        "studio new ran on host-b: {calls:?}"
    );
    assert!(calls.iter().any(
        |(m, argv, _)| m.as_deref() == Some("host-b") && argv.as_slice() == ["stage", "start"]
    ));
}

#[tokio::test]
async fn the_dry_run_argv_and_a_verbatim_error() {
    let (mut app, recorded) = dialog_app().await;
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::Enter);
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    // A failing dry run lands its text verbatim — still recording.
    let sink = recorded.clone();
    app.set_ns_run(std::sync::Arc::new(move |machine, argv, stdin| {
        sink.lock().unwrap().push((machine, argv.clone(), stdin));
        let joined = argv.join(" ");
        let out = if joined.contains("--dry-run") {
            Err("repo dirty: 3 tracked files\nuncommitted changes".to_string())
        } else if joined == "runners --json" {
            Ok(runners_json())
        } else {
            Ok(String::new())
        };
        Box::pin(async move { out })
    }));
    recorded.lock().unwrap().clear();
    app.new_studio.as_mut().unwrap().step = Step::CheckStart;
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    let d = app.new_studio.as_ref().unwrap();
    match &d.dry {
        Probe::Failed(text) => {
            assert_eq!(text, "repo dirty: 3 tracked files\nuncommitted changes");
        }
        other => panic!("expected the verbatim error, got {other:?}"),
    }
    let calls = recorded.lock().unwrap();
    assert!(
        calls
            .iter()
            .any(|(_, argv, _)| argv.last().is_some_and(|a| a == "--dry-run"))
    );
}

#[tokio::test]
async fn focus_moves_to_the_producer_after_create() {
    let (mut app, _) = dialog_app().await;
    to_step4(&mut app).await;
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    assert!(app.new_studio.is_none());
    // The publish arrives with the new studio's producer on t9.
    publish_summary(
        &mut app,
        None,
        serde_json::json!({
            "schema": "backlot/1", "updated_ms": 3,
            "studios": [{
                "id": "acme-shop", "title": "acme shop", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [{ "role": "producer", "name": "producer",
                            "terminal": "t9", "state": "live" }]
            }]
        }),
    );
    assert_eq!(app.focused.as_deref(), Some("t9"));
    assert!(app.ns_pending_focus.is_none());
}

// ---------- the Esc rule ----------

#[tokio::test]
async fn esc_steps_back_then_closes_and_never_reaches_a_program() {
    let (mut app, _) = dialog_app().await;
    to_step4(&mut app).await;
    let before = app.pane.input_backlog().0;
    let id = app.new_studio.as_ref().unwrap().id.as_str().to_string();
    for want in [Step::Cast, Step::IdTitle, Step::MachineRepo] {
        key(&mut app, hc_vt::Key::Escape);
        assert_eq!(app.new_studio.as_ref().unwrap().step, want);
    }
    // Values kept along the way back.
    assert_eq!(app.new_studio.as_ref().unwrap().id.as_str(), id);
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.new_studio.is_none(), "the last Esc closes");
    assert_eq!(
        app.pane.input_backlog().0,
        before,
        "a raw ^[ never reached a terminal"
    );
}

#[tokio::test]
async fn ctrl_c_cancels_from_any_step() {
    let (mut app, _) = dialog_app().await;
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.new_studio.as_ref().unwrap().step, Step::IdTitle);
    key_mods(&mut app, hc_vt::Key::Char('c'), hc_vt::Mods::CTRL);
    assert!(app.new_studio.is_none());
}

// ---------- mouse ----------

#[tokio::test]
async fn a_click_outside_closes_and_a_machine_click_selects() {
    let (mut app, _) = dialog_app().await;
    draw_to(&mut app, 120, 36);
    let machine_hit = app
        .ns_hits
        .cells
        .iter()
        .find_map(|(rect, hit)| match hit {
            crate::new_studio::NsHit::Machine(1) => Some((rect.x + 1, rect.y)),
            _ => None,
        })
        .expect("a host-b machine hit");
    app.on_mouse(MouseInput {
        col: machine_hit.0,
        row: machine_hit.1,
        kind: MouseEventKind::Down(MouseButton::Left),
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    });
    drain_ns(&mut app).await;
    assert_eq!(app.new_studio.as_ref().unwrap().machine_sel, 1);
    app.on_mouse(MouseInput {
        col: 0,
        row: 0,
        kind: MouseEventKind::Down(MouseButton::Left),
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    });
    assert!(app.new_studio.is_none(), "outside click closed");
}

// ---------- geometry ----------

#[tokio::test]
async fn every_step_draws_at_all_sizes_with_unclipped_hints() {
    let (mut app, _) = dialog_app().await;
    let sizes = [(160, 47), (120, 36), (80, 24), (60, 24), (40, 24)];
    // A marker each step must draw, plus `esc` — `fit_hints` picks a
    // tier that fits, so the hint row is whole at every width.
    for (w, h) in sizes {
        let backend = draw_to(&mut app, w, h);
        let text = screen_lines(&backend).join("\n");
        assert!(text.contains("Machine"), "step 1 at {w}x{h}:\n{text}");
        assert!(text.contains("esc"), "step 1 at {w}x{h}:\n{text}");
    }
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::Enter);
    for (w, h) in sizes {
        let backend = draw_to(&mut app, w, h);
        let text = screen_lines(&backend).join("\n");
        assert!(text.contains("ID"), "step 2 at {w}x{h}:\n{text}");
        assert!(text.contains("esc"), "step 2 at {w}x{h}:\n{text}");
    }
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    for (w, h) in sizes {
        let backend = draw_to(&mut app, w, h);
        let text = screen_lines(&backend).join("\n");
        assert!(text.contains("producer"), "step 3 at {w}x{h}:\n{text}");
        assert!(text.contains("esc"), "step 3 at {w}x{h}:\n{text}");
    }
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    for (w, h) in sizes {
        let backend = draw_to(&mut app, w, h);
        let text = screen_lines(&backend).join("\n");
        assert!(text.contains("dry-run"), "step 4 at {w}x{h}:\n{text}");
        assert!(text.contains("esc"), "step 4 at {w}x{h}:\n{text}");
    }
}

// ---------- the ^E editor ----------

#[tokio::test]
async fn a_broken_editor_file_shows_its_error_and_keeps_the_table() {
    let (mut app, _) = dialog_app().await;
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::Enter);
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    let file = std::env::temp_dir().join(format!("ns-test-cast-{}", std::process::id()));
    std::fs::write(&file, "limits = { fake = 0 }\n").unwrap();
    app.ns_editor = Some(file.clone());
    app.ns_editor_check();
    let d = app.new_studio.as_ref().unwrap();
    assert!(d.cast.custom_error.is_some());
    assert!(!d.cast.rows.is_empty(), "the table survived");
    assert!(!file.exists(), "the temp file was removed");
}

#[tokio::test]
async fn a_valid_editor_file_replaces_the_cast() {
    let (mut app, _) = dialog_app().await;
    let cast = "[producer]\nrunner = \"fake\"\nmodel = \"m\"\neffort = \"low\"\npermission_profile = \"standard\"\n\
                [director]\nrunner = \"fake\"\nmodel = \"m\"\neffort = \"low\"\npermission_profile = \"standard\"\n\
                [writer]\nrunner = \"fake\"\nmodel = \"m\"\neffort = \"low\"\npermission_profile = \"standard\"\n";
    let file = std::env::temp_dir().join(format!("ns-test-cast-ok-{}", std::process::id()));
    std::fs::write(&file, cast).unwrap();
    app.ns_editor = Some(file.clone());
    app.ns_editor_check();
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.cast.custom.as_deref(), Some(cast));
    assert!(d.cast.custom_error.is_none());
    assert!(!file.exists());
}
