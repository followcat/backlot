//! The card hardening (field notes §1–2) end to end through the CLI and
//! the stage's card pass: a relative path resolves at propose time and a
//! missing file is refused then, the referenced files are copied into the
//! card's own directory and an edit makes the card stale, Hollywood
//! withdraws only its own open card, a newer card on one fact supersedes
//! the older one, and a finished studio is reopened by the person.

use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::hollywood::{CardAction, CardBase, CardState, HollywoodKind, HollywoodState};

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::draft_tests::FAKE_CAST;
use crate::store;

// ── shared fixtures ─────────────────────────────────────────────

/// A shell with no `BACKLOT_STUDIO` pin — Hollywood's own, where
/// `--studio` picks the target.
fn free(env: &Env) -> Env {
    let mut vars = env.vars.clone();
    vars.remove("BACKLOT_STUDIO");
    Env { vars }
}

/// `acme-shop` on the fake runner — the studio the cards change.
fn fake_studio(scratch: &Scratch, env: &Env) {
    let cast = fixture(scratch, "studio.toml", FAKE_CAST);
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
}

fn hstate(scratch: &Scratch) -> HollywoodState {
    store::load_hstate(&store::hollywood_dir(&scratch.dir)).unwrap()
}

fn card(state: &HollywoodState, seq: u64) -> &backlot_model::hollywood::Card {
    state.cards.get(&seq).expect("a proposed card")
}

/// A real git repository under `files/` — the studio-new dry run asks
/// git itself.
fn git_repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-b", "main"]);
    fs::write(dir.join("f"), "base\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "base"]);
}

fn cast_path(scratch: &Scratch) -> PathBuf {
    scratch.files.join("cast.toml")
}

// ── 1. paths resolved at propose ───────────────────────────────

/// A relative file argument is resolved against the proposer's working
/// directory at propose time, not Hollywood's own folder, so the card
/// carries an absolute snapshot.
#[test]
fn a_relative_cast_path_resolves_at_propose() {
    let work = crate::test_support::TestDir::new("backlot-card-relative");
    fs::write(work.join("cast.toml"), FAKE_CAST).unwrap();
    fs::write(work.join("brief.md"), "# brief\n").unwrap();
    let files = crate::hcard_snapshot::resolve_in(
        &work,
        &CardAction::StudioNew {
            studio: "new-shop".into(),
            title: "New".into(),
            repo: "/home/user/new-shop".into(),
            cast: "cast.toml".into(),
            brief: "brief.md".into(),
            no_git: false,
            snapshot: None,
        },
    )
    .expect("the relative files exist under the proposer's directory")
    .expect("a studio-new draft resolves its files");
    // Compare real paths: on macOS the temp dir is reached through the
    // `/var` -> `/private/var` link, so one side may be spelled either way.
    let real = |p: &std::path::Path| p.canonicalize().unwrap();
    assert_eq!(real(&files.cast), real(&work.join("cast.toml")));
    assert_eq!(
        files.brief.as_deref().map(real),
        Some(real(&work.join("brief.md")))
    );
    assert!(files.cast.is_absolute(), "the resolved path is absolute");
}

/// A file that does not exist then is refused at propose — never
/// accepted and failed later at apply.
#[test]
fn a_missing_file_is_refused_at_propose() {
    let dir = crate::test_support::TestDir::new("backlot-card-missing-file");
    let err = crate::hcard_snapshot::resolve_in(
        &dir,
        &CardAction::Cast {
            studio: "acme-shop".into(),
            cast: "no-such-cast.toml".into(),
            snapshot: None,
        },
    )
    .expect_err("a file that is not there is refused");
    assert!(err.message().contains("file not found"), "{err:?}");
}

/// `propose studio-new` writes the cast and brief into the card's own
/// directory and records their hashes — the snapshot the apply reads.
#[test]
fn propose_studio_new_copies_its_files_into_the_card() {
    let (scratch, env) = scratch();
    let env = free(&env);
    let cast = fixture(&scratch, "cast.toml", FAKE_CAST);
    let brief = fixture(&scratch, "brief.md", "# the brief\n");
    let repo = scratch.files.join("repo");
    git_repo(&repo);
    ok(
        &env,
        &format!(
            "hollywood propose studio-new --studio new-shop --title N \
             --repo {} --cast {cast} --brief {brief} --why w --as hollywood",
            repo.display()
        ),
    );
    let hdir = store::hollywood_dir(&scratch.dir);
    let (events, _) = store::load_hevents(&hdir).unwrap();
    let action = match &events[0].kind {
        HollywoodKind::CardProposed { action, .. } => action.clone(),
        k => panic!("expected card.proposed: {k:?}"),
    };
    match &action {
        CardAction::StudioNew {
            cast,
            brief,
            snapshot,
            ..
        } => {
            assert_eq!(cast, "cards/c1/cast.toml");
            assert_eq!(brief, "cards/c1/brief.md");
            let snapshot = snapshot.as_ref().expect("the card carries a snapshot");
            let snap_cast = snapshot.cast.as_ref().unwrap();
            // The proposer's own path, absolute.
            // Real paths on both sides (macOS reaches temp through `/var` -> `/private/var`).
            assert_eq!(
                std::path::Path::new(&snap_cast.source)
                    .canonicalize()
                    .unwrap(),
                cast_path(&scratch).canonicalize().unwrap()
            );
            assert_eq!(
                snap_cast.sha256,
                backlot_model::hollywood::sha256_hex(FAKE_CAST.as_bytes())
            );
        }
        other => panic!("expected studio-new: {other:?}"),
    }
    assert!(hdir.join("cards/c1/cast.toml").is_file());
    assert!(hdir.join("cards/c1/brief.md").is_file());
}

/// The `card list` line a cast card carries, read from its snapshot.
#[test]
fn card_list_shows_the_cast_roles_from_the_snapshot() {
    let (scratch, env) = scratch();
    fake_studio(&scratch, &env);
    let draft = scratch.files.join("cast.toml");
    fs::write(
        &draft,
        FAKE_CAST
            .replace("model-p", "producer-model")
            .replace("rework_cap = 2", "rework_cap = 3"),
    )
    .unwrap();
    ok(
        &env,
        &format!(
            "hollywood propose cast --studio acme-shop --file {} --why cheaper --as hollywood",
            draft.display()
        ),
    );
    let list = ok(&env, "card list");
    assert!(
        list.contains("producer=fake/producer-model/high"),
        "the roles line is missing:\n{list}"
    );
    assert!(list.contains("director=fake/model-d/high"), "{list}");
    // `card show` carries the same line.
    let show = ok(&env, "card show c1");
    assert!(show.contains("producer=fake/producer-model/high"), "{show}");
}

// ── 2. a snapshot, and the stale check ─────────────────────────

/// An edited source file makes the card `stale` when it is applied — the
/// person re-proposes what the file says now.
#[test]
fn an_edited_cast_file_makes_the_card_stale() {
    let (scratch, env) = scratch();
    fake_studio(&scratch, &env);
    let draft = scratch.files.join("cast.toml");
    fs::write(
        &draft,
        FAKE_CAST.replace("rework_cap = 2", "rework_cap = 4"),
    )
    .unwrap();
    ok(
        &env,
        &format!(
            "hollywood propose cast --studio acme-shop --file {} --why cheaper --as hollywood",
            draft.display()
        ),
    );
    // The proposer edits the file after the card exists.
    fs::write(&draft, FAKE_CAST.replace("model-d", "another-model")).unwrap();
    let out = ok(&env, "card apply c1");
    assert!(out.contains("card.stale c1"), "{out}");
    assert!(
        out.contains("the cast file changed after this card was proposed"),
        "the reason names the edit: {out}"
    );
    assert_eq!(card(&hstate(&scratch), 1).state, CardState::Stale);
}

/// The apply executes the card's own copy, not the proposer's source.
#[test]
fn the_apply_reads_the_cards_copy() {
    let (scratch, env) = scratch();
    fake_studio(&scratch, &env);
    let draft = scratch.files.join("cast.toml");
    let changed = FAKE_CAST.replace("rework_cap = 2", "rework_cap = 4");
    fs::write(&draft, &changed).unwrap();
    ok(
        &env,
        &format!(
            "hollywood propose cast --studio acme-shop --file {} --why cheaper --as hollywood",
            draft.display()
        ),
    );
    let hdir = store::hollywood_dir(&scratch.dir);
    let copy = fs::read_to_string(hdir.join("cards/c1/cast.toml")).unwrap();
    assert_eq!(copy, changed, "the copy is the reviewed bytes");
}

// ── 3. withdraw ────────────────────────────────────────────────

/// `hollywood propose recast` on the director — an open card.
fn propose_recast(env: &Env) {
    ok(
        env,
        "hollywood propose recast --studio acme-shop --person director \
         --runner fake --model model-b --effort high --why quota --as hollywood",
    );
}

/// Hollywood withdraws its own open card; the event records the reason.
#[test]
fn hollywood_withdraws_its_own_card() {
    let (scratch, env) = scratch();
    fake_studio(&scratch, &env);
    propose_recast(&env);
    let out = ok(
        &env,
        "hollywood withdraw c1 --reason quota-returned --as hollywood",
    );
    assert!(out.contains("card.withdrawn c1"), "{out}");
    let state = hstate(&scratch);
    let c = card(&state, 1);
    assert_eq!(c.state, CardState::Withdrawn);
    assert_eq!(c.detail, "quota-returned");
    assert_eq!(c.by, "hollywood", "the card names who proposed it");
}

/// Another role cannot withdraw, and a card the user already cancelled is
/// no longer open to withdraw.
#[test]
fn withdraw_refuses_a_card_hollywood_did_not_propose() {
    let (scratch, env) = scratch();
    fake_studio(&scratch, &env);
    propose_recast(&env);
    let msg = refused(&env, "hollywood withdraw c1 --reason no --as user");
    assert!(msg.contains("the hollywood role can"), "{msg}");
    ok(&env, "card cancel c1");
    let msg = refused(&env, "hollywood withdraw c1 --reason late --as hollywood");
    assert!(msg.contains("is already cancelled"), "{msg}");
}

/// The stage's own outward card is not Hollywood's to withdraw.
#[test]
fn withdraw_refuses_the_stages_card() {
    let (scratch, env) = scratch();
    fake_studio(&scratch, &env);
    // Plant a stage-proposed outward card straight into Hollywood's
    // ledger — the rule is about the proposer, not the shape.
    let hdir = store::hollywood_dir(&scratch.dir);
    store::append_hollywood(&hdir, |_, _| {
        Ok::<_, std::convert::Infallible>(store::HPending {
            event: backlot_model::hollywood::NewHollywoodEvent::new(
                backlot_model::EventBy::role("stage"),
                HollywoodKind::CardProposed {
                    action: CardAction::Outward {
                        studio: "acme-shop".into(),
                        role: "actor-t1-c1".into(),
                        source_seq: 9,
                        argv: vec!["git".into(), "push".into()],
                        cwd: "/home/user/acme-shop".into(),
                        repo: "/home/user/acme-shop".into(),
                        remote_url: "git@host-a:shop.git".into(),
                        rule: "push".into(),
                        commits: Vec::new(),
                        commits_total: 0,
                    },
                    why: "role asked".into(),
                    base: CardBase::GitHead {
                        repo: "/home/user/acme-shop".into(),
                        head_oid: "abc".into(),
                        remote_ref: "refs/heads/main".into(),
                        remote_oid: String::new(),
                    },
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let msg = refused(&env, "hollywood withdraw c1 --reason gone --as hollywood");
    assert!(msg.contains("was not proposed by hollywood"), "{msg}");
}

// ── 4. supersede ───────────────────────────────────────────────

/// A newer recast card on the same role marks the older one superseded in
/// the same append, so a batch apply can never run both.
#[test]
fn a_newer_recast_supersedes_the_older_one() {
    let (scratch, env) = scratch();
    fake_studio(&scratch, &env);
    ok(
        &env,
        "hollywood propose recast --studio acme-shop --person director \
         --runner fake --model model-b --effort high --why first --as hollywood",
    );
    ok(
        &env,
        "hollywood propose recast --studio acme-shop --person director \
         --runner fake --model model-c --effort high --why second --as hollywood",
    );
    let state = hstate(&scratch);
    assert_eq!(card(&state, 1).state, CardState::Superseded);
    assert!(card(&state, 1).detail.contains("superseded by c2"));
    assert_eq!(card(&state, 2).state, CardState::Open);
    let list = ok(&env, "card list");
    assert!(list.contains("superseded"), "{list}");
}

/// The proposal line is appended BEFORE the supersede lines it carries,
/// so a torn tail can only drop a supersede (both cards stay open, as
/// before), never record an old card superseded by a card that does not
/// exist.
#[test]
fn the_proposal_precedes_its_supersedes_in_the_ledger() {
    let (scratch, env) = scratch();
    fake_studio(&scratch, &env);
    ok(
        &env,
        "hollywood propose recast --studio acme-shop --person director \
         --runner fake --model model-b --effort high --why first --as hollywood",
    );
    ok(
        &env,
        "hollywood propose recast --studio acme-shop --person director \
         --runner fake --model model-c --effort high --why second --as hollywood",
    );
    let (events, _) = store::load_hevents(&store::hollywood_dir(&scratch.dir)).unwrap();
    let proposed = events
        .iter()
        .rposition(|e| matches!(e.kind, HollywoodKind::CardProposed { .. }))
        .expect("a second proposal");
    let superseded = events
        .iter()
        .position(|e| matches!(e.kind, HollywoodKind::CardSuperseded { .. }))
        .expect("a supersede");
    assert!(
        proposed < superseded,
        "the proposal must land before the supersede"
    );
    // And the supersede names the card that now exists.
    match &events[superseded].kind {
        HollywoodKind::CardSuperseded { by_card, .. } => {
            assert_eq!(*by_card, events[proposed].seq, "the proposal's own seq")
        }
        other => panic!("expected a supersede, got {other:?}"),
    }
}

/// A recast on a different role never supersedes.
#[test]
fn a_recast_on_another_role_keeps_both_open() {
    let (scratch, env) = scratch();
    fake_studio(&scratch, &env);
    ok(
        &env,
        "hollywood propose recast --studio acme-shop --person director \
         --runner fake --model model-b --effort high --why first --as hollywood",
    );
    ok(
        &env,
        "hollywood propose recast --studio acme-shop --person writer \
         --runner fake --model model-c --effort high --why second --as hollywood",
    );
    let state = hstate(&scratch);
    assert_eq!(card(&state, 1).state, CardState::Open);
    assert_eq!(card(&state, 2).state, CardState::Open);
}

/// Two studio-new cards for one name supersede: a batch apply could not
/// create the studio twice.
#[test]
fn a_second_studio_new_supersedes_the_first() {
    let (scratch, env) = scratch();
    let env = free(&env);
    let cast = fixture(&scratch, "cast.toml", FAKE_CAST);
    let brief = fixture(&scratch, "brief.md", "# brief\n");
    let repo = scratch.files.join("repo");
    git_repo(&repo);
    for _ in 0..2 {
        ok(
            &env,
            &format!(
                "hollywood propose studio-new --studio new-shop --title N \
                 --repo {} --cast {cast} --brief {brief} --why w --as hollywood",
                repo.display()
            ),
        );
    }
    let state = hstate(&scratch);
    assert_eq!(card(&state, 1).state, CardState::Superseded);
    assert_eq!(card(&state, 2).state, CardState::Open);
    // Each proposal's copies were written under its own seq.
    let hdir = store::hollywood_dir(&scratch.dir);
    assert!(hdir.join("cards/c1/cast.toml").is_file());
    assert!(hdir.join("cards/c2/cast.toml").is_file());
}

// ── 5. reopen ──────────────────────────────────────────────────

/// `studio reopen --as user` brings a finished studio back, and a recast
/// works again.
#[test]
fn the_person_reopens_a_finished_studio() {
    let (scratch, env) = scratch();
    fake_studio(&scratch, &env);
    ok(&env, "studio finish --force --reason done --as user");
    let msg = refused(
        &env,
        "recast --person director --runner fake --model model-q --effort high",
    );
    assert!(msg.contains("finished"), "{msg}");
    let out = ok(&env, "studio reopen --as user --reason more-work");
    assert!(out.contains("studio.reopened"), "{out}");
    // A recast works again.
    ok(
        &env,
        "recast --person director --runner fake --model model-q --effort high",
    );
    let state = store::load_state(&store::studio_dir(&scratch.dir, "acme-shop")).unwrap();
    assert!(state.finished.is_none(), "the studio runs again");
    let reopened = state.reopened.expect("a studio.reopened record");
    assert_eq!(reopened.reason, "more-work");
}

/// Reopening is the person's: a role and Hollywood itself are refused, and
/// Hollywood may only ask with a card.
#[test]
fn only_the_person_reopens() {
    let (scratch, env) = scratch();
    fake_studio(&scratch, &env);
    ok(&env, "studio finish --force --as user");
    let msg = refused(&env, "studio reopen --as director");
    assert!(msg.contains("the person's call"), "{msg}");
    // Hollywood may not run a studio command at all — it may only
    // propose a `reopen-studio` card.
    let msg = refused(&env, "studio reopen --as hollywood");
    assert!(msg.contains("cannot do this"), "{msg}");
    // Hollywood asks with a `reopen-studio` card instead.
    let out = ok(
        &env,
        "hollywood propose reopen-studio --studio acme-shop --why reopen --as hollywood",
    );
    assert!(out.contains("reopen studio acme-shop"), "{out}");
    let state = hstate(&scratch);
    let id = state.cards.keys().next().copied().unwrap();
    assert_eq!(card(&state, id).base, CardBase::StudioFinished);
}

/// A `reopen-studio` card whose studio is not finished is refused at
/// propose — a card born stale never exists.
#[test]
fn a_reopen_card_on_a_running_studio_is_refused() {
    let (scratch, env) = scratch();
    fake_studio(&scratch, &env);
    let msg = refused(
        &env,
        "hollywood propose reopen-studio --studio acme-shop --why x --as hollywood",
    );
    assert!(msg.contains("not finished"), "{msg}");
    assert!(hstate(&scratch).cards.is_empty(), "no card was recorded");
}

// ── 6. the name rule ───────────────────────────────────────────

/// The studio name rule is visible in the refusal and in both helps,
/// without changing the length.
#[test]
fn the_studio_name_rule_is_named() {
    let (_scratch, env) = scratch();
    let env = free(&env);
    // The refusal names the rule.
    let err = crate::cli::run(
        &[
            "studio".into(),
            "new".into(),
            "--title".into(),
            "T".into(),
            "--repo".into(),
            "/home/user/x".into(),
            "--cast".into(),
            "x.toml".into(),
            "--studio".into(),
            "acme-shop-with-a-long-name".into(),
        ],
        &env,
    )
    .unwrap_err();
    assert!(err.message().contains("[a-z0-9_-]{1,16}"), "{err:?}");

    // `studio new`'s own help carries it.
    let help = crate::cli::run(&["studio".into(), "new".into(), "--help".into()], &env).unwrap();
    assert!(help.contains("[a-z0-9_-]{1,16}"), "{help}");
    // `propose studio-new`'s help carries it.
    let help = crate::cli::run(
        &["hollywood".into(), "propose".into(), "--help".into()],
        &env,
    )
    .unwrap();
    assert!(help.contains("[a-z0-9_-]{1,16}"), "{help}");
    // The length is unchanged: a 16-char name passes, 17 does not.
    assert!(crate::cli::valid_studio_name("a234567890123456"));
    assert!(!crate::cli::valid_studio_name("a2345678901234567"));
}
