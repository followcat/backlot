//! U6b New Studio dialog: entry points, the step flow, the Esc
//! rule, the exact `studio new` argv on a remote machine, the
//! cast table, the `^E` reload and focus-on-create — through an
//! injected runner, never a spawned binary. Neutral fixtures only
//! (`host-b`, `acme-shop`, `beta-tools`, `/home/user`).

use super::endpoint_nav::app_multi;
use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput, UiEvent};
use crate::new_studio::{Probe, RepoKind, Step, Step1Pane, Step2Field};
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
              "kind": "git_ready", "in_use_studio": "taken-stud-tool", "branch": null },
            { "name": "notes", "path": "/home/user/notes",
              "kind": "dir", "in_use_studio": null, "branch": null },
            { "name": "mono", "path": "/home/user/mono",
              "kind": "dir", "in_use_studio": null, "branch": null,
              "members": ["beta-api", "web"] }
        ]
    })
    .to_string()
}

/// The same home with dot-directories in the answer — what the
/// dialog's `--all` request returns.
fn repos_json_with_dots() -> String {
    serde_json::json!({
        "cwd": "/home/user",
        "entries": [
            { "name": ".cache", "path": "/home/user/.cache",
              "kind": "dir", "in_use_studio": null, "branch": null },
            { "name": ".config", "path": "/home/user/.config",
              "kind": "dir", "in_use_studio": null, "branch": null },
            { "name": "acme-shop", "path": "/home/user/acme-shop",
              "kind": "git_ready", "in_use_studio": null, "branch": "main" },
            { "name": "mono", "path": "/home/user/mono",
              "kind": "dir", "in_use_studio": null, "branch": null,
              "members": ["beta-api", "web"] },
            { "name": "notes", "path": "/home/user/notes",
              "kind": "dir", "in_use_studio": null, "branch": null }
        ]
    })
    .to_string()
}

fn notes_json() -> String {
    serde_json::json!({
        "cwd": "/home/user/notes",
        "entries": [
            { "name": "drafts", "path": "/home/user/notes/drafts",
              "kind": "dir", "in_use_studio": null, "branch": null },
            { "name": "notepad", "path": "/home/user/notes/notepad",
              "kind": "dir", "in_use_studio": null, "branch": null },
            { "name": "raw", "path": "/home/user/notes/raw",
              "kind": "dir", "in_use_studio": null, "branch": null }
        ]
    })
    .to_string()
}

fn drafts_json() -> String {
    serde_json::json!({
        "cwd": "/home/user/notes/drafts",
        "entries": [
            { "name": "ideas", "path": "/home/user/notes/drafts/ideas",
              "kind": "dir", "in_use_studio": null, "branch": null },
            { "name": "scratch", "path": "/home/user/notes/drafts/scratch",
              "kind": "dir", "in_use_studio": null, "branch": null }
        ]
    })
    .to_string()
}

/// `base_answers` plus the subdirectory listings — the longest key
/// first, the prefix matcher takes the first hit.
fn answers_with_dirs() -> Vec<(&'static str, Result<String, String>)> {
    let mut answers = vec![
        (
            "repos --json --from /home/user/notes/drafts",
            Ok(drafts_json()),
        ),
        ("repos --json --from /home/user/notes", Ok(notes_json())),
    ];
    answers.extend(base_answers());
    answers
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

/// Versions that repeat their runner's name — the banner must strip
/// the duplicate token rather than print `claude claude 2.1.0`.
fn runners_json_stuttering_versions() -> String {
    serde_json::json!([
        { "runner": "claude", "status": "found", "version": "claude 2.1.0",
          "models": ["sonnet"], "efforts": ["high"] },
        { "runner": "codex", "status": "found", "version": "codex 0.9.0",
          "models": ["o5"], "efforts": ["high"] },
        { "runner": "fake", "status": "found", "version": "fake-runner 1.0",
          "models": ["m"], "efforts": ["low"] }
    ])
    .to_string()
}

/// A runner whose catalog is long enough to scroll — `w-00`…`w-14`.
fn runners_json_wide() -> String {
    let models: Vec<String> = (0..15).map(|i| format!("w-{i:02}")).collect();
    serde_json::json!([
        { "runner": "wide", "status": "found", "version": "1",
          "models": models, "efforts": ["low", "high"] }
    ])
    .to_string()
}

/// A `needs_path` row — `cmd` found only off the launch PATH, like
/// an nvm install — seatable like any `found` runner.
fn runners_json_needs_path() -> String {
    serde_json::json!([
        { "runner": "fake", "status": "found", "version": "1",
          "models": ["m"], "efforts": ["low", "high"] },
        { "runner": "claude", "status": "missing", "version": null,
          "models": ["sonnet"], "efforts": ["low", "medium", "high"] },
        { "runner": "cmd", "status": "found", "version": "2",
          "path": "/home/user/.nvm/versions/node/v24.11.1/bin/cmd",
          "needs_path": true,
          "models": ["cmd-x"], "efforts": ["low"] }
    ])
    .to_string()
}

/// `base_answers` with the needs_path runner probed instead.
fn answers_with_needs_path_runner() -> Vec<(&'static str, Result<String, String>)> {
    let mut answers = base_answers();
    answers.retain(|(k, _)| *k != "runners --json");
    answers.push(("runners --json", Ok(runners_json_needs_path())));
    answers
}

/// `base_answers` with the wide-catalog runner probed instead.
fn answers_with_wide_runner() -> Vec<(&'static str, Result<String, String>)> {
    let mut answers = base_answers();
    answers.retain(|(k, _)| *k != "runners --json");
    answers.push(("runners --json", Ok(runners_json_wide())));
    answers
}

/// A cursor-shaped catalog: `auto` is already a real model id.
fn runners_json_auto() -> String {
    serde_json::json!([
        { "runner": "cur", "status": "found", "version": "1",
          "models": ["auto", "x-1", "x-2"], "efforts": [] }
    ])
    .to_string()
}

/// `base_answers` with the auto-catalog runner probed instead.
fn answers_with_auto_runner() -> Vec<(&'static str, Result<String, String>)> {
    let mut answers = base_answers();
    answers.retain(|(k, _)| *k != "runners --json");
    answers.push(("runners --json", Ok(runners_json_auto())));
    answers
}

/// A runner with no effort flag whose model ids carry the depth —
/// cursor/devin's shape.
fn runners_json_depth() -> String {
    serde_json::json!([
        { "runner": "deep", "status": "found", "version": "1",
          "models": ["x-1-high", "x-2"], "efforts": [] }
    ])
    .to_string()
}

/// `base_answers` with the depth-in-model runner probed instead.
fn answers_with_depth_runner() -> Vec<(&'static str, Result<String, String>)> {
    let mut answers = base_answers();
    answers.retain(|(k, _)| *k != "runners --json");
    answers.push(("runners --json", Ok(runners_json_depth())));
    answers
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

/// `base_answers` with a catalog whose runner and model names are
/// long enough to test truncation, plus a resolved binary path.
fn answers_with_long_names() -> Vec<(&'static str, Result<String, String>)> {
    let mut answers = base_answers();
    answers.retain(|(k, _)| *k != "runners --json");
    answers.push((
        "runners --json",
        Ok(serde_json::json!([
            { "runner": "claude", "status": "found", "version": "9",
              "path": "/home/user/.local/bin/claude",
              "models": ["claude-sonnet-5-5"], "efforts": ["low", "high"] }
        ])
        .to_string()),
    ));
    answers
}

/// Open the dialog and land on the Cast step, the draft seeded.
async fn to_cast(app: &mut App) {
    key(app, hc_vt::Key::Tab);
    key(app, hc_vt::Key::Enter);
    key(app, hc_vt::Key::Enter);
    drain_ns(app).await;
    assert_eq!(app.new_studio.as_ref().unwrap().step, Step::Cast);
}

/// The dialog's footer line, whatever tier it picked.
fn footer(backend: &ratatui::backend::TestBackend) -> String {
    screen_lines(backend)
        .into_iter()
        .rev()
        .find(|l| l.contains("tab") || l.contains("⏎"))
        .unwrap_or_default()
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
async fn an_empty_repo_picks_like_any_path() {
    let (mut app, _) = dialog_app().await;
    {
        let d = app.new_studio.as_mut().unwrap();
        d.pane = Step1Pane::Repos;
        d.repos.sel = 2; // the fixture's sorted list puts beta-tools third.
        assert_eq!(d.repo().unwrap().kind, RepoKind::GitEmpty);
    }
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        app.new_studio.as_ref().unwrap().step,
        Step::IdTitle,
        "an empty repository picks — the launch writes the initial commit"
    );
}

/// Pick `repos.sel`'s row and walk to step 4 — the pick's own
/// `studio new` argv then sits in the dry run's record.
async fn pick_to_step4(app: &mut App, sel: usize) {
    {
        let d = app.new_studio.as_mut().unwrap();
        d.pane = Step1Pane::Repos;
        d.repos.sel = sel;
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

#[tokio::test]
async fn a_plain_dir_rides_init_and_g_flips_to_no_git() {
    let (mut app, recorded) = dialog_app().await;
    pick_to_step4(&mut app, 4).await; // notes — a plain directory.
    let argv_of = |recorded: &Recorded| {
        recorded
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(_, a, _)| a.starts_with(&["studio".into(), "new".into()]))
            .map(|(_, a, _)| a.clone())
            .expect("a studio new call")
    };
    let argv = argv_of(&recorded);
    assert!(argv.contains(&"--init".into()), "{argv:?}");
    assert!(!argv.contains(&"--no-git".into()), "{argv:?}");
    // `g` toggles the no-git mode and re-runs the dry run with it.
    key(&mut app, hc_vt::Key::Char('g'));
    drain_ns(&mut app).await;
    let argv = argv_of(&recorded);
    assert!(argv.contains(&"--no-git".into()), "{argv:?}");
    assert!(!argv.contains(&"--init".into()), "{argv:?}");
    key(&mut app, hc_vt::Key::Char('g'));
    drain_ns(&mut app).await;
    let argv = argv_of(&recorded);
    assert!(argv.contains(&"--init".into()), "{argv:?}");
}

#[tokio::test]
async fn a_workspace_picks_with_no_git_flags() {
    let (mut app, recorded) = dialog_app().await;
    pick_to_step4(&mut app, 3).await; // mono — two member repos.
    let calls = recorded.lock().unwrap();
    let argv = calls
        .iter()
        .rev()
        .find(|(_, a, _)| a.starts_with(&["studio".into(), "new".into()]))
        .map(|(_, a, _)| a.clone())
        .expect("a studio new call");
    assert!(argv.contains(&"/home/user/mono".to_string()), "{argv:?}");
    assert!(!argv.contains(&"--init".into()), "{argv:?}");
    assert!(!argv.contains(&"--no-git".into()), "{argv:?}");
}

#[tokio::test]
async fn an_in_use_repo_warns_but_passes() {
    let (mut app, _) = dialog_app().await;
    let d = app.new_studio.as_mut().unwrap();
    d.pane = Step1Pane::Repos;
    d.repos.sel = 1; // taken — in use by taken-stud-tool.

    assert_eq!(
        d.repo().unwrap().in_use_studio.as_deref(),
        Some("taken-stud-tool")
    );
    assert!(
        d.step1_enter_error().is_none(),
        "in-use is a warning, not a gate"
    );
}

// ---------- the live filter ----------

#[tokio::test]
async fn typing_narrows_the_list_without_a_slash_mode() {
    let (mut app, _) = dialog_app().await;
    // No `/` press — the first printable char already filters.
    key(&mut app, hc_vt::Key::Char('a'));
    key(&mut app, hc_vt::Key::Char('c'));
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.repos.filter.as_str(), "ac");
    assert_eq!(d.repos.sel, 0, "an edit re-seats the selection");
    assert_eq!(d.pane, Step1Pane::Repos, "typing addresses the list");
    let names: Vec<&str> = d.repos.visible().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["acme-shop"], "the list narrows live");
    // Backspace widens it again.
    key(&mut app, hc_vt::Key::Backspace);
    let names: Vec<&str> = {
        let d = app.new_studio.as_ref().unwrap();
        d.repos.visible().iter().map(|e| e.name.as_str()).collect()
    };
    assert_eq!(names, ["acme-shop", "taken", "beta-tools"]);
    // Enter picks the filtered row and steps forward.
    key(&mut app, hc_vt::Key::Enter);
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.step, Step::IdTitle);
    assert_eq!(d.id.as_str(), "acme-shop");
}

#[tokio::test]
async fn slash_is_an_ordinary_filter_character_now() {
    let (mut app, _) = dialog_app().await;
    key(&mut app, hc_vt::Key::Char('/'));
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.repos.filter.as_str(), "/", "no mode — a literal char");
    assert!(d.repos.visible().is_empty(), "no name contains '/'");
}

/// `$HOME` opens on real work: dot-directories are hidden and the
/// git repository leads the workspace and the plain directory. A
/// query that starts with `.` reveals the dot-directories, so the
/// wizard can still reach `.config` when asked.
#[tokio::test]
async fn dot_directories_stay_hidden_until_the_filter_asks() {
    let mut app = app_multi(vec![term("t1")], vec![]);
    app.set_plain(false);
    let mut answers = base_answers();
    answers.retain(|(k, _)| *k != "repos --json");
    answers.push(("repos --json", Ok(repos_json_with_dots())));
    let recorded = install_run(&mut app, &answers);
    app.new_studio_open(None);
    drain_ns(&mut app).await;

    // The dialog asks for `--all` and filters locally.
    assert!(
        recorded
            .lock()
            .unwrap()
            .iter()
            .any(|(_, argv, _)| argv.iter().any(|a| a == "--all")),
        "the listing requested dot-directories"
    );
    {
        let d = app.new_studio.as_ref().unwrap();
        let names: Vec<&str> = d.repos.visible().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["acme-shop", "mono", "notes"],
            "dotfiles hidden, git first, workspace above a plain pick"
        );
    }
    // A `.` query is the one way to see them.
    key(&mut app, hc_vt::Key::Char('.'));
    key(&mut app, hc_vt::Key::Char('c'));
    {
        let d = app.new_studio.as_ref().unwrap();
        let names: Vec<&str> = d.repos.visible().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, [".cache", ".config"], "a dot query reveals them");
    }
    // Clearing the query hides them again.
    key(&mut app, hc_vt::Key::Backspace);
    key(&mut app, hc_vt::Key::Backspace);
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.repos.visible().len(), 3, "the dotfiles stay hidden");
}

#[tokio::test]
async fn the_vim_keys_are_filter_text_arrows_navigate() {
    let (mut app, _) = dialog_app().await;
    install_run(&mut app, &answers_with_dirs());
    // `l` and `h` used to descend/ascend — now they type.
    key(&mut app, hc_vt::Key::Char('l'));
    key(&mut app, hc_vt::Key::Char('h'));
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.repos.filter.as_str(), "lh");
    assert_eq!(
        d.repos.dir.as_deref(),
        Some("/home/user"),
        "nothing navigated"
    );
    // Arrows still do: clear, sit on `notes`, `→` descends.
    key(&mut app, hc_vt::Key::Backspace);
    key(&mut app, hc_vt::Key::Backspace);
    app.new_studio.as_mut().unwrap().repos.sel = 4;
    key(&mut app, hc_vt::Key::ArrowRight);
    drain_ns(&mut app).await;
    assert_eq!(
        app.new_studio.as_ref().unwrap().repos.dir.as_deref(),
        Some("/home/user/notes")
    );
}

#[tokio::test]
async fn esc_clears_the_filter_then_cancels_the_step() {
    let (mut app, _) = dialog_app().await;
    key(&mut app, hc_vt::Key::Char('t'));
    assert_eq!(app.new_studio.as_ref().unwrap().repos.visible().len(), 3);
    key(&mut app, hc_vt::Key::Escape);
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.repos.filter.as_str(), "", "the first Esc clears");
    assert_eq!(d.step, Step::MachineRepo, "it never left step 1");
    assert_eq!(d.repos.visible().len(), 5, "the list is whole again");
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.new_studio.is_none(), "the second Esc cancels");
}

#[tokio::test]
async fn the_filter_echoes_live_and_the_hint_names_it() {
    let (mut app, _) = dialog_app().await;
    // Untouched the heading's tail carries the hint.
    let backend = draw_to(&mut app, 120, 36);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("type to filter"), "the hint:\n{text}");
    for c in ['a', 'c'] {
        key(&mut app, hc_vt::Key::Char(c));
    }
    let backend = draw_to(&mut app, 120, 36);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("/ac"), "the live filter echoes:\n{text}");
    assert!(app.ns_hits.cursor.is_some(), "the filter owns the cursor");
}

#[tokio::test]
async fn committed_text_and_paste_land_in_the_filter_not_the_terminal() {
    let (mut app, _) = dialog_app().await;
    app.host_reports_all_keys = true;
    app.host_reports_key_releases = true;
    let before = app.pane.input_backlog().0;
    // A plain-text press under report-all (an IME/dead-key commit)
    // is the filter's, never the terminal's behind the dialog.
    let mut commit = term_key(
        hc_vt::Key::Char('a'),
        hc_vt::Mods::NONE,
        hc_vt::Action::Press,
    )
    .with_text_commit();
    commit.vt_bytes = b"a".to_vec();
    app.on_input(UiEvent::Key(commit), app.area.0);
    // IME text and a paste land the same way.
    app.on_input(UiEvent::Text("c".into()), app.area.0);
    app.on_input(UiEvent::Paste(b"me".to_vec()), app.area.0);
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.repos.filter.as_str(), "acme");
    assert_eq!(d.repos.visible().len(), 1);
    assert_eq!(
        app.pane.input_backlog().0,
        before,
        "nothing reached the pane"
    );
    // Steps without a text field still eat it, still not the pane's.
    app.new_studio.as_mut().unwrap().step = Step::Cast;
    app.on_input(UiEvent::Text("z".into()), app.area.0);
    assert_eq!(
        app.new_studio.as_ref().unwrap().repos.filter.as_str(),
        "acme"
    );
    assert_eq!(app.pane.input_backlog().0, before);
}

#[tokio::test]
async fn descending_into_a_dir_clears_the_filter() {
    let (mut app, _) = dialog_app().await;
    install_run(&mut app, &answers_with_dirs());
    // `not` narrows to the `notes` dir — `→` descends.
    for c in ['n', 'o', 't'] {
        key(&mut app, hc_vt::Key::Char(c));
    }
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.repos.filter.as_str(), "not");
    assert_eq!(d.repo().map(|r| r.name.as_str()), Some("notes"));
    key(&mut app, hc_vt::Key::ArrowRight);
    drain_ns(&mut app).await;
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.repos.dir.as_deref(), Some("/home/user/notes"));
    assert_eq!(d.repos.filter.as_str(), "", "the descend dropped it");
    let names: Vec<&str> = d.repos.visible().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["drafts", "notepad", "raw"], "nothing hidden");
    // A click while filtered navigates the same way.
    for c in ['d', 'r'] {
        key(&mut app, hc_vt::Key::Char(c));
    }
    draw_to(&mut app, 120, 36);
    let hit = app
        .ns_hits
        .cells
        .iter()
        .find_map(|(rect, hit)| match hit {
            crate::new_studio::NsHit::Repo(_) => Some((rect.x + 1, rect.y)),
            _ => None,
        })
        .expect("the filtered row is clickable");
    app.on_mouse(MouseInput {
        col: hit.0,
        row: hit.1,
        kind: MouseEventKind::Down(MouseButton::Left),
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    });
    drain_ns(&mut app).await;
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.repos.dir.as_deref(), Some("/home/user/notes/drafts"));
    assert_eq!(d.repos.filter.as_str(), "");
    assert_eq!(d.repos.visible().len(), 2);
}

#[tokio::test]
async fn climbing_to_the_parent_clears_the_filter_too() {
    let (mut app, _) = dialog_app().await;
    install_run(&mut app, &answers_with_dirs());
    // Descend into `notes` first so `dir` sits under `home`.
    app.new_studio.as_mut().unwrap().repos.sel = 4;
    key(&mut app, hc_vt::Key::ArrowRight);
    drain_ns(&mut app).await;
    assert_eq!(
        app.new_studio.as_ref().unwrap().repos.dir.as_deref(),
        Some("/home/user/notes")
    );
    // A filter hiding everything is the stale-filter worst case.
    key(&mut app, hc_vt::Key::Char('x'));
    assert!(app.new_studio.as_ref().unwrap().repos.visible().is_empty());
    key(&mut app, hc_vt::Key::ArrowLeft);
    drain_ns(&mut app).await;
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.repos.dir.as_deref(), Some("/home/user"));
    assert_eq!(d.repos.filter.as_str(), "", "the ascend dropped it");
    assert_eq!(d.repos.visible().len(), 5, "the home listing is whole");
}

#[tokio::test]
async fn a_machine_switch_clears_the_filter_too() {
    let (mut app, _) = dialog_app().await;
    for c in ['n', 'o'] {
        key(&mut app, hc_vt::Key::Char(c));
    }
    draw_to(&mut app, 120, 36);
    let hit = app
        .ns_hits
        .cells
        .iter()
        .find_map(|(rect, hit)| match hit {
            crate::new_studio::NsHit::Machine(1) => Some((rect.x + 1, rect.y)),
            _ => None,
        })
        .expect("a host-b machine hit");
    app.on_mouse(MouseInput {
        col: hit.0,
        row: hit.1,
        kind: MouseEventKind::Down(MouseButton::Left),
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    });
    drain_ns(&mut app).await;
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(d.machine_sel, 1, "host-b selected");
    assert_eq!(d.repos.filter.as_str(), "");
    assert_eq!(d.repos.visible().len(), 5, "its listing is unfiltered");
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
    // Cycle the runner cell: `claude` answers `missing` — only
    // seatable runners cycle in, so the seat stays on `fake`.
    key(&mut app, hc_vt::Key::ArrowRight);
    {
        let d = app.new_studio.as_ref().unwrap();
        assert_eq!(d.cast.rows[0].runner, 0, "a missing runner is skipped");
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

/// A `needs_path` runner — `cmd` living only under an nvm dir —
/// is a `found` seat like any other: the auto-seat lands on the
/// first found runner, and `→` cycling skips `claude` (`missing`)
/// to reach `cmd`, then wraps back.
#[tokio::test]
async fn a_needs_path_runner_cycles_in() {
    let mut app = app_multi(vec![term("t1")], vec![machine("host-b", "connected")]);
    app.set_plain(false);
    app.focused = Some("t1".into());
    install_run(&mut app, &answers_with_needs_path_runner());
    app.new_studio_open(None);
    drain_ns(&mut app).await;
    to_cast(&mut app).await;
    {
        let d = app.new_studio.as_ref().unwrap();
        assert_eq!(d.cast.rows[0].runner, 0, "the first found runner seats");
    }
    // fake → (claude: missing, skipped) → cmd.
    key(&mut app, hc_vt::Key::ArrowRight);
    {
        let d = app.new_studio.as_ref().unwrap();
        assert_eq!(
            d.cast.rows[0].runner, 2,
            "cmd cycles in past the missing row"
        );
    }
    // cmd → wrap → fake.
    key(&mut app, hc_vt::Key::ArrowRight);
    {
        let d = app.new_studio.as_ref().unwrap();
        assert_eq!(d.cast.rows[0].runner, 0);
    }
}

/// The generated cast.toml the table currently stands for.
fn cast_toml(app: &App) -> String {
    let d = app.new_studio.as_ref().unwrap();
    let runners = match &d.runners {
        Probe::Done(r) => r.clone(),
        _ => Vec::new(),
    };
    d.cast.toml(&runners, d.mode)
}

#[tokio::test]
async fn space_on_the_model_cell_opens_its_runners_list() {
    let mut app = app_multi(vec![term("t1")], vec![machine("host-b", "connected")]);
    app.set_plain(false);
    app.focused = Some("t1".into());
    install_run(&mut app, &answers_with_needs_path_runner());
    app.new_studio_open(None);
    drain_ns(&mut app).await;
    to_cast(&mut app).await;
    // `space` on RUNNER takes the next seat — past the `missing`
    // claude row, `cmd` — and opens no list.
    key(&mut app, hc_vt::Key::Char(' '));
    {
        let d = app.new_studio.as_ref().unwrap();
        assert!(d.model_pick.is_none(), "no sub-picker on RUNNER");
        assert_eq!(d.cast.rows[0].runner, 2, "space cycled to cmd");
    }
    key(&mut app, hc_vt::Key::ArrowLeft); // back to fake
    key(&mut app, hc_vt::Key::Tab); // col → MODEL
    key(&mut app, hc_vt::Key::Char(' '));
    {
        let d = app.new_studio.as_ref().unwrap();
        let pick = d.model_pick.as_ref().expect("the list is open");
        let options: Vec<&str> = pick.options.iter().map(String::as_str).collect();
        assert_eq!(options, ["auto", "m"], "auto first, fake's catalog after");
        assert_eq!(pick.chosen(), Some("m"), "the current value seats");
        assert_eq!(d.step, Step::Cast, "still on the cast step");
    }
    // The list is the row's runner's: cycling to `cmd` — past the
    // `missing` claude row — offers its catalog instead.
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::BackTab); // col → RUNNER
    key(&mut app, hc_vt::Key::ArrowRight); // fake → cmd
    key(&mut app, hc_vt::Key::Tab); // col → MODEL
    key(&mut app, hc_vt::Key::Char(' '));
    {
        let d = app.new_studio.as_ref().unwrap();
        let pick = d.model_pick.as_ref().unwrap();
        let options: Vec<&str> = pick.options.iter().map(String::as_str).collect();
        assert_eq!(options, ["auto", "cmd-x"], "cmd's catalog");
        assert_eq!(pick.chosen(), Some("cmd-x"));
    }
    // `space` on the cell opens it too.
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Char(' '));
    assert!(app.new_studio.as_ref().unwrap().model_pick.is_some());
}

#[tokio::test]
async fn picking_a_model_seats_it_into_the_cast() {
    let (mut app, _) = dialog_app().await;
    install_run(&mut app, &answers_with_wide_runner());
    to_cast(&mut app).await;
    app.new_studio.as_mut().unwrap().cast.col = 1;
    key(&mut app, hc_vt::Key::Char(' '));
    // The current `w-00` seats at index 1 — two downs land `w-02`.
    key(&mut app, hc_vt::Key::ArrowDown);
    key(&mut app, hc_vt::Key::ArrowDown);
    key(&mut app, hc_vt::Key::Enter);
    {
        let d = app.new_studio.as_ref().unwrap();
        assert!(d.model_pick.is_none(), "the pick closed the list");
        assert_eq!(d.cast.rows[0].model, 2);
    }
    assert!(
        cast_toml(&app).contains("model = \"w-02\""),
        "the pick flows into the cast:\n{}",
        cast_toml(&app)
    );
}

#[tokio::test]
async fn esc_cancel_keeps_the_old_model() {
    let (mut app, _) = dialog_app().await;
    install_run(&mut app, &answers_with_wide_runner());
    to_cast(&mut app).await;
    app.new_studio.as_mut().unwrap().cast.col = 1;
    key(&mut app, hc_vt::Key::Char(' '));
    // Move away from the current value, then bail.
    for _ in 0..3 {
        key(&mut app, hc_vt::Key::ArrowDown);
    }
    key(&mut app, hc_vt::Key::Escape);
    {
        let d = app.new_studio.as_ref().unwrap();
        assert!(d.model_pick.is_none(), "esc dropped the list");
        assert_eq!(d.cast.rows[0].model, 0, "the old value stayed");
        assert_eq!(d.step, Step::Cast, "esc did not leave the step");
    }
    assert!(
        cast_toml(&app).contains("model = \"w-00\""),
        "the cast still names the old model:\n{}",
        cast_toml(&app)
    );
}

#[tokio::test]
async fn auto_is_an_explicit_first_entry() {
    let (mut app, _) = dialog_app().await;
    to_cast(&mut app).await;
    app.new_studio.as_mut().unwrap().cast.col = 1;
    key(&mut app, hc_vt::Key::Char(' '));
    // `auto` sits first — the runner's own default, meaning it is
    // not a catalog position.
    {
        let d = app.new_studio.as_ref().unwrap();
        let pick = d.model_pick.as_ref().unwrap();
        assert_eq!(pick.options[0], "auto");
    }
    key(&mut app, hc_vt::Key::ArrowUp); // w-… current → auto
    key(&mut app, hc_vt::Key::Enter);
    assert!(
        cast_toml(&app).contains("model = \"auto\""),
        "auto lands in the cast like a typed model:\n{}",
        cast_toml(&app)
    );
    // And the catalog carries it now — the list does not offer it twice.
    {
        let d = app.new_studio.as_ref().unwrap();
        let runners = match &d.runners {
            Probe::Done(r) => r,
            _ => panic!("runners answered"),
        };
        assert_eq!(
            runners[0]
                .models
                .iter()
                .filter(|m| m.as_str() == "auto")
                .count(),
            1,
            "auto joined the catalog exactly once"
        );
    }
}

/// A catalog that already names `auto` — cursor's shape — opens
/// with it still first and still once.
#[tokio::test]
async fn a_catalog_auto_is_not_doubled_in_the_list() {
    let (mut app, _) = dialog_app().await;
    install_run(&mut app, &answers_with_auto_runner());
    to_cast(&mut app).await;
    app.new_studio.as_mut().unwrap().cast.col = 1;
    key(&mut app, hc_vt::Key::Char(' '));
    let d = app.new_studio.as_ref().unwrap();
    let pick = d.model_pick.as_ref().expect("the list is open");
    assert_eq!(
        pick.options,
        vec!["auto".to_string(), "x-1".to_string(), "x-2".to_string()],
        "auto stays first and appears once"
    );
}

/// A runner with no effort flag shows the depth its model id
/// carries in the EFFORT column — the flag-less runners'
/// `--effort` replacement, not a fake list.
#[tokio::test]
async fn a_flagless_runner_shows_the_depth_in_its_model_id() {
    let (mut app, _) = dialog_app().await;
    install_run(&mut app, &answers_with_depth_runner());
    to_cast(&mut app).await;
    let backend = draw_to(&mut app, 120, 36);
    let text = screen_lines(&backend).join("\n");
    assert!(
        text.contains("high"),
        "the model's depth shows where the effort flag would:\n{text}"
    );
    // Switching the model to a depth-less id falls back to `-`.
    {
        let d = app.new_studio.as_mut().unwrap();
        d.cast.rows[0].model = 1;
    }
    let backend = draw_to(&mut app, 120, 36);
    let text = screen_lines(&backend).join("\n");
    let row = text.lines().find(|l| l.contains("producer")).unwrap();
    assert!(row.contains(" -"), "no depth to show:\n{row}");
}

#[tokio::test]
async fn typing_filters_the_model_list() {
    let (mut app, _) = dialog_app().await;
    install_run(&mut app, &answers_with_wide_runner());
    to_cast(&mut app).await;
    app.new_studio.as_mut().unwrap().cast.col = 1;
    key(&mut app, hc_vt::Key::Char(' '));
    // Implicit like the repo picker — `w`,`-`,`1` narrows to w-1*.
    for c in ['w', '-', '1'] {
        key(&mut app, hc_vt::Key::Char(c));
    }
    {
        let d = app.new_studio.as_ref().unwrap();
        let pick = d.model_pick.as_ref().unwrap();
        assert_eq!(
            pick.visible(),
            ["w-10", "w-11", "w-12", "w-13", "w-14"],
            "the list narrows live"
        );
        assert_eq!(pick.sel, 0, "an edit re-seats the selection");
    }
    // Enter picks the filtered row — `w-11`, one down from `w-10`.
    key(&mut app, hc_vt::Key::ArrowDown);
    key(&mut app, hc_vt::Key::Enter);
    {
        let d = app.new_studio.as_ref().unwrap();
        assert_eq!(d.cast.rows[0].model, 11);
    }
    assert!(cast_toml(&app).contains("model = \"w-11\""));
}

#[tokio::test]
async fn the_model_list_scrolls_through_a_big_catalog() {
    let (mut app, _) = dialog_app().await;
    install_run(&mut app, &answers_with_wide_runner());
    to_cast(&mut app).await;
    app.new_studio.as_mut().unwrap().cast.col = 1;
    key(&mut app, hc_vt::Key::Char(' '));
    // 15 models + auto — the bottom row needs the window to follow.
    for _ in 0..14 {
        key(&mut app, hc_vt::Key::ArrowDown);
    }
    let backend = draw_to(&mut app, 80, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("w-14"), "the tail scrolled in:\n{text}");
    assert!(!text.contains("w-00"), "the top scrolled out:\n{text}");
}

#[tokio::test]
async fn a_click_on_the_list_picks_and_committed_text_filters() {
    let (mut app, _) = dialog_app().await;
    install_run(&mut app, &answers_with_wide_runner());
    to_cast(&mut app).await;
    app.new_studio.as_mut().unwrap().cast.col = 1;
    key(&mut app, hc_vt::Key::Char(' '));
    // IME/committed text narrows the list like the repo picker's.
    app.on_input(UiEvent::Text("w-1".into()), app.area.0);
    {
        let d = app.new_studio.as_ref().unwrap();
        let pick = d.model_pick.as_ref().unwrap();
        assert_eq!(pick.filter.as_str(), "w-1");
        assert_eq!(pick.visible().len(), 5);
    }
    // A click on a visible row picks it outright.
    draw_to(&mut app, 120, 36);
    let hit = app
        .ns_hits
        .cells
        .iter()
        .find_map(|(rect, hit)| match hit {
            crate::new_studio::NsHit::ModelPick(i) => Some((rect.x + 1, rect.y, *i)),
            _ => None,
        })
        .expect("a model row is clickable");
    assert_eq!(hit.2, 0, "first filtered row is w-10");
    app.on_mouse(MouseInput {
        col: hit.0,
        row: hit.1,
        kind: MouseEventKind::Down(MouseButton::Left),
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    });
    {
        let d = app.new_studio.as_ref().unwrap();
        assert!(d.model_pick.is_none(), "the click committed");
        assert_eq!(d.cast.rows[0].model, 10);
    }
    assert!(cast_toml(&app).contains("model = \"w-10\""));
}

#[tokio::test]
async fn enter_elsewhere_still_dry_runs() {
    let (mut app, _) = dialog_app().await;
    to_cast(&mut app).await;
    // `⏎` on the RUNNER cell is the dry run — now on MODEL too.
    assert_eq!(app.new_studio.as_ref().unwrap().cast.col, 0);
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    assert_eq!(app.new_studio.as_ref().unwrap().step, Step::CheckStart);
}

/// The Step-3 loop the audit found: `space` opens the MODEL picker,
/// `⏎` seats the pick and closes it, and the next `⏎` on the same
/// MODEL cell advances to the Step-4 dry run instead of reopening
/// the list.
#[tokio::test]
async fn space_picks_a_model_and_the_next_enter_advances() {
    let (mut app, recorded) = dialog_app().await;
    to_cast(&mut app).await;
    app.new_studio.as_mut().unwrap().cast.col = 1;
    recorded.lock().unwrap().clear();

    key(&mut app, hc_vt::Key::Char(' '));
    assert!(
        app.new_studio.as_ref().unwrap().model_pick.is_some(),
        "space opens the model list"
    );
    // The closed `fake` catalog is `auto, m`; take `auto`.
    key(&mut app, hc_vt::Key::ArrowUp);
    key(&mut app, hc_vt::Key::Enter);
    {
        let d = app.new_studio.as_ref().unwrap();
        assert!(d.model_pick.is_none(), "⏎ seats the pick");
        assert_eq!(d.step, Step::Cast, "and stays on the cast step");
        assert_eq!(d.cast.col, 1, "the MODEL cell keeps focus");
        assert_eq!(d.cast.rows[0].model, 1, "the picked row seated");
    }
    assert!(cast_toml(&app).contains("model = \"auto\""));
    // The second `⏎` never reopens the list — it dry-runs.
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    {
        let d = app.new_studio.as_ref().unwrap();
        assert!(d.model_pick.is_none(), "the list did not reopen");
        assert_eq!(d.step, Step::CheckStart, "⏎ advanced to the dry run");
    }
    assert!(
        recorded
            .lock()
            .unwrap()
            .iter()
            .any(|(_, argv, _)| argv.last().is_some_and(|a| a == "--dry-run")),
        "the dry run actually ran"
    );
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

/// Step 3's detection banner names each runner once: a version that
/// already leads with the runner's name loses that token.
#[tokio::test]
async fn the_detection_banner_never_stutters_runner_names() {
    let mut app = app_multi(vec![term("t1")], vec![]);
    app.set_plain(false);
    let mut answers = base_answers();
    answers.retain(|(k, _)| *k != "runners --json");
    answers.push(("runners --json", Ok(runners_json_stuttering_versions())));
    install_run(&mut app, &answers);
    app.new_studio_open(None);
    drain_ns(&mut app).await;
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::Enter);
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(
        text.contains("claude 2.1.0, codex 0.9.0, fake 1.0"),
        "the banner formats each runner once:\n{text}"
    );
    for stutter in [
        "claude claude",
        "codex codex",
        "fake fake",
        "claude claude 2.1.0",
    ] {
        assert!(!text.contains(stutter), "stutter '{stutter}':\n{text}");
    }
}

/// The footer teaches the one key map: `space` picks on MODEL and
/// takes the next value on RUNNER/EFFORT, `←→` changes the cell, and
/// `⏎` always runs the Step-4 dry run.
#[tokio::test]
async fn the_cast_footer_teaches_space_and_enter_per_cell() {
    let (mut app, _) = dialog_app().await;
    to_cast(&mut app).await;

    // RUNNER cell: `space next`, `←→ change`, `⏎ dry-run`.
    let text = footer(&draw_to(&mut app, 120, 36));
    assert!(text.contains("tab col"), "the footer teaches tab: {text}");
    assert!(
        text.contains("space next"),
        "on RUNNER space cycles: {text}"
    );
    assert!(text.contains("←→ change"), "the arrows change: {text}");
    assert!(text.contains("⏎ dry-run"), "on RUNNER ⏎ is dry-run: {text}");

    // MODEL cell: the picker is `space`'s, but `⏎` still dry-runs.
    key(&mut app, hc_vt::Key::Tab);
    let text = footer(&draw_to(&mut app, 120, 36));
    assert!(text.contains("space pick"), "on MODEL space picks: {text}");
    assert!(
        text.contains("⏎ dry-run"),
        "the model cell never claims ⏎ opens the list: {text}"
    );

    // EFFORT cell: back to `space next`, still dry-run.
    key(&mut app, hc_vt::Key::Tab);
    let text = footer(&draw_to(&mut app, 120, 36));
    assert!(
        text.contains("space next"),
        "on EFFORT space cycles: {text}"
    );
    assert!(text.contains("⏎ dry-run"), "on EFFORT ⏎ is dry-run: {text}");
}

/// A candidate row is tagged `actor-a*`, never truncated to `(c…`,
/// and the runner cell drops the `‹ ›` marks so a real name fits.
#[tokio::test]
async fn long_values_stay_readable_at_typical_widths() {
    let mut app = app_multi(vec![term("t1")], vec![]);
    app.set_plain(false);
    install_run(&mut app, &answers_with_long_names());
    app.new_studio_open(None);
    drain_ns(&mut app).await;
    to_cast(&mut app).await;

    for w in [60u16, 80, 120] {
        let text = screen_lines(&draw_to(&mut app, w, 36)).join("\n");
        assert!(
            text.contains("claude"),
            "the runner name fits at {w}:\n{text}"
        );
        assert!(
            text.contains("actor-a*"),
            "the candidate tag is `actor-a*` at {w}:\n{text}"
        );
        // The *values* never clip — the path on the detail line may,
        // it is a diagnostic tail, not a cast value.
        for cell in text.lines().filter(|l| l.contains("claude-sonnet")) {
            assert!(
                cell.contains("claude-sonnet-5-5"),
                "the model value clips at {w}: {cell}"
            );
        }
        assert!(
            !text.contains("(c…"),
            "the candidate tag is not `(c…` at {w}:\n{text}"
        );
        assert!(
            !text.contains('‹'),
            "the selected cell carries no ‹ › marks at {w}:\n{text}"
        );
    }
}

/// A focused-row detail line under the table names the whole value
/// the columns may clip, plus the runner's resolved binary path.
#[tokio::test]
async fn the_focused_row_has_a_full_detail_line() {
    let mut app = app_multi(vec![term("t1")], vec![]);
    app.set_plain(false);
    install_run(&mut app, &answers_with_long_names());
    app.new_studio_open(None);
    drain_ns(&mut app).await;
    to_cast(&mut app).await;

    let text = screen_lines(&draw_to(&mut app, 80, 36)).join("\n");
    assert!(
        text.contains("claude-sonnet-5-5"),
        "the whole model name is on the detail line:\n{text}"
    );
    assert!(
        text.contains("/home/user/.local/bin/claude"),
        "the resolved runner path is on the detail line:\n{text}"
    );
}

/// An app on a core that holds one studio, the dialog still closed —
/// `new_workspace` opens the wizard rather than the opening page.
fn app_under_studio() -> App {
    let mut app = app_multi(vec![term("t1")], vec![]);
    app.set_plain(false);
    app.focused = Some("t1".into());
    publish_summary(
        &mut app,
        None,
        serde_json::json!({
            "schema": "backlot/1", "updated_ms": 2,
            "studios": [{
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [{ "role": "producer", "name": "producer",
                            "terminal": "t1", "state": "live" }]
            }]
        }),
    );
    app
}

/// The chord that opens the dialog is swallowed: `prefix+shift+n`
/// must not type its tail `N` into step 1's repo filter, where it
/// would read `/N…` and match nothing.
#[tokio::test]
async fn the_opening_chord_never_types_into_the_repo_filter() {
    // A studio is on the core, the dialog still closed: `new_workspace`
    // opens the wizard over it.
    let mut app = app_under_studio();
    let _ = install_run(&mut app, &base_answers());
    assert!(app.new_studio.is_none(), "the dialog starts closed");
    // `ctrl+b` arms the prefix; `shift+n` is `new_workspace`, which
    // opens the wizard over a studio.
    key_mods(&mut app, hc_vt::Key::Char('b'), hc_vt::Mods::CTRL);
    key_mods(&mut app, hc_vt::Key::Char('N'), hc_vt::Mods::SHIFT);
    drain_ns(&mut app).await;
    let d = app
        .new_studio
        .as_ref()
        .expect("the chord opened the dialog");
    assert_eq!(d.step, Step::MachineRepo);
    assert_eq!(
        d.repos.filter.as_str(),
        "",
        "the chord tail leaked into the filter: {:?}",
        d.repos.filter.as_str()
    );
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
    let dry = calls
        .iter()
        .find(|(_, argv, _)| argv.last().is_some_and(|a| a == "--dry-run"))
        .expect("a dry-run call");
    // A repository pick takes no shape flag.
    assert!(
        !dry.1.iter().any(|a| a == "--init" || a == "--no-git"),
        "dry argv: {:?}",
        dry.1
    );
}

/// A plain-directory pick plans `git init` — the dry-run argv and
/// the command the step shows must carry the flag it plans on
/// (`--init`, or `--no-git` once the toggle lands), or the stage's
/// own shape check refuses what the plan promised.
#[tokio::test]
async fn the_dry_run_carries_the_plain_dirs_flag() {
    let (mut app, recorded) = dialog_app().await;
    key(&mut app, hc_vt::Key::Tab);
    // `notes` — the plain directory — is the sorted list's last row.
    // Its row already promises the init the launch will run.
    app.new_studio.as_mut().unwrap().repos.sel = 4;
    let backend = draw_to(&mut app, 100, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("will git init"), "the pick row: {text}");
    key(&mut app, hc_vt::Key::Enter);
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    assert_eq!(app.new_studio.as_ref().unwrap().step, Step::CheckStart);

    let dry_argv = |recorded: &Recorded| {
        recorded
            .lock()
            .unwrap()
            .iter()
            .rfind(|(_, argv, _)| argv.last().is_some_and(|a| a == "--dry-run"))
            .map(|(_, argv, _)| argv.clone())
            .expect("a dry-run call")
    };
    let argv = dry_argv(&recorded);
    assert!(argv.iter().any(|a| a == "--init"), "dry argv: {argv:?}");
    assert!(!argv.iter().any(|a| a == "--no-git"), "dry argv: {argv:?}");

    // The plan row and the command the step displays both say so.
    let backend = draw_to(&mut app, 100, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("git init"), "the plan: {text}");
    assert!(text.contains("--init"), "the shown command: {text}");

    // The no-git toggle re-runs it — argv and display follow.
    key(&mut app, hc_vt::Key::Char('g'));
    drain_ns(&mut app).await;
    let argv = dry_argv(&recorded);
    assert!(argv.iter().any(|a| a == "--no-git"), "dry argv: {argv:?}");
    assert!(!argv.iter().any(|a| a == "--init"), "dry argv: {argv:?}");
    let backend = draw_to(&mut app, 100, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("--no-git"), "the shown command: {text}");
}

/// The cast a successful `studio new` used is remembered per
/// machine: the next dialog on that machine seeds its rows from it
/// — roles, added candidates, a `^E` file too — while another
/// machine still gets the draft.
#[tokio::test]
async fn the_last_created_cast_prefills_that_machines_next_dialog() {
    let (mut app, _) = dialog_app().await;
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::Enter); // acme-shop → step 2
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await; // → Cast, the draft seeded
    // Producer's effort to `high`, an extra candidate, a custom file.
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::Tab); // EFFORT column
    key(&mut app, hc_vt::Key::ArrowRight);
    key(&mut app, hc_vt::Key::Char('a')); // actor-c
    let custom_text = {
        let d = app.new_studio.as_ref().unwrap();
        let runners = match &d.runners {
            Probe::Done(rows) => rows.clone(),
            _ => Vec::new(),
        };
        d.cast.toml(&runners, d.mode)
    };
    let file = std::env::temp_dir().join(format!("ns-cast-{}", std::process::id()));
    std::fs::write(&file, &custom_text).unwrap();
    app.ns_editor = Some(file);
    app.ns_editor_check();
    assert!(app.new_studio.as_ref().unwrap().cast.custom.is_some());
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await; // → CheckStart, the dry run saves nothing
    assert!(app.ns_saved_casts.is_empty(), "a dry run does not save");

    key(&mut app, hc_vt::Key::Enter); // create
    drain_ns(&mut app).await;
    let saved = &app.ns_saved_casts[0];
    assert_eq!(saved.machine, None, "the Local machine's cast");
    assert_eq!(saved.rows.len(), 6, "3 roles + 3 candidates");
    assert_eq!(saved.rows[0].effort.as_deref(), Some("high"));
    assert_eq!(saved.rows[5].key, "actor-c");
    assert!(saved.rows[5].candidate);
    assert_eq!(saved.custom.as_deref(), Some(custom_text.as_str()));

    // The same machine's next dialog seeds from the saved cast.
    app.new_studio_open(None);
    drain_ns(&mut app).await;
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::Enter);
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    let cast = &app.new_studio.as_ref().unwrap().cast;
    assert_eq!(cast.rows.len(), 6, "the saved rows came back");
    assert_eq!(cast.rows[0].effort, 1, "high is index 1 in fake's efforts");
    assert_eq!(cast.rows[5].key, "actor-c");
    assert!(cast.rows[5].candidate);
    assert_eq!(
        cast.custom.as_deref(),
        Some(custom_text.as_str()),
        "the hand-edited file came back too"
    );
    app.new_studio = None;

    // Another machine has no saved cast — the draft stands. A
    // preselected machine opens on the Repos pane directly.
    app.new_studio_open(Some("host-b".into()));
    drain_ns(&mut app).await;
    key(&mut app, hc_vt::Key::Enter); // repo → step 2
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await; // → Cast
    let cast = &app.new_studio.as_ref().unwrap().cast;
    assert_eq!(cast.rows.len(), 5, "the draft's own rows");
    assert_eq!(cast.rows[0].effort, 0);
    assert!(cast.custom.is_none());
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

// ---------- what a create leaves behind ----------

/// A created studio replaces its wizard: the dialog closes, the
/// draft is gone and the focus is owed to the new studio's producer —
/// even when the `stage start` that followed failed. The failure is
/// then a notice, not a reopened dialog the user must Esc out of.
#[tokio::test]
async fn a_created_studio_closes_the_wizard_even_if_the_stage_failed() {
    let mut answers = base_answers();
    answers.retain(|(k, _)| *k != "stage start");
    answers.push(("stage start", Err("stage start: refused".into())));
    let mut app = app_multi(vec![term("t1")], vec![machine("host-b", "connected")]);
    app.set_plain(false);
    app.focused = Some("t1".into());
    install_run(&mut app, &answers);
    app.new_studio_open(None);
    drain_ns(&mut app).await;
    to_step4(&mut app).await;

    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;

    assert!(
        app.new_studio.is_none(),
        "the created studio kept its wizard open"
    );
    assert_eq!(
        app.ns_pending_focus.as_ref().map(|key| key.id.as_str()),
        Some("acme-shop"),
        "the new studio is not owed the focus"
    );
    assert!(app.notice.is_some(), "the stage failure went unmentioned");
}

/// A failed `studio new` keeps everything: the wizard stays on its
/// review step with the error, the draft intact — the user is still
/// editing and must be able to fix and retry.
#[tokio::test]
async fn a_failed_create_keeps_the_wizard_and_its_draft() {
    // The prefix matcher takes the first hit, so the dry run's own
    // argv is answered first and the real create fails.
    let dry = "studio new --studio acme-shop --title acme shop \
               --repo /home/user/acme-shop --cast - --dry-run";
    let mut answers = base_answers();
    answers.retain(|(k, _)| *k != "studio new");
    answers.insert(0, (dry, Ok("dry run ok\n".into())));
    answers.push((
        "studio new",
        Err("studio 'acme-shop' already exists".into()),
    ));
    let mut app = app_multi(vec![term("t1")], vec![machine("host-b", "connected")]);
    app.set_plain(false);
    app.focused = Some("t1".into());
    install_run(&mut app, &answers);
    app.new_studio_open(None);
    drain_ns(&mut app).await;
    to_step4(&mut app).await;

    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;

    let d = app
        .new_studio
        .as_ref()
        .expect("a failed create closed the wizard");
    assert_eq!(d.step, Step::CheckStart, "the user is still on review");
    assert_eq!(
        d.create_error.as_deref(),
        Some("studio 'acme-shop' already exists")
    );
    assert_eq!(d.id.as_str(), "acme-shop", "the draft survived");
    assert!(!d.creating, "creating cleared");
    assert!(app.ns_pending_focus.is_none());
    assert!(app.notice.is_none(), "a failed create is the dialog's news");
}

/// A dry run never closes anything: it is a check, and the create
/// still has to be asked for.
#[tokio::test]
async fn a_dry_run_keeps_the_wizard_and_creates_nothing() {
    let (mut app, recorded) = dialog_app().await;
    to_step4(&mut app).await;
    let d = app.new_studio.as_ref().expect("the wizard is open");
    assert_eq!(d.step, Step::CheckStart);
    assert!(
        matches!(d.dry, Probe::Done(_)),
        "the dry run answered: {:?}",
        d.dry
    );
    assert!(
        !recorded.lock().unwrap().iter().any(|(_, argv, _)| {
            argv.starts_with(&["studio".into(), "new".into()])
                && !argv.iter().any(|a| a == "--dry-run")
        }),
        "the dry run issued a real create"
    );
    assert!(app.ns_pending_focus.is_none());
    assert!(app.notice.is_none());
    assert!(app.new_studio.is_some(), "a dry run closed the wizard");
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

// ---------- round 2: the selection covers text, `…` truncates, the
// strip collapses ----------

/// The bg of `needle`'s first cell inside the dialog rect
/// (`app.ns_hits.area` from the last draw).
fn cell_bg_in_dialog(
    b: &ratatui::backend::TestBackend,
    area: ratatui::layout::Rect,
    needle: &str,
) -> Option<ratatui::style::Color> {
    let buf = b.buffer();
    for y in area.y..area.bottom() {
        let line: String = (area.x..area.right())
            .map(|x| buf[(x, y)].symbol())
            .collect();
        if let Some(col) = line.find(needle) {
            return Some(buf[(area.x + col as u16, y)].bg);
        }
    }
    None
}

/// The bg of `needle`'s first cell, on the row holding `row_needle`.
fn cell_bg_on_row(
    b: &ratatui::backend::TestBackend,
    area: ratatui::layout::Rect,
    row_needle: &str,
    needle: &str,
) -> Option<ratatui::style::Color> {
    let buf = b.buffer();
    for y in area.y..area.bottom() {
        let line: String = (area.x..area.right())
            .map(|x| buf[(x, y)].symbol())
            .collect();
        if line.contains(row_needle) {
            let col = line.find(needle)?;
            return Some(buf[(area.x + col as u16, y)].bg);
        }
    }
    None
}

#[tokio::test]
async fn the_selected_row_highlights_its_text_cells() {
    let (mut app, _) = dialog_app().await;
    let sel = app.shell.palette.elevation().raised_on_overlay;
    // Machines pane — the `Local` row's own text carries the selection.
    let backend = draw_to(&mut app, 120, 36);
    let area = app.ns_hits.area;
    assert_eq!(cell_bg_in_dialog(&backend, area, "Local"), Some(sel));
    // Repos pane — the selected repo's name and its note too.
    key(&mut app, hc_vt::Key::Tab);
    let backend = draw_to(&mut app, 120, 36);
    let area = app.ns_hits.area;
    assert_eq!(cell_bg_in_dialog(&backend, area, "acme-shop"), Some(sel));
    assert_eq!(cell_bg_in_dialog(&backend, area, "git repo"), Some(sel));
    // Cast table — the selected producer row's role and value cells.
    key(&mut app, hc_vt::Key::Enter);
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    let backend = draw_to(&mut app, 120, 36);
    let area = app.ns_hits.area;
    assert_eq!(cell_bg_in_dialog(&backend, area, "producer"), Some(sel));
    assert_eq!(
        cell_bg_on_row(&backend, area, "producer", "fake"),
        Some(sel),
        "the RUNNER cell on the selected row"
    );
}

#[tokio::test]
async fn narrow_widths_truncate_with_ellipsis_and_collapse_the_strip() {
    let (mut app, _) = dialog_app().await;
    for w in [40u16, 60] {
        let backend = draw_to(&mut app, w, 24);
        let text = screen_lines(&backend).join("\n");
        assert!(
            text.contains("1/4 Machine"),
            "compact strip at {w}:\n{text}"
        );
        assert!(text.contains('…'), "ellipsis truncation at {w}:\n{text}");
    }
    let backend = draw_to(&mut app, 80, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(
        text.contains("3 Cast") && text.contains("4 Check & Start"),
        "the full strip at 80:\n{text}"
    );
}

// ---------- R3-M8: step 4 at 80×24 ----------

/// The final check at `80×24` keeps the checkbox label clear of the
/// `Create ⏎` button — a gap (or an ellipsis) before it, never a
/// word cut in half under it — and the button's click creates.
#[tokio::test]
async fn step4_at_80x24_keeps_the_checkbox_clear_of_create() {
    let (mut app, recorded) = dialog_app().await;
    to_step4(&mut app).await;
    let backend = draw_to(&mut app, 80, 24);
    let row = screen_lines(&backend)
        .into_iter()
        .find(|line| line.contains("Ensure stage"))
        .expect("the start-stage toggle row is on screen");
    let at = row.find(" Create ⏎").expect("the button rides the row");
    assert!(
        row[..at].ends_with(' '),
        "a gap separates the label from the button: {row:?}"
    );
    assert!(
        !row[..at].contains("and foc Create"),
        "the label is not cut mid-word under the button: {row:?}"
    );

    // The toggle's hit rect stops at the label; the button owns its
    // own cells, so a click there creates rather than toggling.
    let create = app
        .ns_hits
        .cells
        .iter()
        .find_map(|(rect, hit)| matches!(hit, crate::new_studio::NsHit::Create).then_some(*rect))
        .expect("the Create button is clickable");
    let toggle = app
        .ns_hits
        .cells
        .iter()
        .find_map(|(rect, hit)| {
            matches!(hit, crate::new_studio::NsHit::StartToggle).then_some(*rect)
        })
        .expect("the toggle row is clickable");
    assert!(
        toggle.right() <= create.x,
        "the toggle hit ends before the button: toggle {toggle:?}, create {create:?}"
    );

    let before = app.new_studio.as_ref().unwrap().start_stage;
    recorded.lock().unwrap().clear();
    app.on_mouse(MouseInput {
        col: create.x + 2,
        row: create.y,
        kind: MouseEventKind::Down(MouseButton::Left),
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    });
    drain_ns(&mut app).await;
    // A successful create closes the wizard; while it stays open the
    // checkbox must be exactly as the click found it.
    if let Some(d) = &app.new_studio {
        assert_eq!(d.start_stage, before, "the button never toggles the box");
    }
    assert!(
        recorded
            .lock()
            .unwrap()
            .iter()
            .any(|(_, argv, _)| argv.starts_with(&["studio".into(), "new".into()])),
        "the button click ran the create"
    );
}

// ---------- W6: the build brief on step 2 ----------

/// Walk to step 2 with the build field focused, typing `text` into it.
async fn type_build(app: &mut App, text: &str) {
    // Step 1: repo → step 2.
    if app.new_studio.as_ref().unwrap().pane == Step1Pane::Machines {
        key(app, hc_vt::Key::Tab);
    }
    key(app, hc_vt::Key::Enter);
    assert_eq!(app.new_studio.as_ref().unwrap().step, Step::IdTitle);
    // Fields cycle Id → Title → Mode → Build.
    key(app, hc_vt::Key::Tab);
    key(app, hc_vt::Key::Tab);
    key(app, hc_vt::Key::Tab);
    assert_eq!(app.new_studio.as_ref().unwrap().field, Step2Field::Build);
    for c in text.chars() {
        key(app, hc_vt::Key::Char(c));
    }
}

#[tokio::test]
async fn step2_draws_the_build_field() {
    let (mut app, _) = dialog_app().await;
    // Land on step 2.
    if app.new_studio.as_ref().unwrap().pane == Step1Pane::Machines {
        key(&mut app, hc_vt::Key::Tab);
    }
    key(&mut app, hc_vt::Key::Enter);
    let text = screen_lines(&draw_to(&mut app, 100, 36)).join("\n");
    assert!(
        text.contains("What should this studio build?"),
        "the build field is drawn:\n{text}"
    );
    assert!(text.contains("optional"), "it is optional:\n{text}");
}

#[tokio::test]
async fn typing_a_brief_reaches_the_submit_argv() {
    let (mut app, recorded) = dialog_app().await;
    type_build(&mut app, "build a cart").await;
    assert_eq!(
        app.new_studio.as_ref().unwrap().build_text(),
        "build a cart",
        "the field holds the typed brief"
    );
    // Step 2 → Cast → Check & Start (the dry run fires).
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    assert_eq!(app.new_studio.as_ref().unwrap().step, Step::Cast);
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    recorded.lock().unwrap().clear();
    key(&mut app, hc_vt::Key::Enter); // create
    drain_ns(&mut app).await;

    let calls = recorded.lock().unwrap();
    let create = calls
        .iter()
        .find(|(_, argv, _)| argv.starts_with(&["studio".into(), "new".into()]))
        .expect("the create call");
    let argv = &create.1;
    let at = argv
        .iter()
        .position(|a| a == "--ask-file")
        .expect("--ask-file in the argv");
    let path = &argv[at + 1];
    assert!(path.ends_with(".md"), "the ask temp file: {path}");
    // The temp file was written with the brief, then removed after.
    assert!(
        !std::path::Path::new(path).exists(),
        "the temp file was cleaned up"
    );
}

#[tokio::test]
async fn an_empty_brief_sends_nothing() {
    let (mut app, recorded) = dialog_app().await;
    to_step4(&mut app).await;
    recorded.lock().unwrap().clear();
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    let calls = recorded.lock().unwrap();
    let create = calls
        .iter()
        .find(|(_, argv, _)| argv.starts_with(&["studio".into(), "new".into()]))
        .expect("the create call");
    assert!(
        !create.1.iter().any(|a| a == "--ask-file"),
        "no brief, no --ask-file: {:?}",
        create.1
    );
}

#[tokio::test]
async fn the_summary_card_shows_the_briefs_first_line() {
    let (mut app, _) = dialog_app().await;
    type_build(&mut app, "build a cart").await;
    // Continue to the cast, then to the summary.
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    let text = screen_lines(&draw_to(&mut app, 100, 36)).join("\n");
    assert!(
        text.contains("First ask") && text.contains("build a cart"),
        "the summary shows the brief's first line:\n{text}"
    );
}

#[tokio::test]
async fn alt_enter_adds_a_line_to_the_brief() {
    let (mut app, _) = dialog_app().await;
    type_build(&mut app, "first").await;
    key_mods(&mut app, hc_vt::Key::Enter, hc_vt::Mods::ALT);
    for c in "second".chars() {
        key(&mut app, hc_vt::Key::Char(c));
    }
    let d = app.new_studio.as_ref().unwrap();
    assert_eq!(
        d.build.lines.len(),
        2,
        "alt+⏎ split the brief into two lines"
    );
    assert_eq!(d.build_text(), "first\nsecond");
    assert_eq!(d.step, Step::IdTitle, "alt+⏎ does not advance the step");
}

#[tokio::test]
async fn new_studio_dialog_creates_studio_with_design_gate_required() {
    struct TempGuard(std::path::PathBuf);
    impl Drop for TempGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let (mut app, recorded) = dialog_app().await;
    to_step4(&mut app).await;
    recorded.lock().unwrap().clear();
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;

    let calls = recorded.lock().unwrap();
    let create = calls
        .iter()
        .find(|(_, argv, _)| argv.starts_with(&["studio".into(), "new".into()]))
        .expect("the create call");
    let cast_toml = create.2.as_deref().expect("dialog passes cast on stdin");

    let root = std::env::temp_dir().join(format!("hc-tui-ns-design-gate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _guard = TempGuard(root.clone());
    let repo = root.join("acme-shop");
    std::fs::create_dir_all(&repo).unwrap();
    let cast_file = root.join("cast.toml");
    std::fs::write(&cast_file, cast_toml).unwrap();

    let env = backlot_stage::cli::Env {
        vars: std::collections::BTreeMap::from([(
            "BACKLOT_STATE_DIR".into(),
            root.to_string_lossy().into_owned(),
        )]),
    };
    let argv: Vec<String> = [
        "studio",
        "new",
        "--studio",
        "acme-shop",
        "--title",
        "Acme Shop",
        "--repo",
        &repo.to_string_lossy(),
        "--cast",
        &cast_file.to_string_lossy(),
        "--no-git",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    std::thread::spawn(move || backlot_stage::cli::run(&argv, &env))
        .join()
        .unwrap()
        .unwrap();

    let sdir = backlot_stage::store::studio_dir(&root, "acme-shop");
    let state = backlot_stage::store::load_state(&sdir).unwrap();
    assert_eq!(
        state.studio.unwrap().design_gate,
        backlot_model::design::DesignGate::Required
    );
}
