//! Fail-first tests for `CardAction::RepoNote`, `CardBase::RepoNoteEntry`,
//! `<state_dir>/repo-notes/<hash>.md` (`0600`), static secret/DSN refusal,
//! stage bridging of studio `repo_note` messages, user-only `show`/`remove`,
//! and read-only prompt injection for `director` and `writer`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use backlot_model::event::{ActorIdentity, EventKind};
use backlot_model::hollywood::CardState;
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::cli::{Env, Fail, run};
use crate::cli_testkit::{CAST_TOML, Scratch, fixture, git, scratch};
use crate::daemon::Stage;
use crate::deliver::outbox;
use crate::prompts;
use crate::reconcile::{RoleKind, Start};
use crate::store;

fn run_args(env: &Env, args: &[&str]) -> Result<String, Fail> {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    run(&owned, env)
}

fn ok_args(env: &Env, args: &[&str]) -> String {
    run_args(env, args).unwrap_or_else(|e| panic!("expected Ok for {args:?}, got {e:?}"))
}

async fn dummy_stage(base: &Path) -> Stage {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (read, mut write) = tokio::io::split(core_side);
        let mut reader = Reader::new(read);
        let hello = reader.read_line().await.unwrap().unwrap();
        let req: Value = serde_json::from_slice(&hello).unwrap();
        let reply = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": req["id"],
            "result": {
                "generation": 1, "version": "fake-core 0.0",
                "framed": true,
                "core": {"id": "core-1", "name": "fake"},
                "methods": [], "features": [],
            }
        }))
        .unwrap();
        write.write_all(&reply).await.unwrap();
        write.write_all(b"\n").await.unwrap();
        while let Ok(Some(_)) = reader.read_frame().await {}
    });
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    let mut stage = Stage::for_test(client);
    stage.base = base.to_path_buf();
    stage.state_dir = base.to_string_lossy().into_owned();
    stage
}

fn setup_repo_studio() -> (Scratch, Env, PathBuf, PathBuf) {
    let (scratch, env) = scratch();
    let repo = scratch.files.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("README.md"), "# Acme Repo\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "init"]);

    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("design_gate = \"off\"\n{CAST_TOML}"),
    );
    ok_args(
        &env,
        &[
            "studio",
            "new",
            "--title",
            "Shop",
            "--repo",
            &repo.to_string_lossy(),
            "--cast",
            &cast,
        ],
    );
    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    (scratch, env, repo, sdir)
}

fn hwood_only_env(env: &Env) -> Env {
    let mut vars = env.vars.clone();
    vars.remove("BACKLOT_STUDIO");
    vars.insert("BACKLOT_ROLE".into(), "hollywood".into());
    Env { vars }
}

fn user_only_env(env: &Env) -> Env {
    let mut vars = env.vars.clone();
    vars.remove("BACKLOT_ROLE");
    vars.remove("BACKLOT_STUDIO");
    Env { vars }
}

fn find_notes_md(base: &Path) -> PathBuf {
    let dir = base.join("repo-notes");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    assert_eq!(files.len(), 1, "expected 1 notes file in {}", dir.display());
    files.pop().unwrap()
}

#[test]
fn hollywood_propose_repo_note_applies_0600_idempotently_and_keeps_two_ids_independent() {
    let (scratch, env, repo, _sdir) = setup_repo_studio();
    let hdir = store::hollywood_dir(&scratch.dir);
    let repo_str = repo.canonicalize().unwrap().to_string_lossy().into_owned();
    let hw_env = hwood_only_env(&env);
    let u_env = user_only_env(&env);

    // Propose two distinct entry IDs for the same repo.
    let out1 = ok_args(
        &hw_env,
        &[
            "hollywood",
            "propose",
            "repo-note",
            "--repo",
            &repo_str,
            "--id",
            "build-cmd",
            "--text",
            "Run cargo nextest -p acme for fast inner-loop checks.",
            "--why",
            "avoid slow full builds",
        ],
    );
    assert!(out1.contains("card.proposed c1"), "{out1}");

    let out2 = ok_args(
        &hw_env,
        &[
            "hollywood",
            "propose",
            "repo-note",
            "--repo",
            &repo_str,
            "--id",
            "db-schema",
            "--text",
            "JSONB columns hold sparse tenant settings.",
            "--why",
            "prevent duplicate migration",
        ],
    );
    assert!(out2.contains("card.proposed c2"), "{out2}");

    // Two distinct entry_ids stay both Open (neither supersedes the other).
    let hs = store::load_hstate(&hdir).unwrap();
    assert_eq!(hs.open_cards().count(), 2);

    // Propose a second card on `build-cmd` -> supersedes open c1!
    ok_args(
        &hw_env,
        &[
            "hollywood",
            "propose",
            "repo-note",
            "--repo",
            &repo_str,
            "--id",
            "build-cmd",
            "--text",
            "Run scripts/farm-test.sh instead of local cargo.",
            "--why",
            "local box has no build cache",
        ],
    );
    let hs = store::load_hstate(&hdir).unwrap();
    assert_eq!(hs.cards[&1].state, CardState::Superseded);
    assert_eq!(hs.open_cards().count(), 2);

    // Apply both open cards (`c2` and `c3`) and run `Stage::hcards`.
    ok_args(&u_env, &["card", "apply", "c2"]);
    ok_args(&u_env, &["card", "apply", "c3"]);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let stage = rt.block_on(dummy_stage(&scratch.dir));
    stage.hcards(&hdir).unwrap();

    let hs = store::load_hstate(&hdir).unwrap();
    assert_eq!(hs.cards[&2].state, CardState::Done);
    assert_eq!(hs.cards[&3].state, CardState::Done);

    // Verify 0600 file in <state_dir>/repo-notes/<hash>.md and NOTHING inside repo.
    let note_file = find_notes_md(&scratch.dir);
    let mode = fs::metadata(&note_file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "repo-notes file must be mode 0600");
    let body = fs::read_to_string(&note_file).unwrap();
    assert!(body.contains("## db-schema"), "{body}");
    assert!(body.contains("<!-- card c2 -->"), "{body}");
    assert!(
        body.contains("JSONB columns hold sparse tenant settings."),
        "{body}"
    );
    assert!(body.contains("## build-cmd"), "{body}");
    assert!(body.contains("<!-- card c3 -->"), "{body}");
    assert!(body.contains("scripts/farm-test.sh"), "{body}");
    assert!(!repo.join("repo-notes").exists());
    assert!(git(&repo, &["status", "--short"]).is_empty());

    // Idempotency: running `hcards` again changes nothing.
    let mtime_before = fs::metadata(&note_file).unwrap().modified().unwrap();
    stage.hcards(&hdir).unwrap();
    let mtime_after = fs::metadata(&note_file).unwrap().modified().unwrap();
    assert_eq!(mtime_before, mtime_after);

    // Proposing and applying a second card against an outdated base fails `card apply` or stales.
    let stale_out = ok_args(
        &hw_env,
        &[
            "hollywood",
            "propose",
            "repo-note",
            "--repo",
            &repo_str,
            "--id",
            "db-schema",
            "--text",
            "First update.",
            "--why",
            "base 1",
        ],
    );
    assert!(stale_out.contains("card.proposed c"), "{stale_out}");
    let cid = stale_out
        .split_whitespace()
        .find(|w| w.starts_with('c') && w[1..].bytes().all(|b| b.is_ascii_digit()))
        .unwrap();
    // Mutate note via remove then attempt apply -> card is stale!
    ok_args(
        &u_env,
        &[
            "repo-note",
            "remove",
            "--id",
            "db-schema",
            "--repo",
            &repo_str,
        ],
    );
    let apply_out = ok_args(&u_env, &["card", "apply", cid]);
    assert!(
        apply_out.contains("card.stale") && apply_out.contains("changed"),
        "{apply_out}"
    );
}

#[test]
fn repo_note_rejects_secrets_and_dsns_at_propose_time() {
    let (scratch, env, repo, sdir) = setup_repo_studio();
    let hdir = store::hollywood_dir(&scratch.dir);
    let hw_env = hwood_only_env(&env);
    let repo_str = repo.to_string_lossy().into_owned();

    let bad_inputs = [
        "postgres://admin:s3cretPass@db.internal:5432/prod",
        "Use ANTHROPIC_API_KEY=sk-ant-api03-abcdef1234567890abcdef",
        "Token is ghp_1234567890abcdefghijklmnopqrstuvwxyz",
        "-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA",
        "password=super_secret_value",
    ];

    for bad in bad_inputs {
        let err1 = run_args(
            &hw_env,
            &[
                "hollywood",
                "propose",
                "repo-note",
                "--repo",
                &repo_str,
                "--id",
                "bad-secret",
                "--text",
                bad,
                "--why",
                "test",
            ],
        )
        .unwrap_err();
        assert!(
            matches!(err1, Fail::Refused(_)),
            "hw err={err1:?} for {bad}"
        );

        let err2 = run_args(
            &env,
            &[
                "repo-note",
                "propose",
                "--as",
                "director",
                "--id",
                "bad-secret",
                "--text",
                bad,
                "--why",
                "test",
            ],
        )
        .unwrap_err();
        assert!(
            matches!(err2, Fail::Refused(_)),
            "studio err={err2:?} for {bad}"
        );
    }

    assert!(!hdir.join("events.jsonl").exists());
    let (events, _) = store::load_events(&sdir).unwrap();
    assert!(
        !events.iter().any(|e| matches!(
            &e.kind,
            EventKind::MessageQueued { purpose: Some(p), .. } if p == "repo_note"
        )),
        "refused secret must leave zero repo_note messages"
    );
}

#[test]
fn studio_repo_note_propose_bridges_skips_outbox_and_surfaces_in_summary_without_hollywood() {
    let (scratch, env, _repo, sdir) = setup_repo_studio();
    let hdir = store::hollywood_dir(&scratch.dir);

    let out = ok_args(
        &env,
        &[
            "repo-note",
            "propose",
            "--as",
            "director",
            "--id",
            "ci-lints",
            "--text",
            "Always run scripts/lint.sh before finishing.",
            "--why",
            "shared studio discipline",
        ],
    );
    assert!(out.contains("message.queued"), "{out}");

    // Outbox skips internal `repo_note` bridge requests so producer isn't woken.
    let sstate = store::load_state(&sdir).unwrap();
    assert!(
        outbox(&sstate, &sdir).is_empty(),
        "{:?}",
        outbox(&sstate, &sdir)
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let stage = rt.block_on(dummy_stage(&scratch.dir));
    rt.block_on(stage.hollywood(&[], &[], &mut true, 1000))
        .unwrap();
    rt.block_on(stage.hollywood(&[], &[], &mut true, 2000))
        .unwrap();

    // Bridged exactly once by `stage` into hollywood/events.jsonl!
    let (hev, _) = store::load_hevents(&hdir).unwrap();
    assert_eq!(hev.len(), 1, "{hev:?}");
    assert_eq!(hev[0].by.role, "stage");

    // Even without [hollywood] config, CardSummary appears in Summary!
    let hs = store::load_hstate(&hdir).unwrap();
    let hw_sec = backlot_model::summary::hollywood_section_with_activity(
        &hs, "down", None, None, None, None, None, None,
    );
    assert_eq!(hw_sec.cards.len(), 1);
    assert_eq!(hw_sec.cards[0].studio, "acme-shop");
    assert!(
        hw_sec.cards[0].action.contains("ci-lints"),
        "{:?}",
        hw_sec.cards[0]
    );
    let card_seq = hev[0].seq;
    let _ = card_seq;
}

#[test]
fn repo_note_show_remove_user_only_and_injects_only_when_present_for_director_and_writer() {
    let (scratch, env, repo, sdir) = setup_repo_studio();
    let hdir = store::hollywood_dir(&scratch.dir);
    let repo_str = repo.canonicalize().unwrap().to_string_lossy().into_owned();
    let hw_env = hwood_only_env(&env);
    let u_env = user_only_env(&env);

    // Before notes exist: director and writer prompts mention NO repo notes file.
    let sstate = store::load_state(&sdir).unwrap();
    let start_for = |kind: RoleKind, role: &str, task: Option<&str>| Start {
        role: role.into(),
        kind,
        task: task.map(String::from),
        terminal: format!("bl-acme-shop-{role}"),
        actor: ActorIdentity {
            runner: "claude".into(),
            runner_version: None,
            model: "sonnet-5".into(),
            effort: None,
            permission_profile: "standard".into(),
        },
        repo: repo_str.clone(),
        members: Vec::new(),
        no_git: false,
        resume: None,
    };

    let dir_before = fs::read_to_string(
        prompts::prepare(
            &sdir,
            "acme-shop",
            &start_for(RoleKind::Director, "director", None),
            &sstate,
        )
        .unwrap(),
    )
    .unwrap();
    assert!(!dir_before.contains("repo-notes"), "{dir_before}");

    // Create note via card propose + apply + hcards.
    ok_args(
        &hw_env,
        &[
            "hollywood",
            "propose",
            "repo-note",
            "--repo",
            &repo_str,
            "--id",
            "dev-setup",
            "--text",
            "Postgres runs on port 55442 in tests.",
            "--why",
            "save discovery time",
        ],
    );
    ok_args(&u_env, &["card", "apply", "c1"]);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(dummy_stage(&scratch.dir))
        .hcards(&hdir)
        .unwrap();

    // User can show; roles are refused for both `show` and `remove`.
    let shown = ok_args(&u_env, &["repo-note", "show", "--repo", &repo_str]);
    assert!(shown.contains("Postgres runs on port 55442"), "{shown}");
    for role in ["director", "producer", "writer", "actor-t1-c1", "hollywood"] {
        let mut rvars = env.vars.clone();
        rvars.insert("BACKLOT_ROLE".into(), role.into());
        let renv = Env { vars: rvars };
        assert!(matches!(
            run_args(&renv, &["repo-note", "show", "--repo", &repo_str]),
            Err(Fail::Refused(_))
        ));
        assert!(matches!(
            run_args(
                &renv,
                &[
                    "repo-note",
                    "remove",
                    "--id",
                    "dev-setup",
                    "--repo",
                    &repo_str
                ]
            ),
            Err(Fail::Refused(_))
        ));
    }

    // Director & Writer now receive the read-only notes file path; Actor does NOT.
    let note_file = find_notes_md(&scratch.dir);
    let note_str = note_file.to_string_lossy().into_owned();
    let dir_after = fs::read_to_string(
        prompts::prepare(
            &sdir,
            "acme-shop",
            &start_for(RoleKind::Director, "director", None),
            &sstate,
        )
        .unwrap(),
    )
    .unwrap();
    let wri_after = fs::read_to_string(
        prompts::prepare(
            &sdir,
            "acme-shop",
            &start_for(RoleKind::Writer, "writer", None),
            &sstate,
        )
        .unwrap(),
    )
    .unwrap();
    let act_after = fs::read_to_string(
        prompts::prepare(
            &sdir,
            "acme-shop",
            &start_for(RoleKind::Actor, "actor-t1-c1", Some("t1")),
            &sstate,
        )
        .unwrap(),
    )
    .unwrap();
    assert!(dir_after.contains(&note_str), "{dir_after}");
    assert!(wri_after.contains(&note_str), "{wri_after}");
    assert!(!act_after.contains(&note_str), "{act_after}");

    // Runner allowlist: Claude `Read(//...)` and cmd `--add-dir` / `additional_dirs` carry notes path.
    let sdir_str = sdir.to_string_lossy().into_owned();
    let stage_env = std::collections::BTreeMap::new();
    let launch_ctx = crate::runners::LaunchCtx {
        terminal: "bl-acme-shop-director",
        cwd: &repo_str,
        prompt: Some("Read prompt"),
        resume: None,
        studio_dir: &sdir_str,
        stage_env: &stage_env,
        role: "director",
        checks: &[],
        answer_trust: false,
    };
    let claude_id = ActorIdentity {
        runner: "claude".into(),
        runner_version: None,
        model: "sonnet-5".into(),
        effort: None,
        permission_profile: "standard".into(),
    };
    let claude_launch =
        crate::runners::plan_with_notes(&claude_id, &launch_ctx, Some(&note_str)).unwrap();
    let stripped_note = note_str.strip_prefix('/').unwrap_or(&note_str);
    assert!(
        claude_launch
            .argv
            .iter()
            .any(|a| a.contains(&format!("Read(//{stripped_note})"))),
        "{:?}",
        claude_launch.argv
    );
    let cmd_id = ActorIdentity {
        runner: "cmd".into(),
        ..claude_id
    };
    let cmd_launch =
        crate::runners::plan_with_notes(&cmd_id, &launch_ctx, Some(&note_str)).unwrap();
    let notes_dir_str = scratch
        .dir
        .join("repo-notes")
        .to_string_lossy()
        .into_owned();
    assert!(
        cmd_launch.argv.contains(&notes_dir_str),
        "{:?}",
        cmd_launch.argv
    );

    // Remove deletes the section (and removes empty file cleanly).
    let rem = ok_args(
        &u_env,
        &[
            "repo-note",
            "remove",
            "--id",
            "dev-setup",
            "--repo",
            &repo_str,
        ],
    );
    assert!(rem.contains("dev-setup"), "{rem}");
    assert!(!note_file.exists());
}
