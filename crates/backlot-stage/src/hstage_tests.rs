//! The lifecycle decision's truth table (`hplan`): what the config,
//! the role record and a live `bl-hollywood` together owe — start,
//! adopt, rejoin, lost, resume, the crash guard's give-up, close and
//! drop — plus the actor identity and the terminal env the launch
//! hands the runner.

use backlot_model::event::ActorIdentity;
use backlot_model::hollywood::{HOLLYWOOD_TERMINAL, HWRole};

use crate::config::HollywoodCfg;
use crate::hlife::hrole_env;
use crate::hstage::{HMove, hplan, identity, resume_session};

fn cfg(runner: &str) -> HollywoodCfg {
    HollywoodCfg {
        runner: runner.into(),
        model: "model-h".into(),
        effort: Some("high".into()),
        key: None,
        default_cast: None,
    }
}

/// A role with a recorded actor — `recorded()` true without the
/// live/closed flags.
fn recorded(runner: &str) -> HWRole {
    HWRole {
        actor: Some(ActorIdentity {
            runner: runner.into(),
            runner_version: None,
            model: "model-h".into(),
            effort: Some("high".into()),
            permission_profile: "standard".into(),
        }),
        ..Default::default()
    }
}

fn live(runner: &str) -> HWRole {
    HWRole {
        live: true,
        terminal: Some("term-1".into()),
        ..recorded(runner)
    }
}

/// No config: a live record closes on purpose, a stray terminal is
/// dropped unrecorded, nothing else moves.
#[test]
fn without_config_the_layer_comes_down() {
    let bare = HWRole::default();
    assert_eq!(
        hplan(None, &live("fake"), true, 0, 0),
        vec![HMove::Close("not configured")]
    );
    assert_eq!(hplan(None, &bare, true, 0, 0), vec![HMove::Drop]);
    assert_eq!(hplan(None, &bare, false, 0, 0), Vec::<HMove>::new());
}

/// A restart the user recorded on a live record with a live terminal
/// closes it on purpose (`Restart`); the start that follows resumes
/// the kept session (`ReviveResume`) or, for a `fresh` request,
/// starts over (`Start`).
#[test]
fn a_live_restart_closes_then_starts() {
    use backlot_model::state::RestartRequest;
    let c = cfg("fake");
    let mut role = live("fake");
    role.restart = Some(RestartRequest {
        seq: 9,
        fresh: false,
        reason: None,
    });
    assert_eq!(hplan(Some(&c), &role, true, 0, 0), vec![HMove::Restart]);

    // The close the restart recorded, session kept: resume it.
    role.live = false;
    role.closed = true;
    role.session_id = Some("sess-h".into());
    assert_eq!(
        hplan(Some(&c), &role, false, 0, 0),
        vec![HMove::ReviveResume]
    );

    // A fresh request starts a new conversation.
    role.restart = Some(RestartRequest {
        seq: 9,
        fresh: true,
        reason: None,
    });
    assert_eq!(hplan(Some(&c), &role, false, 0, 0), vec![HMove::Start]);
}

/// A restart on a lost record comes back at once, around the loss
/// guard.
#[test]
fn a_lost_restart_ignores_the_loss_guard() {
    use backlot_model::state::RestartRequest;
    let c = cfg("fake");
    let mut role = recorded("fake");
    role.session_id = Some("sess-h".into());
    role.losses = vec![100_000, 150_000, 190_000];
    role.restart = Some(RestartRequest {
        seq: 9,
        fresh: false,
        reason: None,
    });
    assert_eq!(
        hplan(Some(&c), &role, false, 200_000, 0),
        vec![HMove::ResumeRun]
    );
}

/// A live record with a live terminal holds — unless the config's
/// runner moved, which is a new conversation: close and start.
#[test]
fn a_runner_change_is_a_new_conversation() {
    let c = cfg("fake");
    assert_eq!(
        hplan(Some(&c), &live("fake"), true, 0, 0),
        Vec::<HMove>::new()
    );
    assert_eq!(
        hplan(Some(&c), &live("claude"), true, 0, 0),
        vec![HMove::Close("runner changed"), HMove::Start]
    );
}

/// Live record, no terminal: `lost` this tick; the resume runs the
/// next — the producer's pattern, so the two states stay distinct.
#[test]
fn a_live_record_without_a_terminal_is_lost() {
    let c = cfg("fake");
    assert_eq!(
        hplan(Some(&c), &live("fake"), false, 0, 0),
        vec![HMove::Lost]
    );
}

/// A recorded `lost` keeps its session for the resume run; three
/// losses in ten minutes is the guard's give-up — old ones do not
/// count.
#[test]
fn the_resume_guard_counts_recent_losses() {
    let c = cfg("fake");
    let mut role = recorded("fake");
    role.terminal = Some("term-1".into());
    role.session_id = Some("fake-bl-hollywood".into());
    role.losses = vec![100_000];
    let now = 200_000;
    assert_eq!(
        hplan(Some(&c), &role, false, now, 0),
        vec![HMove::ResumeRun]
    );
    role.losses = vec![100_000, 150_000, 190_000];
    assert_eq!(hplan(Some(&c), &role, false, now, 0), vec![HMove::GiveUp]);
    // The same count spread past the ten-minute window resumes
    // instead — only `190_000` is recent once `now` reaches 700_000.
    role.losses = vec![1_000, 2_000, 190_000];
    assert_eq!(
        hplan(Some(&c), &role, false, 700_000, 0),
        vec![HMove::ResumeRun]
    );
    // The give-up's closed record stands: nothing starts it again.
    role.live = false;
    role.closed = true;
    role.closed_reason = Some("cannot resume".into());
    assert_eq!(hplan(Some(&c), &role, false, now, 0), Vec::<HMove>::new());
}

/// `cannot resume` is this stage process's give-up: the close stamps
/// `since_ms`, and a stage born after it revives the conversation —
/// a person restarting the stage is the one way back (R1-4). Within
/// the same process it stays down.
#[test]
fn a_gave_up_hollywood_revives_only_under_a_new_stage() {
    let c = cfg("fake");
    let mut gave_up = recorded("fake");
    gave_up.closed = true;
    gave_up.closed_reason = Some("cannot resume".into());
    gave_up.since_ms = 500;
    // The same stage process — born before the close — holds the
    // give-up; a process born after it revives.
    assert_eq!(
        hplan(Some(&c), &gave_up, false, 600, 400),
        Vec::<HMove>::new()
    );
    assert_eq!(
        hplan(Some(&c), &gave_up, false, 600, 600),
        vec![HMove::Revive]
    );
    // A stray terminal still drops either way.
    assert_eq!(hplan(Some(&c), &gave_up, true, 600, 600), vec![HMove::Drop]);
}

/// A live terminal the ledger never recorded is adopted — the
/// stage restart's case; a `lost` record's terminal rejoins with
/// `recovered`.
#[test]
fn a_live_terminal_is_adopted_or_rejoined() {
    let c = cfg("fake");
    let bare = HWRole::default();
    assert_eq!(hplan(Some(&c), &bare, true, 0, 0), vec![HMove::Adopt]);
    assert_eq!(
        hplan(Some(&c), &recorded("fake"), true, 0, 0),
        vec![HMove::Rejoin]
    );
}

/// Nothing recorded and nothing live: start. A deliberate `closed`
/// that is not the guard's revives on the config's return.
#[test]
fn starts_and_revives() {
    let c = cfg("fake");
    let bare = HWRole::default();
    assert_eq!(hplan(Some(&c), &bare, false, 0, 0), vec![HMove::Start]);
    let mut closed = recorded("fake");
    closed.closed = true;
    closed.closed_reason = Some("not configured".into());
    assert_eq!(hplan(Some(&c), &closed, false, 0, 0), vec![HMove::Revive]);
    // …but a stray terminal beside a closed record is still dropped.
    assert_eq!(hplan(Some(&c), &closed, true, 0, 0), vec![HMove::Drop]);
}

/// A resume may only carry a session recorded under the runner
/// being launched: the old runner's id belongs to a conversation
/// that runner minted, and handing it to another is `No session
/// named …`. With no recorded actor there is nothing to prove, so
/// no session goes out either.
#[test]
fn a_resume_never_carries_another_runners_session() {
    let mut role = recorded("devin");
    role.session_id = Some("beaded-harp".into());
    assert_eq!(resume_session(&role, &identity(&cfg("cmd"))), None);
    assert_eq!(
        resume_session(&role, &identity(&cfg("devin"))),
        Some("beaded-harp")
    );
    let bare = HWRole {
        session_id: Some("beaded-harp".into()),
        ..Default::default()
    };
    assert_eq!(resume_session(&bare, &identity(&cfg("devin"))), None);
}

/// The identity `[hollywood]` names — runner, model, effort from
/// the config, the one `standard` permission profile (D17a).
#[test]
fn the_identity_carries_the_standard_profile() {
    let a = identity(&cfg("fake"));
    assert_eq!(a.runner, "fake");
    assert_eq!(a.model, "model-h");
    assert_eq!(a.effort.as_deref(), Some("high"));
    assert_eq!(a.permission_profile, "standard");
}

/// The terminal env: `BACKLOT_ROLE=hollywood`, the state dir, no
/// `BACKLOT_STUDIO`, and a PATH that leads with this exe's dir —
/// the runner env's own PATH keeps the lead but stays behind it.
#[test]
fn the_terminal_env_marks_hollywood_alone() {
    let mut adapter = std::collections::BTreeMap::new();
    adapter.insert("FAKE_MODEL".into(), "model-h".into());
    let mut runner = std::collections::BTreeMap::new();
    runner.insert("PATH".into(), "/opt/bin".into());
    runner.insert("PROXY".into(), "http://proxy:1".into());
    let env = hrole_env(adapter, runner, "/state", "/exe", "/usr/bin");
    assert_eq!(env["BACKLOT_ROLE"], "hollywood");
    assert_eq!(env["BACKLOT_STATE_DIR"], "/state");
    assert!(!env.contains_key("BACKLOT_STUDIO"));
    assert_eq!(
        env["PATH"],
        "/state/hollywood/roles/hollywood/bin:/exe:/opt/bin"
    );
    assert_eq!(env["PROXY"], "http://proxy:1");
    assert_eq!(env["FAKE_MODEL"], "model-h");
    assert_eq!(env["GIT_CONFIG_COUNT"], "1");
    assert_eq!(env["GIT_CONFIG_KEY_0"], "core.hooksPath");
    assert_eq!(
        env["GIT_CONFIG_VALUE_0"],
        "/state/hollywood/roles/hollywood/git-hooks"
    );
    // No configured PATH: shim dir leads the stage's own PATH.
    let env = hrole_env(
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::new(),
        "/state",
        "/exe",
        "/usr/bin",
    );
    assert_eq!(env["PATH"], "/state/hollywood/roles/hollywood/bin:/usr/bin");
}

/// `devin` takes `--prompt-file <path>`, never inline prompt text:
/// fresh Hollywood launches write `<hdir>/prompts/hollywood.first.txt`
/// pointing at `AGENTS.md` and pass that file path to `devin`, while
/// resumes pass no `--prompt-file` and other runners keep inline text.
#[test]
fn devin_hollywood_launch_writes_and_passes_a_prompt_file() {
    use crate::hlife::hprepare_launch;
    use crate::hstage::hfirst_prompt;
    use crate::runners::{LaunchCtx, plan};
    use backlot_model::hollywood::{HOLLYWOOD, HOLLYWOOD_TERMINAL};

    let tmp = crate::test_support::TestDir::new("bl-hdevin");
    let hdir = tmp.path().join("hollywood");
    let cwd = hdir.to_string_lossy().into_owned();
    let stage_env = std::collections::BTreeMap::new();

    // Fresh devin launch: `--prompt-file` receives an existing file path.
    let prompt = hfirst_prompt(&hdir, "devin", None);
    let actor = identity(&cfg("devin"));
    let launch = plan(
        &actor,
        &LaunchCtx {
            terminal: HOLLYWOOD_TERMINAL,
            cwd: &cwd,
            prompt: prompt.as_deref(),
            resume: None,
            studio_dir: &cwd,
            stage_env: &stage_env,
            role: HOLLYWOOD,
            checks: &[],
        },
    )
    .expect("devin plan");
    hprepare_launch(&hdir, "devin", None).expect("prepare devin hollywood");
    let pf_idx = launch
        .argv
        .iter()
        .position(|a| a == "--prompt-file")
        .expect("devin carries --prompt-file");
    let pf_path = std::path::PathBuf::from(&launch.argv[pf_idx + 1]);
    assert_eq!(pf_path, hdir.join("prompts/hollywood.first.txt"));
    assert_eq!(
        std::fs::read_to_string(&pf_path).expect("prompt file written"),
        format!("Read {} and follow it.\n", hdir.join("AGENTS.md").display())
    );
    assert!(hdir.join("AGENTS.md").is_file());

    // Resume devin launch: no `--prompt-file` arg.
    let resume_prompt = hfirst_prompt(&hdir, "devin", Some("beaded-harp"));
    assert!(resume_prompt.is_none());
    let resumed = plan(
        &actor,
        &LaunchCtx {
            terminal: HOLLYWOOD_TERMINAL,
            cwd: &cwd,
            prompt: resume_prompt.as_deref(),
            resume: Some("beaded-harp"),
            studio_dir: &cwd,
            stage_env: &stage_env,
            role: HOLLYWOOD,
            checks: &[],
        },
    )
    .expect("devin resume plan");
    assert!(!resumed.argv.iter().any(|a| a == "--prompt-file"));

    // Non-devin runner keeps the inline sentence.
    assert_eq!(
        hfirst_prompt(&hdir, "claude", None).as_deref(),
        Some(format!("Read {} and follow it.", hdir.join("AGENTS.md").display()).as_str())
    );
}

/// Hollywood's `terminal.run` asks for a grid a person can read: at
/// least `MIN_COLS` wide and hc's `200×120` default, so a peek or focus
/// starts from a sane size and the program is never born at a strip
/// (H3). A narrow size was what left `bl-hollywood` 21 columns wide.
#[test]
fn hollywood_starts_at_a_readable_grid() {
    use crate::hlife::{HC_DEFAULT_COLS, HC_DEFAULT_ROWS, MIN_COLS, MIN_ROWS, hrun_params};

    let params = hrun_params(
        vec!["claude".into()],
        "/state/hollywood".into(),
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::new(),
    );
    let size = params.size.expect("the run carries an initial size");
    assert!(
        size.cols >= MIN_COLS.max(HC_DEFAULT_COLS),
        "cols {} is under the readable floor",
        size.cols
    );
    assert!(
        size.rows >= MIN_ROWS.max(HC_DEFAULT_ROWS),
        "rows {} is under the readable floor",
        size.rows
    );
    // The labels and the name still mark it Hollywood's terminal.
    assert_eq!(params.name.as_deref(), Some(HOLLYWOOD_TERMINAL));
}
