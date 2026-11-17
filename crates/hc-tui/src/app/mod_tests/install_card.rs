//! The in-client installer, driven through injected seams: the pipeline
//! is recorded, never run; output, exit and the version check are fed as
//! events. Process-group behaviour itself is covered in `update_run_tests`.
use super::*;
use crate::app::install_flow::Seams;
use crate::app::update_flow::UpdateEvent;
use crate::chrome::update_modal::Phase;
use crate::update_run::Plan;
use hc_vt::{Action, Key, Mods};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Record {
    plans: Vec<Plan>,
    signals: Vec<(i32, i32)>,
    probe: Option<String>,
}

fn seams(record: &Arc<Mutex<Record>>) -> Seams {
    let spawned = Arc::clone(record);
    let signalled = Arc::clone(record);
    let probed = Arc::clone(record);
    Seams {
        spawn: Arc::new(move |plan, _events, _generation| {
            spawned.lock().unwrap().plans.push(plan.clone());
            Ok(4242)
        }),
        signal: Arc::new(move |pgid, signal| {
            signalled.lock().unwrap().signals.push((pgid, signal))
        }),
        alive: Arc::new(|_| false),
        probe: Arc::new(move |_bin| {
            let version = probed.lock().unwrap().probe.clone();
            Box::pin(async move { version })
        }),
    }
}

/// A writable install dir and an offered 0.1.13 with the card opened.
fn offered(dir: &std::path::Path, record: &Arc<Mutex<Record>>) -> App {
    std::fs::create_dir_all(dir).unwrap();
    let mut app = app();
    app.update_latest = Some("0.1.13".into());
    app.running_exe = Some(dir.join("hc-tui"));
    app.seams = seams(record);
    app.open_update();
    app
}

fn press(app: &mut App, key: Key) {
    app.on_input(
        input::UiEvent::Key(term_key(key, Mods::NONE, Action::Press)),
        120,
    );
}

fn drain(app: &mut App) {
    while let Ok(event) = app.update_rx.try_recv() {
        app.on_update_event(event);
    }
}

fn phase(app: &App) -> Option<Phase> {
    app.update_modal.as_ref().map(|modal| modal.phase)
}

fn scratch(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("hctui-install-{label}-{}", std::process::id()))
}

#[tokio::test]
async fn a_verified_install_reaches_done_through_the_version_check() {
    let dir = scratch("done");
    let record = Arc::new(Mutex::new(Record {
        probe: Some("hc-tui 0.1.13".into()),
        ..Record::default()
    }));
    let mut app = offered(&dir, &record);
    press(&mut app, Key::Enter);
    assert_eq!(phase(&app), Some(Phase::Updating));
    let generation = app.install_generation;
    app.on_update_event(UpdateEvent::Output {
        generation,
        line: "installed hc-tui 0.1.13 (x86_64-unknown-linux-musl)".into(),
    });
    app.on_update_event(UpdateEvent::Exited {
        generation,
        code: Some(0),
    });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    drain(&mut app);
    assert_eq!(phase(&app), Some(Phase::Done));
    let install = app.install.as_ref().unwrap();
    assert_eq!(install.verified.as_deref(), Some("hc-tui 0.1.13"));
    assert_eq!(
        record.lock().unwrap().plans.len(),
        1,
        "one install, no re-run"
    );
}

#[tokio::test]
async fn a_wrong_version_reaches_failed_with_the_reason() {
    let dir = scratch("wrong");
    let record = Arc::new(Mutex::new(Record {
        probe: Some("hc-tui 0.1.11".into()),
        ..Record::default()
    }));
    let mut app = offered(&dir, &record);
    press(&mut app, Key::Enter);
    let generation = app.install_generation;
    app.on_update_event(UpdateEvent::Exited {
        generation,
        code: Some(0),
    });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    drain(&mut app);
    assert_eq!(phase(&app), Some(Phase::Failed));
    assert_eq!(
        app.install.as_ref().unwrap().reason.as_deref(),
        Some("binary check expected 0.1.13, got 0.1.11")
    );
}

#[tokio::test]
async fn a_nonzero_exit_reaches_failed_with_the_reason_and_the_tail() {
    let dir = scratch("exit");
    let record = Arc::new(Mutex::new(Record::default()));
    let mut app = offered(&dir, &record);
    press(&mut app, Key::Enter);
    let generation = app.install_generation;
    for line in [
        "curl: (6) Could not resolve host: github.com",
        "install.sh: download failed",
    ] {
        app.on_update_event(UpdateEvent::Output {
            generation,
            line: line.into(),
        });
    }
    app.on_update_event(UpdateEvent::Exited {
        generation,
        code: Some(1),
    });
    assert_eq!(phase(&app), Some(Phase::Failed));
    let install = app.install.as_ref().unwrap();
    assert_eq!(
        install.reason.as_deref(),
        Some("installer exited with status 1")
    );
    assert_eq!(install.tail.len(), 2, "the output tail is kept");
    assert!(install.tail[0].starts_with("curl:"));
}

#[tokio::test]
async fn cancel_signals_the_whole_group_and_returns_to_opened() {
    let dir = scratch("cancel");
    let record = Arc::new(Mutex::new(Record::default()));
    let mut app = offered(&dir, &record);
    press(&mut app, Key::Enter);
    press(&mut app, Key::Escape);
    assert_eq!(phase(&app), Some(Phase::Opened));
    assert!(app.install.is_none());
    assert_eq!(record.lock().unwrap().signals, [(4242, libc::SIGTERM)]);
}

#[tokio::test]
async fn retry_reruns_the_pipeline_after_a_failure() {
    let dir = scratch("retry");
    let record = Arc::new(Mutex::new(Record::default()));
    let mut app = offered(&dir, &record);
    press(&mut app, Key::Enter);
    let generation = app.install_generation;
    app.on_update_event(UpdateEvent::Exited {
        generation,
        code: Some(1),
    });
    assert_eq!(phase(&app), Some(Phase::Failed));
    press(&mut app, Key::Enter);
    assert_eq!(phase(&app), Some(Phase::Updating));
    assert_eq!(record.lock().unwrap().plans.len(), 2);
}

#[tokio::test]
async fn the_pipeline_sets_the_three_variables_before_sh_and_runs_locally() {
    let dir = scratch("env");
    let record = Arc::new(Mutex::new(Record::default()));
    let mut app = offered(&dir, &record);
    press(&mut app, Key::Enter);
    let record = record.lock().unwrap();
    let plan = &record.plans[0];
    let dir_text = dir.display().to_string();
    assert_eq!(
        plan.env,
        [
            ("HC_TUI_VERSION", "v0.1.13".to_owned()),
            ("HC_TUI_SKIP_CORE", "1".to_owned()),
            ("HC_TUI_INSTALL_DIR", dir_text.clone()),
        ]
    );
    assert!(
        plan.pipeline.ends_with(&format!(
            " HC_TUI_VERSION=v0.1.13 HC_TUI_SKIP_CORE=1 HC_TUI_INSTALL_DIR={dir_text} sh"
        )),
        "{}",
        plan.pipeline
    );
    assert!(
        plan.pipeline
            .starts_with("curl -fsSL https://followcat.github.io/")
    );
}

#[tokio::test]
async fn esc_later_keeps_the_finished_install_and_reopening_does_not_reinstall() {
    let dir = scratch("later");
    let record = Arc::new(Mutex::new(Record {
        probe: Some("hc-tui 0.1.13".into()),
        ..Record::default()
    }));
    let mut app = offered(&dir, &record);
    press(&mut app, Key::Enter);
    let generation = app.install_generation;
    app.on_update_event(UpdateEvent::Exited {
        generation,
        code: Some(0),
    });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    drain(&mut app);
    press(&mut app, Key::Escape);
    assert!(app.update_modal.is_none());
    assert_eq!(app.restart_ready.as_deref(), Some("0.1.13"));
    app.open_update();
    assert_eq!(
        phase(&app),
        Some(Phase::Done),
        "reopening shows the finished install"
    );
    assert_eq!(
        record.lock().unwrap().plans.len(),
        1,
        "never installed again"
    );
}

#[tokio::test]
async fn events_from_a_cancelled_run_never_change_the_card() {
    let dir = scratch("stale");
    let record = Arc::new(Mutex::new(Record::default()));
    let mut app = offered(&dir, &record);
    press(&mut app, Key::Enter);
    let first = app.install_generation;
    press(&mut app, Key::Escape);
    press(&mut app, Key::Enter);
    assert_eq!(app.install_generation, first + 1);
    app.on_update_event(UpdateEvent::Output {
        generation: first,
        line: "late line from the cancelled run".into(),
    });
    app.on_update_event(UpdateEvent::Exited {
        generation: first,
        code: Some(1),
    });
    assert_eq!(
        phase(&app),
        Some(Phase::Updating),
        "the live run is untouched"
    );
    assert!(app.install.as_ref().unwrap().tail.is_empty());
}

#[tokio::test]
async fn enter_on_a_verified_install_marks_the_version_seen_and_leaves_for_the_restart() {
    let dir = scratch("restart");
    let record = Arc::new(Mutex::new(Record {
        probe: Some("hc-tui 0.1.13".into()),
        ..Record::default()
    }));
    let mut app = offered(&dir, &record);
    press(&mut app, Key::Enter);
    let generation = app.install_generation;
    app.on_update_event(UpdateEvent::Exited {
        generation,
        code: Some(0),
    });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    drain(&mut app);
    assert_eq!(phase(&app), Some(Phase::Done));
    press(&mut app, Key::Enter);
    assert!(app.quitting, "the run loop unwinds the way a detach does");
    assert!(app.update_modal.is_none());
    assert_eq!(app.take_restart(), Some(dir.join("hc-tui")));
    assert!(
        app.take_restart().is_none(),
        "the restart is handed back once"
    );
    assert_eq!(
        app.update_state.show_on_startup_version.as_deref(),
        Some("0.1.13"),
        "marked seen before the exec"
    );
}

#[tokio::test]
async fn a_restarted_run_does_not_reopen_whats_new_but_an_upgrade_does() {
    let dir = scratch("whats-new");
    std::fs::create_dir_all(&dir).unwrap();
    let mut app = app();
    app.running_exe = Some(dir.join("hc-tui"));
    app.update_state.releases = vec![crate::update_state::CachedRelease {
        version: env!("CARGO_PKG_VERSION").into(),
        date: "2026-01-01".into(),
        body: "## Changes\n- **Lead-in** text (#39)".into(),
    }];
    app.update_state.show_on_startup_version = Some(env!("CARGO_PKG_VERSION").into());
    app.maybe_open_whats_new();
    assert!(
        app.update_modal.is_none(),
        "marked seen: no card on this start"
    );
    app.update_state.show_on_startup_version = Some("0.0.0".into());
    app.maybe_open_whats_new();
    assert_eq!(
        phase(&app),
        Some(Phase::WhatsNew),
        "an upgrade shows its notes once"
    );
}

#[tokio::test]
async fn enter_closes_the_whats_new_card_and_marks_it_seen() {
    let dir = scratch("whats-new-enter");
    std::fs::create_dir_all(&dir).unwrap();
    let mut app = app();
    app.running_exe = Some(dir.join("hc-tui"));
    app.update_state.releases = vec![crate::update_state::CachedRelease {
        version: env!("CARGO_PKG_VERSION").into(),
        date: "2026-01-01".into(),
        body: "## Changes\n- **Lead-in** text (#39)".into(),
    }];
    app.maybe_open_whats_new();
    assert_eq!(phase(&app), Some(Phase::WhatsNew));
    press(&mut app, Key::Enter);
    assert!(app.update_modal.is_none());
    assert_eq!(
        app.update_state.show_on_startup_version.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
}
