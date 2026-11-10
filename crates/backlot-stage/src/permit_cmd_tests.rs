//! S9b part 1 — prompts that reach for `BACKLOT_*` or the
//! environment-clearing flags are the user's, end to end through
//! the CLI: the director cannot record the answer, `status` marks
//! the attention as the user's, and only the user's approval lands.

use std::fs;
use std::path::Path;

use backlot_model::event::{EventKind, PERMISSION_PROMPT};
use backlot_model::testkit as tk;
use backlot_model::{EventBy, NewEvent};

use crate::cli_testkit::*;
use crate::store::studio_dir;

/// A studio whose actor on t1 sits at a permission prompt — the
/// `needs_user` flag is `flagged`.
fn studio_at_prompt(scratch: &Scratch, role: &str, flagged: bool) {
    let dir = studio_dir(&scratch.dir, "acme-shop");
    fs::create_dir_all(&dir).unwrap();
    let ev = |seq: u64, by: &str, kind: EventKind| {
        NewEvent::new(EventBy::role(by), kind).stamp(seq, seq * 1000)
    };
    write_log(
        &dir,
        &[
            ev(1, "user", tk::studio_kind()),
            ev(
                2,
                "producer",
                EventKind::ReqVersion {
                    version: 1,
                    file: "req/v1.md".into(),
                    summary: "first".into(),
                },
            ),
            ev(3, "director", tk::task_created("t1", "feature", 1)),
            ev(4, "director", tk::cast("t1", "actor-a", "a")),
            ev(
                5,
                "stage",
                EventKind::AttentionRaised {
                    role: role.into(),
                    reason: PERMISSION_PROMPT.into(),
                    detail: "needs the user; screen saved to attention/5.txt".into(),
                    needs_user: flagged,
                },
            ),
        ],
    );
}

/// A flagged prompt refuses the director — the refusal names the
/// variables — and accepts the user; the ordinary actor prompt
/// stays the director's.
#[test]
fn a_flagged_prompt_is_the_users_to_answer() {
    let (scratch, env) = scratch();
    studio_at_prompt(&scratch, "actor-t1-c1", true);
    let msg = refused(
        &env,
        "approve --person actor-t1-c1 --decision allow --as director",
    );
    assert_eq!(
        msg,
        "the permission prompt on 'actor-t1-c1' is the user's — its screen \
         reached for BACKLOT_* or an environment-clearing env flag; the \
         director cannot answer it"
    );
    ok(
        &env,
        "approve --person actor-t1-c1 --decision allow --as user",
    );
    let json = ok(&env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let approvals = v["approvals"].as_array().unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0]["by"], "user");
    // The ledger keeps the flag: a replayed `needs_user` attention
    // marks itself in the state.
    assert_eq!(v["attention"][0]["needs_user"], true);
}

/// The director never could answer a non-actor's prompt — D9
/// delegates only an actor's — and still cannot.
#[test]
fn a_non_actors_prompt_is_the_users_too() {
    let (scratch, env) = scratch();
    studio_at_prompt(&scratch, "writer", false);
    let msg = refused(
        &env,
        "approve --person writer --decision allow --as director",
    );
    assert_eq!(
        msg,
        "the permission prompt on 'writer' is the user's — 'writer' is not \
         an actor; only an actor's prompt is the director's; the director \
         cannot answer it"
    );
    ok(&env, "approve --person writer --decision deny --as user");
}

/// An unflagged actor prompt remains the director's, and `status`
/// shows a user-owned prompt as the user's.
#[test]
fn the_director_still_answers_an_ordinary_actor_prompt() {
    let (scratch, env) = scratch();
    studio_at_prompt(&scratch, "actor-t1-c1", false);
    ok(
        &env,
        "approve --person actor-t1-c1 --decision allow --as director",
    );
    // …while a flagged one marks itself in `status`.
    let (scratch2, env) = crate::cli_testkit::scratch();
    studio_at_prompt(&scratch2, "actor-t1-c1", true);
    let out = ok(&env, "status");
    assert!(
        out.contains("actor-t1-c1: permission prompt — needs the user"),
        "{out}"
    );
}

/// Serialize the events one per line — the log's own format.
fn write_log(dir: &Path, log: &[backlot_model::Event]) {
    let mut text = String::new();
    for e in log {
        text.push_str(&serde_json::to_string(e).unwrap());
        text.push('\n');
    }
    fs::write(dir.join("events.jsonl"), text).unwrap();
}
