//! Fail-first and regression tests for field-notes-1127 fixes.

use std::fs;

use crate::cli::Fail;
use crate::cli_testkit::{CAST_TOML, fixture, ok, refused, scratch, usage};
use crate::store;

fn args(s: &str) -> Vec<String> {
    s.split(' ').map(String::from).collect()
}

fn setup_shop_cast_fake() -> (crate::cli_testkit::Scratch, crate::cli::Env) {
    let (scratch, env) = scratch();
    let cast_toml = CAST_TOML
        .replace("runner-p", "fake")
        .replace("runner-d", "fake")
        .replace("runner-w", "fake")
        .replace("runner-a", "fake")
        .replace("runner-b", "fake");
    let cast = fixture(&scratch, "cast.toml", &cast_toml);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    (scratch, env)
}

#[test]
fn producer_ask_is_refused_with_tell_user_hint() {
    let (scratch, env) = setup_shop_cast_fake();
    let q = fixture(&scratch, "q.md", "Which API version should we ship?\n");
    let msg = refused(&env, &format!("ask --file {q} --as producer"));
    assert!(
        msg.contains("the producer asks the person with: backlot tell user --file <file> --ask"),
        "expected producer tell-user hint, got: {msg}"
    );
    let (events, _) = store::load_events(&store::studio_dir(&scratch.dir, "acme-shop")).unwrap();
    assert!(
        !events.iter().any(|e| matches!(
            &e.kind,
            backlot_model::EventKind::MessageQueued { to, .. } if to == "producer"
        )),
        "producer must never self-queue via ask: {events:?}"
    );

    // Other roles still use `backlot ask` to reach the producer.
    let dq = fixture(&scratch, "dq.md", "Need clarification on target branch.\n");
    ok(&env, &format!("ask --file {dq} --as director"));
    let (events, _) = store::load_events(&store::studio_dir(&scratch.dir, "acme-shop")).unwrap();
    assert!(events.iter().any(|e| matches!(
        &e.kind,
        backlot_model::EventKind::MessageQueued { to, .. } if to == "producer"
    )));
}

#[test]
fn task_new_readonly_gives_cast_hint() {
    let (scratch, env) = setup_shop_cast_fake();
    let acc = fixture(&scratch, "acc.md", "# Acceptance\nAudit notes only.\n");
    let msg = usage(
        &env,
        &format!(
            "task new --title Audit --kind audit --acceptance-file {acc} --readonly --as director"
        ),
    );
    assert_eq!(
        msg.lines().next().unwrap_or(""),
        "--readonly is set when casting: backlot cast … --readonly (task new --kind audit makes the audit task)",
        "got usage error: {msg}"
    );
}

#[test]
fn cmd_effort_is_refused_across_cast_and_hollywood_paths() {
    let (scratch, env) = setup_shop_cast_fake();
    let bad_cast = CAST_TOML
        .replace("runner-p", "fake")
        .replace("runner-d", "fake")
        .replace("runner-w", "fake")
        .replace("runner-b", "fake")
        .replace(
            "runner = \"runner-a\"\nmodel = \"model-a\"\neffort = \"high\"",
            "runner = \"cmd\"\nmodel = \"deepseek/deepseek-v4.1-flash\"\neffort = \"high\"",
        );
    let bad_file = fixture(&scratch, "bad-cmd-effort.toml", &bad_cast);

    // 1. `studio new` rejects `cmd` with `effort`.
    let mut vars = env.vars.clone();
    vars.insert("BACKLOT_STUDIO".into(), "beta-api".into());
    let new_env = crate::cli::Env { vars };
    let err = usage(
        &new_env,
        &format!("studio new --title Beta --repo /home/user/beta-api --cast {bad_file}"),
    );
    assert!(
        err.contains("cmd has no effort setting; drop --effort"),
        "studio new: {err}"
    );

    // 2. `studio cast set` (both dry-run and real) rejects `cmd` with `effort`.
    for suffix in [" --dry-run", ""] {
        let err = usage(
            &env,
            &format!("studio cast set --studio acme-shop --file {bad_file}{suffix}"),
        );
        assert!(
            err.contains("cmd has no effort setting; drop --effort"),
            "studio cast set{suffix}: {err}"
        );
    }

    // 3. `hollywood propose cast` rejects `cmd` with `effort` before recording a card.
    let hw_draft = store::hollywood_dir(&scratch.dir).join("drafts/acme-shop/bad-cast.toml");
    fs::create_dir_all(hw_draft.parent().unwrap()).unwrap();
    fs::write(&hw_draft, &bad_cast).unwrap();
    let err = crate::cli::run(
        &args(&format!(
            "hollywood propose cast --studio acme-shop --file {} --why test --as hollywood",
            hw_draft.display()
        )),
        &env,
    )
    .unwrap_err();
    assert!(
        matches!(err, Fail::Usage(_) | Fail::Refused(_))
            && err
                .message()
                .contains("cmd has no effort setting; drop --effort"),
        "propose cast: {err:?}"
    );
    let list = ok(&env, "card list");
    assert!(!list.contains("cast acme-shop"), "{list}");

    // 4. `recast` and `hollywood propose recast` reject `--runner cmd --effort high`.
    // Provide a stub `cmd` executable on PATH so runner availability does not mask the effort check.
    let bin_dir = scratch.dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let stub = bin_dir.join("cmd");
    fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut path_vars = env.vars.clone();
    let orig_path = std::env::var("PATH").unwrap_or_default();
    path_vars.insert("PATH".into(), format!("{}:{orig_path}", bin_dir.display()));
    path_vars.insert(
        "XDG_CONFIG_HOME".into(),
        scratch.dir.join("xdg").to_string_lossy().into_owned(),
    );
    let path_env = crate::cli::Env { vars: path_vars };

    let err = refused(
        &path_env,
        "recast --studio acme-shop --person director --runner cmd --model deepseek/deepseek-v4.1-flash --effort high --reason test",
    );
    assert!(
        err.contains("cmd has no effort setting; drop --effort"),
        "recast: {err}"
    );
    let err = refused(
        &path_env,
        "hollywood propose recast --studio acme-shop --person director --runner cmd --model deepseek/deepseek-v4.1-flash --effort high --why test --as hollywood",
    );
    assert!(
        err.contains("cmd has no effort setting; drop --effort"),
        "propose recast: {err}"
    );
    let err = crate::cli::run(
        &args("hollywood setup --runner cmd --model deepseek/deepseek-v4.1-flash --effort high"),
        &path_env,
    )
    .unwrap_err();
    assert!(
        err.message()
            .contains("cmd has no effort setting; drop --effort"),
        "hollywood setup: {err:?}"
    );
}

#[test]
fn producer_prompt_covers_terminal_question_visibility_decide_and_model_ids() {
    let prod = crate::prompts::template(crate::reconcile::RoleKind::Producer);
    assert!(
        prod.contains("backlot tell user --file <file> --ask"),
        "{prod}"
    );
    let norm = prod.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        norm.contains("also print one short line in your terminal: the decision needed, plus the shortest command or answer the person can copy"),
        "{prod}"
    );
    assert!(
        prod.contains("backlot decide --id AD-<n> --file <file> --notify <roles>"),
        "{prod}"
    );
    assert!(
        norm.contains("terminal answer alone is not binding"),
        "{prod}"
    );
    assert!(
        prod.contains("backlot decide --id AD-1 --file /tmp/ad-1.md --notify writer,director"),
        "{prod}"
    );
    assert!(
        prod.contains("deepseek/deepseek-v4.1-flash") && prod.contains("takes no effort"),
        "{prod}"
    );
}

#[test]
fn director_and_hollywood_and_guide_prompts_cover_audit_and_model_ids() {
    let dir = crate::prompts::template(crate::reconcile::RoleKind::Director);
    assert!(
        dir.contains("high-risk structural or workflow refactors"),
        "{dir}"
    );
    assert!(
        dir.contains("Never merge an audit branch (`backlot/{studio}/<task>`) into"),
        "{dir}"
    );

    let hw = crate::prompts::HOLLYWOOD;
    let hw_norm = hw.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        hw_norm.contains("Write a model id exactly as that runner lists it. For example, Command Code's is `deepseek/deepseek-v4.1-flash`, and Command Code takes no effort."),
        "{hw}"
    );

    let guide = include_str!("../../../docs/studio/guide.md");
    assert!(
        guide.contains("Write a model id exactly as that runner lists it. For example, Command Code's is\n`deepseek/deepseek-v4.1-flash`, and Command Code takes no effort."),
        "guide missing model-id note"
    );
    assert!(
        !guide.contains("--model deepseek-v4.1-flash"),
        "guide still shows bare cmd model without provider prefix"
    );
}

#[test]
fn person_only_actions_sync_with_code_guards_and_render_into_prompts() {
    use crate::person_only::{ENTRIES, GuardKind, PERSON_ONLY_RULE, rendered_section};
    use crate::proc_guard::CliVerb;

    // 1. Every CliVerb guard maps to an entry in ENTRIES.
    let verbs = [
        CliVerb::CardMutate,
        CliVerb::PermsRevoke,
        CliVerb::ServerControl,
        CliVerb::HollywoodControl,
        CliVerb::DesignSign,
        CliVerb::StudioCheck,
        CliVerb::RepoNoteAdmin,
        CliVerb::Approve { person: "director" },
    ];
    for verb in verbs {
        let kind = GuardKind::from_cli_verb(verb);
        assert!(
            ENTRIES.iter().any(|e| e.kind == kind),
            "missing ENTRIES row for {verb:?}"
        );
    }

    // 2. Non-CliVerb person-only code guards are all represented in ENTRIES.
    for kind in [
        GuardKind::StudioCastAndRecast,
        GuardKind::StudioLifecycle,
        GuardKind::AskAcknowledge,
        GuardKind::ReviewMissingChecks,
        GuardKind::OutwardCardAndRefused,
    ] {
        assert!(
            ENTRIES.iter().any(|e| e.kind == kind),
            "missing ENTRIES row for {kind:?}"
        );
    }

    // 3. Every backticked `backlot ...` command in rendered_section(true) parses cleanly.
    let section = rendered_section(true);
    assert!(section.contains(PERSON_ONLY_RULE), "{section}");
    for span in section.split('`').skip(1).step_by(2) {
        let Some(cmd) = span.strip_prefix("backlot ") else {
            continue;
        };
        let words: Vec<String> = cmd
            .split_whitespace()
            .map(|t| {
                t.replace("<seq>", "1")
                    .replace("<n>", "1")
                    .replace("<r>", "1")
                    .replace("<file>", "/tmp/f")
            })
            .collect();
        assert!(
            crate::args::parse(&words).is_ok(),
            "person_only.rs: `backlot {cmd}` failed to parse: {:?}",
            crate::args::parse(&words).err()
        );
    }

    // 4. Rendered prompts for producer, director, and writer carry the section and rule;
    // when design_gate=Required, `backlot design sign` is included cleanly too.
    let (scratch, base_env) = setup_shop_cast_fake();
    let mut state_gate = store::load_state(&store::studio_dir(&scratch.dir, "acme-shop")).unwrap();
    state_gate.studio.as_mut().unwrap().design_gate = backlot_model::event::DesignGate::Required;
    let dir = crate::test_support::TestDir::new("backlot-person-only-prompts");
    for (kind, role) in [
        (crate::reconcile::RoleKind::Producer, "producer"),
        (crate::reconcile::RoleKind::Director, "director"),
        (crate::reconcile::RoleKind::Writer, "writer"),
    ] {
        let start = crate::reconcile::Start {
            role: role.into(),
            kind,
            task: None,
            terminal: format!("bl-acme-shop-{role}"),
            actor: backlot_model::ActorIdentity {
                runner: "fake".into(),
                runner_version: None,
                model: "m".into(),
                effort: Some("high".into()),
                permission_profile: "standard".into(),
            },
            repo: dir.to_string_lossy().into_owned(),
            members: Vec::new(),
            no_git: true,
            resume: None,
        };
        let path = crate::prompts::prepare(&dir, "acme-shop", &start, &state_gate).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(PERSON_ONLY_RULE), "{role}: {text}");
        assert!(text.contains("`backlot card apply c<n>`"), "{role}");
        assert!(text.contains("`backlot design sign`"), "{role}");
        assert!(text.contains("`backlot ask acknowledged"), "{role}");
        assert!(!text.contains("{person_only_actions}"), "{role}");
    }

    // 5. Verify a non-user caller really refuses each guarded CLI command family.
    let mut vars = base_env.vars.clone();
    vars.insert("BACKLOT_ROLE".into(), "producer".into());
    let prod_env = crate::cli::Env { vars };
    let cast_file = fixture(
        &scratch,
        "cast2.toml",
        &CAST_TOML.replace("runner-p", "fake"),
    );
    for cmd_line in [
        "card apply c1",
        "card cancel c1",
        &format!("studio cast set --file {cast_file}"),
        "recast --person director --runner fake --model m --effort high --reason x",
        "restart --person hollywood --reason x",
        "studio check add --name lint --run true",
        "studio check remove --name lint",
        "design sign",
        "studio finish --reason done",
        "studio reopen --reason more",
        "studio delete",
        "studio restore acme-shop-20261125T000000Z",
        "hollywood setup --runner fake --model m --effort high",
        "hollywood switch --status",
        "approve --person director --decision allow",
        "approve --person hollywood --decision allow",
        "perms revoke 1",
        "server start",
        "repo-note show",
        "repo-note remove --id r1",
        "ask acknowledged --person producer --target 1",
    ] {
        let err = refused(&prod_env, cmd_line);
        assert!(!err.is_empty(), "expected refusal on `{cmd_line}`");
    }
}
