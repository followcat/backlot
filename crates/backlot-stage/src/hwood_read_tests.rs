//! The cross-studio reads (R24): `status --all`,
//! `records --studio/--all` — every studio in one view, then
//! Hollywood's block. Shared fixtures come from `hwood_tests`.

use crate::cli_testkit::*;
use crate::hwood_tests::{argv_ok, hwood_env, running};

#[test]
fn status_all_covers_the_lot() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    ok(
        &env,
        "hollywood propose pause --studio acme-shop --task t1 --why looping --as hollywood",
    );
    argv_ok(
        &env,
        &["hollywood", "report", "one open card", "--as", "hollywood"],
    );

    let out = ok(&env, "status --all");
    assert!(out.contains("acme-shop"), "{out}");
    assert!(out.contains("hollywood"), "{out}");
    assert!(out.contains("one open card"), "{out}");
    assert!(out.contains("c1"), "{out}");

    let json = ok(&env, "status --all --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["studios"]["acme-shop"].is_object(), "{json}");
    assert_eq!(v["hollywood"]["headline"], "one open card");
    // A studio pin works the same everywhere — Hollywood's own
    // shell carries no BACKLOT_STUDIO, so `--studio` picks freely.
    let lenv = hwood_env(&env);
    let one = ok(&lenv, "status --studio acme-shop");
    assert!(one.contains("acme-shop"), "{one}");
    let msg = refused(&lenv, "status --studio no-shop");
    assert!(msg.contains("no studio 'no-shop'"), "{msg}");
    assert!(usage(&lenv, "status --all --studio acme-shop").contains("--all"));
}

#[test]
fn records_scope_to_one_studio_or_all() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    // A delivery gives records something to count.
    let work = work_area(&scratch, "t1");
    let a = commit_in(&work, "one");
    let b = commit_in(&work, "two");
    let rep = fixture(&scratch, "rep.md", "report\n");
    ok(
        &env,
        &format!(
            "deliver --commits {a}..{b} --report {rep} --check tests=pass --as actor-t1-c1 --task t1"
        ),
    );
    // `--studio` scopes the read; `--all` covers the lot. actor-a's
    // runner name is the record's key either way.
    let lenv = hwood_env(&env);
    let out = ok(&lenv, "records --studio acme-shop");
    assert!(out.contains("runner-a"), "{out}");
    let out = ok(&lenv, "records --all");
    assert!(out.contains("runner-a"), "{out}");
    assert!(usage(&lenv, "records --all --studio acme-shop").contains("--all"));
    let msg = refused(&lenv, "records --studio no-shop");
    assert!(msg.contains("no studio 'no-shop'"), "{msg}");
}

#[test]
fn status_all_renders_accounts_block_json_fallback_and_hides_secrets() {
    use backlot_model::event::{EventKind, RoleConfigs, StudioMode};
    use backlot_model::{EventBy, NewEvent};

    let (scratch, mut env) = scratch();
    running(&scratch, &env);

    // Put a second studio (`acme-web`) + Hollywood config + secret env token in config.toml.
    let base = std::path::PathBuf::from(env.get("BACKLOT_STATE_DIR").unwrap());
    let secret = "sk-secret-status-cli-do-not-leak";
    std::fs::write(
        base.join("config.toml"),
        format!("[runner_env.claude]\nANTHROPIC_API_KEY = \"{secret}\"\n"),
    )
    .unwrap();
    env.vars.insert(
        "HOME".into(),
        scratch.dir.path().to_string_lossy().into_owned(),
    );
    let cfg_home = scratch.dir.path().join(".config");
    std::fs::create_dir_all(cfg_home.join("backlot")).unwrap();
    std::fs::write(
        cfg_home.join("backlot/config.toml"),
        "[hollywood]\nrunner = \"claude\"\nmodel = \"opus-5-5\"\neffort = \"high\"\n",
    )
    .unwrap();
    env.vars.insert(
        "XDG_CONFIG_HOME".into(),
        cfg_home.to_string_lossy().into_owned(),
    );

    // Recast/create a second studio on claude and raise a `rate limited` attention.
    let web_dir = crate::store::studio_dir(&base, "acme-web");
    std::fs::create_dir_all(&web_dir).unwrap();
    let claude_actor = backlot_model::event::ActorIdentity {
        runner: "claude".into(),
        runner_version: None,
        model: "sonnet-5".into(),
        effort: None,
        permission_profile: "standard".into(),
    };
    crate::store::append(&web_dir, |_, _| {
        Ok::<_, std::convert::Infallible>(crate::store::Pending {
            event: NewEvent::new(
                EventBy::role("user"),
                EventKind::StudioCreated {
                    title: "acme-web".into(),
                    repo: "/repo-web".into(),
                    members: Vec::new(),
                    no_git: true,
                    mode: StudioMode::Standard,
                    roles: Box::new(RoleConfigs {
                        producer: claude_actor.clone(),
                        director: claude_actor.clone(),
                        writer: claude_actor.clone(),
                    }),
                    candidates: Vec::new(),
                    rework_cap: 2,
                    stall_limit_s: 600,
                    nudge_after_s: 180,
                    escalate_after_s: 600,
                    check_env: Vec::new(),
                    checks: Vec::new(),
                    task_budget_s: None,
                    limits: Default::default(),
                    max_actors: backlot_model::event::default_max_actors(),
                    push: Default::default(),
                    design_gate: backlot_model::event::DesignGate::Off,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let future_reset = crate::live::now_ms() + 3 * 3_600_000;
    crate::store::append(&web_dir, |_, _| {
        Ok::<_, std::convert::Infallible>(crate::store::Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::AttentionRaised {
                    role: "director".into(),
                    reason: crate::ratelimit::RATE_LIMITED.into(),
                    detail: "waiting until reset".into(),
                    needs_user: false,
                    account: Some("claude:env-token".into()),
                    reset_at_ms: Some(future_reset),
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    // No `live.json` exists -> CLI derives accounts locally from ledgers + config!
    let text = ok(&env, "status --all");
    assert!(text.contains("accounts:"), "{text}");
    assert!(text.contains("claude:env-token"), "{text}");
    assert!(text.contains("STALLED"), "{text}");
    assert!(text.contains("hollywood"), "{text}");
    assert!(text.contains("acme-web/director"), "{text}");
    assert!(
        !text.contains(secret),
        "secret leaked in CLI status output: {text}"
    );
    let sec_hash = backlot_model::hollywood::sha256_hex(secret.as_bytes());
    assert!(!text.contains(&sec_hash[..6]), "secret hash leaked: {text}");

    let json = ok(&env, "status --all --json");
    assert!(!json.contains(secret), "secret leaked in JSON: {json}");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let accts = v["accounts"].as_array().expect("top-level accounts array");
    let claude = accts
        .iter()
        .find(|a| a["id"] == "claude:env-token")
        .expect("claude:env-token in accounts JSON");
    assert_eq!(claude["stalled"], true);
    assert_eq!(claude["reset_at_ms"], future_reset);

    // Single-studio `backlot status` also falls back cleanly and shows cross-studio badge & footer.
    let lenv = hwood_env(&env);
    let single = ok(&lenv, "status --studio acme-web");
    assert!(single.contains("[claude:env-token]"), "{single}");
    assert!(single.contains("accounts:"), "{single}");
    assert!(single.contains("elsewhere (hollywood)"), "{single}");
    assert!(
        !single.contains(secret),
        "secret leaked in single status: {single}"
    );
}
