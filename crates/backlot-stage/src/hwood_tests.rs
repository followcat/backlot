//! The overseer commands end to end: `hollywood propose`/`report`,
//! the card gate, `tell hollywood` and the producer-message mirror
//! — all through `cli::run` on a scratch state dir, the same way
//! the studio commands are tested. The cross-studio reads live in
//! `hwood_read_tests.rs`.

use backlot_model::hollywood::{CardBase, CardState, HollywoodKind};

use crate::cli::{Env, run};
use crate::cli_testkit::*;
use crate::store;

/// `run` with real argv — `args()` splits on spaces, so a headline
/// or message with spaces arrives as one argv item here, like a
/// shell's quoted string.
pub(crate) fn argv(env: &Env, parts: &[&str]) -> Result<String, crate::cli::Fail> {
    let v: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
    run(&v, env)
}

pub(crate) fn argv_ok(env: &Env, parts: &[&str]) -> String {
    argv(env, parts).expect("command failed")
}

fn argv_refused(env: &Env, parts: &[&str]) -> String {
    match argv(env, parts) {
        Err(crate::cli::Fail::Refused(m)) => m,
        other => panic!("expected refusal on {parts:?}, got {other:?}"),
    }
}

/// Hollywood's own shell: no `BACKLOT_STUDIO` pins its reads to one
/// studio — `--studio` picks the target freely.
pub(crate) fn hwood_env(env: &Env) -> Env {
    let mut vars = env.vars.clone();
    vars.remove("BACKLOT_STUDIO");
    Env { vars }
}

/// acme-shop with requirement v1 and task `t1` running under
/// actor-a — the state a `pause t1` card proposes against.
pub(crate) fn running(scratch: &Scratch, env: &Env) {
    let cast = fixture(scratch, "cast.toml", CAST_TOML);
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let req = fixture(scratch, "req.md", "# req\n");
    ok(
        env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    let acc = fixture(scratch, "acc.md", "# acc\n");
    ok(
        env,
        &format!("task new --title cart --kind feature --acceptance-file {acc} --as director"),
    );
    ok(env, "cast --task t1 --actor actor-a --as director");
}

pub(crate) fn hwood_events(scratch: &Scratch) -> Vec<backlot_model::hollywood::HollywoodEvent> {
    let dir = store::hollywood_dir(&scratch.dir);
    let (events, _) = store::load_hevents(&dir).unwrap();
    events
}

fn hstate(scratch: &Scratch) -> backlot_model::hollywood::HollywoodState {
    store::load_hstate(&store::hollywood_dir(&scratch.dir)).unwrap()
}

pub(crate) fn studio_events(scratch: &Scratch) -> Vec<backlot_model::Event> {
    let (events, _) = store::load_events(&store::studio_dir(&scratch.dir, "acme-shop")).unwrap();
    events
}

/// D17a: a studio identifier carried in a card never builds a path
/// — `card apply` refuses it, `studio_state` refuses every hostile
/// spelling, and a relative draft path cannot climb out of
/// Hollywood's directory.
#[test]
fn a_hostile_card_identifier_never_reaches_a_path() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    // A card planted straight into Hollywood's ledger with a hostile
    // studio name, proposed so `card apply` reaches for the studio.
    let hdir = store::hollywood_dir(&scratch.dir);
    store::append_hollywood(&hdir, |_, _| {
        Ok::<_, std::convert::Infallible>(store::HPending {
            event: backlot_model::hollywood::NewHollywoodEvent::new(
                backlot_model::EventBy::role("hollywood"),
                HollywoodKind::CardProposed {
                    action: backlot_model::hollywood::CardAction::Pause {
                        studio: "../escape".into(),
                        task: "t1".into(),
                    },
                    why: "hostile".into(),
                    base: CardBase::TaskStatus {
                        was: backlot_model::TaskStatus::Running,
                    },
                },
            ),
            files: vec![],
        })
    })
    .unwrap();
    let msg = refused(&env, "card apply c1");
    assert!(msg.contains("bad studio name"), "{msg}");
    assert!(!scratch.dir.join("escape").exists());
    // The same check guards every spelling — traversal, separators,
    // control bytes, option-lookalikes, case, empty.
    for name in [
        "../escape",
        "..",
        "a/b",
        "foo\nbar",
        "-x",
        "--force",
        "UPPER",
        "",
    ] {
        assert!(
            crate::hwood::studio_state(&scratch.dir, name).is_err(),
            "{name:?}"
        );
    }
    assert!(
        crate::hwood::studio_state(&scratch.dir, "acme-shop")
            .unwrap()
            .is_some()
    );
    // Draft paths on a `studio-new` card: `..` never leaves `hdir`,
    // a clean relative path resolves inside it.
    assert!(crate::hcard_new::resolve_draft(&hdir, "../../etc/passwd").starts_with(&hdir));
    assert_eq!(
        crate::hcard_new::resolve_draft(&hdir, "drafts/cast.toml"),
        hdir.join("drafts/cast.toml")
    );
}

#[test]
fn propose_records_a_card_in_hollywoods_own_ledger() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    let before = studio_events(&scratch).len();

    let out = argv_ok(
        &env,
        &[
            "hollywood",
            "propose",
            "pause",
            "--studio",
            "acme-shop",
            "--task",
            "t1",
            "--why",
            "the actor is looping",
            "--as",
            "hollywood",
        ],
    );
    assert!(out.contains("card.proposed c1"), "{out}");
    assert!(out.contains("pause t1"), "{out}");

    let events = hwood_events(&scratch);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].seq, 1);
    assert_eq!(events[0].by.role, "hollywood");
    match &events[0].kind {
        HollywoodKind::CardProposed { action, why, base } => {
            assert_eq!(action.label(), "pause t1");
            assert_eq!(why, "the actor is looping");
            assert_eq!(
                *base,
                CardBase::TaskStatus {
                    was: backlot_model::TaskStatus::Running
                }
            );
        }
        k => panic!("expected card.proposed, got {k:?}"),
    }
    // The ledger sits beside studios/, never inside it, and the
    // studio's log gained nothing.
    assert_eq!(studio_events(&scratch).len(), before);
    assert!(scratch.dir.join("hollywood/events.jsonl").is_file());
    assert!(!scratch.dir.join("studios/hollywood").exists());
    // Hollywood never lists as a studio.
    assert!(
        !store::studios(&scratch.dir)
            .unwrap()
            .contains(&"hollywood".into())
    );
}

#[test]
fn propose_and_report_are_hollywoods_alone() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    for role in ["user", "producer", "director", "actor-t1-c1"] {
        let msg = refused(
            &env,
            &format!("hollywood propose pause --studio acme-shop --task t1 --why w --as {role}"),
        );
        assert!(msg.contains("the hollywood role can"), "{role}: {msg}");
    }
    let msg = argv_refused(&env, &["hollywood", "report", "all quiet", "--as", "user"]);
    assert!(msg.contains("the hollywood role can"), "{msg}");
}

#[test]
fn propose_checks_the_base_against_the_studio() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    let msg = refused(
        &env,
        "hollywood propose pause --studio acme-shop --task t9 --why w --as hollywood",
    );
    assert!(msg.contains("no task 't9'"), "{msg}");
    // t2 is created, not running — a pause cannot base on it.
    let acc = fixture(&scratch, "acc2.md", "# acc\n");
    ok(
        &env,
        &format!("task new --title more --kind fix --acceptance-file {acc} --as director"),
    );
    let msg = refused(
        &env,
        "hollywood propose pause --studio acme-shop --task t2 --why w --as hollywood",
    );
    assert!(msg.contains("created"), "{msg}");
    // A studio that does not exist at all.
    let msg = refused(
        &hwood_env(&env),
        "hollywood propose pause --studio no-shop --task t1 --why w --as hollywood",
    );
    assert!(msg.contains("no studio 'no-shop'"), "{msg}");
}

#[test]
fn hollywood_report_records_a_headline() {
    let (scratch, env) = scratch();
    let body = fixture(&scratch, "daily.md", "# daily\n");
    let out = argv_ok(
        &env,
        &[
            "hollywood",
            "report",
            "all quiet on the lot",
            "--body",
            &body,
            "--as",
            "hollywood",
        ],
    );
    assert!(out.contains("hollywood.reported"), "{out}");
    let events = hwood_events(&scratch);
    match &events[0].kind {
        HollywoodKind::Reported { headline, body } => {
            assert_eq!(headline, "all quiet on the lot");
            let dest = body.as_deref().unwrap();
            assert!(dest.starts_with("reports/"), "{dest}");
            assert!(store::hollywood_dir(&scratch.dir).join(dest).is_file());
        }
        k => panic!("expected hollywood.reported, got {k:?}"),
    }
    // An empty headline is the model's refusal, not a usage error.
    let msg = argv_refused(&env, &["hollywood", "report", " ", "--as", "hollywood"]);
    assert!(msg.contains("nonempty"), "{msg}");
}

#[test]
fn the_card_gate() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    ok(
        &env,
        "hollywood propose pause --studio acme-shop --task t1 --why looping --as hollywood",
    );
    ok(
        &env,
        "hollywood propose abandon --studio acme-shop --task t1 --why hopeless --as hollywood",
    );

    let list = ok(&env, "card list");
    assert!(list.contains("c1"), "{list}");
    assert!(list.contains("c2"), "{list}");

    // Apply records card.applied and stops — carrying it out is H3.
    let out = ok(&env, "card apply c1");
    assert!(out.contains("card.applied c1"), "{out}");
    assert!(out.contains("stage"), "{out}");
    assert_eq!(hstate(&scratch).cards[&1].state, CardState::Applied);

    let out = ok(&env, "card cancel c2 --reason never-mind");
    assert!(out.contains("card.cancelled c2"), "{out}");
    assert_eq!(hstate(&scratch).cards[&2].state, CardState::Cancelled);

    // A closed card cannot be applied or cancelled again.
    let msg = refused(&env, "card apply c1");
    assert!(msg.contains("already applied"), "{msg}");
    let msg = refused(&env, "card apply c9");
    assert!(msg.contains("no card 'c9'"), "{msg}");
}

#[test]
fn an_apply_on_a_moved_base_records_stale() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    ok(
        &env,
        "hollywood propose pause --studio acme-shop --task t1 --why looping --as hollywood",
    );
    // The task is already paused — the base no longer holds.
    ok(&env, "task pause --task t1 --reason hold --as user");
    let out = ok(&env, "card apply c1");
    assert!(out.contains("card.stale c1"), "{out}");
    assert!(out.contains("paused now"), "{out}");
    assert_eq!(hstate(&scratch).cards[&1].state, CardState::Stale);
}

#[test]
fn card_apply_and_cancel_refuse_a_role_shell() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    ok(
        &env,
        "hollywood propose pause --studio acme-shop --task t1 --why looping --as hollywood",
    );
    let mut vars = env.vars.clone();
    vars.insert("BACKLOT_ROLE".into(), "hollywood".into());
    let role_env = Env { vars };
    for cmd in ["card apply c1", "card cancel c1"] {
        let msg = refused(&role_env, cmd);
        assert!(msg.contains("BACKLOT_ROLE"), "{cmd}: {msg}");
    }
    // `card list` is a read — the role's own cards stay visible.
    assert!(ok(&role_env, "card list").contains("c1"));
}

#[test]
fn tell_hollywood_records_for_later() {
    let (scratch, env) = scratch();
    // No studio at all — the overseer's ledger stands alone.
    let out = argv_ok(&env, &["tell", "hollywood", "watch acme-shop"]);
    assert!(out.contains("message.sent"), "{out}");
    match &hwood_events(&scratch)[0].kind {
        HollywoodKind::MessageSent {
            to,
            studio,
            text,
            file,
            ..
        } => {
            assert_eq!(to, "hollywood");
            assert_eq!(*studio, None);
            assert_eq!(text.as_deref(), Some("watch acme-shop"));
            assert_eq!(*file, None);
        }
        k => panic!("expected message.sent, got {k:?}"),
    }
    // An actor key cannot write Hollywood's inbox.
    let msg = refused(&env, "tell hollywood hi --as actor-t1-c1");
    assert!(msg.contains("may not send"), "{msg}");
    // An empty line says nothing.
    assert!(matches!(
        argv(&env, &["tell", "hollywood", ""]),
        Err(crate::cli::Fail::Usage(_))
    ));
}

#[test]
fn hollywood_tell_producer_mirrors_to_its_own_ledger() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    let msg = fixture(&scratch, "note.md", "the daily word\n");
    let out = ok(&env, &format!("tell producer --file {msg} --as hollywood"));
    // The studio's message.queued is the delivery record …
    assert!(out.contains("message.queued"), "{out}");
    match studio_events(&scratch).last().unwrap().kind {
        backlot_model::EventKind::MessageQueued {
            ref to, ref file, ..
        } => {
            assert_eq!(to, "producer");
            assert!(file.is_some());
        }
        ref k => panic!("expected message.queued, got {k:?}"),
    }
    // … and Hollywood's own copy lands in its ledger (R26).
    assert!(out.contains("message.sent"), "{out}");
    match &hwood_events(&scratch)[0].kind {
        HollywoodKind::MessageSent {
            to, studio, file, ..
        } => {
            assert_eq!(to, "producer");
            assert_eq!(studio.as_deref(), Some("acme-shop"));
            let dest = file.as_deref().unwrap();
            assert!(dest.starts_with("messages/"), "{dest}");
            assert!(store::hollywood_dir(&scratch.dir).join(dest).is_file());
        }
        k => panic!("expected message.sent, got {k:?}"),
    }
}

#[test]
fn hollywood_seqs_are_its_own() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    ok(
        &env,
        "hollywood propose pause --studio acme-shop --task t1 --why looping --as hollywood",
    );
    ok(&env, "tell hollywood noted");
    argv_ok(
        &env,
        &["hollywood", "report", "one card", "--as", "hollywood"],
    );
    let events = hwood_events(&scratch);
    let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, vec![1, 2, 3]);
    // The studio's own numbering is untouched — its ledger still
    // holds only its own 1..n.
    let studio_seqs: Vec<u64> = studio_events(&scratch).iter().map(|e| e.seq).collect();
    assert_eq!(
        studio_seqs,
        (1..=studio_seqs.len() as u64).collect::<Vec<_>>()
    );
}

#[test]
fn the_reserved_name_is_no_studio() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    // `studio new` runs where no BACKLOT_STUDIO pins the shell.
    let mut vars = env.vars.clone();
    vars.remove("BACKLOT_STUDIO");
    let env = Env { vars };
    let msg = refused(
        &env,
        &format!(
            "studio new --title H --repo /home/user/acme-shop --cast {cast} --studio hollywood"
        ),
    );
    assert!(msg.contains("reserved"), "{msg}");
}

#[test]
fn a_studio_role_cannot_be_hollywood() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    // `tell producer` is Hollywood's one write into a studio (R26);
    // `ask` and `report` record no event it may make, and refuse at
    // the command — `ask` builds the same event `tell` does.
    let file = fixture(&scratch, "note.md", "word\n");
    let msg = refused(&env, &format!("ask --file {file} --as hollywood"));
    assert!(
        msg.contains("in a studio it may only 'tell <studio> producer'"),
        "{msg}"
    );
    let msg = refused(
        &env,
        &format!("report --node weekly --file {file} --as hollywood"),
    );
    assert!(
        msg.contains("in a studio it may only 'tell <studio> producer'"),
        "{msg}"
    );
    // `--all` is no report flag at all.
    assert!(
        usage(
            &env,
            &format!("report --node n --file {file} --all --as hollywood")
        )
        .contains("--all")
    );
    // Every other studio event refuses at the ledger's role gate.
    let acc = fixture(&scratch, "accx.md", "# acc\n");
    let msg = refused(
        &env,
        &format!("task new --title x --kind k --acceptance-file {acc} --as hollywood"),
    );
    assert!(msg.contains("cannot do this"), "{msg}");
    let msg = refused(&env, "priority --task t1 --level high --as hollywood");
    assert!(msg.contains("cannot do this"), "{msg}");
    let msg = refused(&env, "cast --task t1 --actor actor-b --as hollywood");
    assert!(msg.contains("cannot do this"), "{msg}");
    // And the stage is never Hollywood's either (D13).
    let msg = refused(&env, "stage run --as hollywood");
    assert!(msg.contains("cannot do this"), "{msg}");
}
