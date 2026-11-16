//! The lifecycle decision's truth table (`hplan`): what the config,
//! the role record and a live `bl-hollywood` together owe — start,
//! adopt, rejoin, lost, resume, the crash guard's give-up, close and
//! drop — plus the actor identity and the terminal env the launch
//! hands the runner.

use backlot_model::event::ActorIdentity;
use backlot_model::hollywood::HWRole;

use crate::config::HollywoodCfg;
use crate::hlife::hrole_env;
use crate::hstage::{HMove, hplan, identity};

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
    assert_eq!(env["PATH"], "/exe:/opt/bin");
    assert_eq!(env["PROXY"], "http://proxy:1");
    assert_eq!(env["FAKE_MODEL"], "model-h");
    // No configured PATH: the stage's own is the whole value.
    let env = hrole_env(
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::new(),
        "/state",
        "/exe",
        "/usr/bin",
    );
    assert_eq!(env["PATH"], "/usr/bin");
}
