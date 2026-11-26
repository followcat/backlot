//! The three step-2 verbs over the server queue (`plugin.backlot.req`):
//! `studio cast show` and `studio cast set` (the file as content, never a
//! path), and `recast` on an actor key. Each runs the same code the local
//! CLI does, under `Caller::doc_user` (`by.via = "doc"`).

use backlot_model::req_results::{RecastResult, StudioCastSetResult, StudioCastShowResult};
use backlot_model::req_wire::ReqAction;

use crate::cli_testkit::{fixture, ok, scratch};
use crate::doc_req_studio_seed::{Server, err_line};
use crate::store;
use crate::studio_cast_tests::REAL_CAST;

/// `REAL_CAST`'s runners all resolve on this machine, so `set`'s
/// runner check passes; one extra candidate on `fake`.
fn cast_with_candidate() -> String {
    let mut s = String::from(REAL_CAST);
    s.push_str(
        "\n[[candidates]]\nname = \"actor-c\"\nrunner = \"fake\"\nmodel = \"m\"\n\
         effort = \"high\"\npermission_profile = \"worktree\"\n",
    );
    s
}

/// `studio cast show` over the queue returns the effective cast as a
/// cast file — the same text the local CLI prints.
#[test]
fn cast_show_round_trips_as_a_cast_file() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", REAL_CAST);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let local = ok(&env, "studio cast show");

    let mut server = Server::start(&env);
    let res: StudioCastShowResult = server.typed(
        &ReqAction::StudioCastShow {
            studio: "acme-shop".into(),
            json: false,
        },
        None,
    );
    assert_eq!(res.cast, local, "the queue answers the CLI's own text");
    assert!(res.settings.is_none(), "plain show carries no JSON");
    // `--json` carries the structured settings too.
    let js: StudioCastShowResult = server.typed(
        &ReqAction::StudioCastShow {
            studio: "acme-shop".into(),
            json: true,
        },
        None,
    );
    assert_eq!(js.settings.unwrap().rework_cap, 2);
}

/// `studio cast set --dry-run` over the queue returns the diff and
/// records nothing; the real set records one `studio.cast` `by.via=doc`.
#[test]
fn cast_set_dry_run_and_commit_travel_as_content() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", REAL_CAST);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    let before = store::load_events(&dir).unwrap().0.len();

    let mut server = Server::start(&env);
    let body = cast_with_candidate();

    // The dry run: a diff, no append.
    let dry: StudioCastSetResult = server.typed(
        &ReqAction::StudioCastSet {
            studio: "acme-shop".into(),
            cast: body.clone(),
            dry_run: true,
            json: false,
        },
        None,
    );
    assert!(dry.dry_run);
    assert_eq!(dry.seq, None);
    assert_eq!(
        dry.diff.candidates.first().map(|c| c.name.as_str()),
        Some("actor-c")
    );
    assert_eq!(store::load_events(&dir).unwrap().0.len(), before);

    // The commit: one `studio.cast` appended, by the doc user.
    let set: StudioCastSetResult = server.typed(
        &ReqAction::StudioCastSet {
            studio: "acme-shop".into(),
            cast: body,
            dry_run: false,
            json: false,
        },
        None,
    );
    assert!(!set.dry_run);
    assert!(set.seq.is_some());
    let events = store::load_events(&dir).unwrap().0;
    let last = events.last().unwrap();
    assert_eq!(last.kind.name(), "studio.cast");
    assert_eq!(last.by.role, "user");
    assert_eq!(last.by.via.as_deref(), Some("doc"));
    match &last.kind {
        backlot_model::EventKind::StudioCast { candidates, .. } => {
            assert_eq!(candidates.last().unwrap().name, "actor-c");
        }
        k => panic!("expected studio.cast, got {k:?}"),
    }
}

/// An unchanged file says "nothing changes" as a refusal ack.
#[test]
fn cast_set_unchanged_is_refused() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", REAL_CAST);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let mut server = Server::start(&env);
    let ack = server.send(
        &ReqAction::StudioCastSet {
            studio: "acme-shop".into(),
            cast: REAL_CAST.to_owned(),
            dry_run: true,
            json: false,
        },
        None,
    );
    let line = err_line(&ack);
    assert!(line.contains("nothing changes"), "{line}");
}

/// `recast` on an actor key travels over the queue: the task's newest
/// cast moves, `by.via=doc`, and the key stays.
#[test]
fn recast_an_actor_key_over_the_queue() {
    let (scratch, env) = scratch();
    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("design_gate = \"off\"\n{REAL_CAST}"),
    );
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let req = fixture(&scratch, "req.md", "# req\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    let acc = fixture(&scratch, "acc.md", "# acc\n");
    ok(
        &env,
        &format!("task new --title cart --kind feature --acceptance-file {acc} --as director"),
    );
    ok(&env, "cast --task t1 --actor actor-a --as director");

    let mut server = Server::start(&env);
    let res: RecastResult = server.typed(
        &ReqAction::Recast {
            studio: "acme-shop".into(),
            person: "actor-t1-c1".into(),
            runner: "fake".into(),
            model: "new-model".into(),
            effort: Some("high".into()),
            permission_profile: None,
            reason: "quota".into(),
        },
        None,
    );
    assert_eq!(res.person, "actor-t1-c1");
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    let events = store::load_events(&dir).unwrap().0;
    let last = events.last().unwrap();
    assert_eq!(last.kind.name(), "role.recast");
    assert_eq!(last.by.via.as_deref(), Some("doc"));
    let state = store::load_state(&dir).unwrap();
    assert_eq!(
        state.tasks["t1"].cast.as_ref().unwrap().actor.model,
        "new-model"
    );
    assert_eq!(state.tasks["t1"].casts.len(), 1, "the key stays");
}
