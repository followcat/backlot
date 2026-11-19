//! D17a argv hygiene at the `run_action` boundary: a word carrying
//! a control byte — a hostile task id, role or card id flowing out
//! of a summary document — never spawns; the refusal rides the same
//! failure-toast path a run error takes. Option-lookalike
//! identifiers stay exactly one argv word.

use super::*;
use crate::action::{Action, unsafe_word};
use crate::studio_cmds;

/// The runner records every argv it is asked to spawn.
fn recorder() -> (
    std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    crate::app::action_tracker::TestRunner,
) {
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    let runner: crate::app::action_tracker::TestRunner = std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(String::new()) })
    });
    (recorded, runner)
}

fn action(argv: Vec<String>, machine: Option<String>) -> Action {
    Action {
        label: "testing".into(),
        done: "tested".into(),
        name: "test action".into(),
        mark: format!("test:{:?}", argv.last()),
        argv,
        machine,
        stdin: None,
        view: false,
    }
}

#[test]
fn unsafe_word_flags_only_control_bytes() {
    for bad in ["foo\nbar", "a\0b", "\u{1b}[2J", "x\ry"] {
        assert!(unsafe_word(bad), "{bad:?}");
    }
    for good in ["--force", "-x", "../escape", "c1;id", "acme-shop", "a b"] {
        assert!(!unsafe_word(good), "{good:?}");
    }
}

/// NUL/newline/ESC in any argv word or the machine name refuses the
/// spawn and lands the refusal on the failure toast.
#[tokio::test]
async fn a_control_byte_in_argv_never_spawns() {
    let mut app = app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    for bad in [
        action(
            vec![
                "task".into(),
                "pause".into(),
                "--task".into(),
                "t1\nnewline".into(),
            ],
            None,
        ),
        action(vec!["card".into(), "apply".into(), "c1\0".into()], None),
        action(
            vec!["stage".into(), "start".into()],
            Some("host\u{1b}".into()),
        ),
    ] {
        app.run_action(bad);
    }
    assert!(recorded.lock().unwrap().is_empty(), "nothing spawned");
    let toast = app.action_toast.as_ref().unwrap();
    assert!(toast.fail);
    assert!(
        toast.body.contains("refused unsafe argument"),
        "{}",
        toast.body
    );
}

/// A clean argv — including an option-lookalike identifier — spawns
/// untouched, each hostile string still exactly one word for the
/// backlot parser to refuse downstream.
#[tokio::test]
async fn a_clean_or_option_lookalike_argv_spawns_untouched() {
    let mut app = app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.run_action(action(
        vec!["card".into(), "apply".into(), "--force".into()],
        None,
    ));
    app.run_action(action(
        studio_cmds::task(
            &crate::studio::StudioKey {
                id: "acme-shop".into(),
                machine: None,
            },
            "c1;id",
            "pause",
            None,
        )
        .argv,
        None,
    ));
    let got = recorded.lock().unwrap().clone();
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[0], vec!["card", "apply", "--force"]);
    assert!(got[1].contains(&"c1;id".to_string()), "{:?}", got[1]);
    // And the display line shell-quotes the same word — `copy cmd`
    // can never inject it.
    let pending = crate::action::Pending {
        id: 1,
        action: studio_cmds::tell(
            &crate::studio::StudioKey {
                id: "acme-shop".into(),
                machine: None,
            },
            "writer",
            "hi'; rm -rf ~ #",
        ),
        text: "x".into(),
    };
    let line = crate::action::copy_line(&pending);
    assert!(line.contains("'hi'\\''; rm -rf ~ #'"), "{line}");
}
