//! Self-setup stage execution, takeover prompt, and CLI-ref tests.

use std::fs;
use std::path::PathBuf;

use backlot_model::EventBy;
use backlot_model::hollywood::{
    HOLLYWOOD_TERMINAL, HollywoodEvent, HollywoodKind, RoleMotion, replay,
};
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::config::{HollywoodCfg, config_path};
use crate::hstage::{
    HMove, hfirst_prompt_recast, hplan, hprepare_launch_recast, hrecord_as, identity,
};
use crate::hwood_tests::argv_ok;
use crate::reconcile::LiveTerminal;
use crate::store;

fn cfg_env(env: &Env) -> Env {
    let mut vars = env.vars.clone();
    vars.insert(
        "XDG_CONFIG_HOME".into(),
        format!("{}/config-home", env.get("BACKLOT_STATE_DIR").unwrap()),
    );
    Env { vars }
}

fn cfg_path(env: &Env) -> PathBuf {
    config_path(&env.vars)
}

fn hevents(hdir: &std::path::Path) -> Vec<HollywoodEvent> {
    store::load_hevents(hdir).unwrap().0
}

fn read_start(path: &std::path::Path) -> Option<String> {
    let text = fs::read_to_string(path).unwrap();
    let val: toml::Value = toml::from_str(&text).unwrap();
    val.get("hollywood")?
        .get("start")?
        .as_str()
        .map(str::to_string)
}

#[test]
fn first_prompt_carries_recast_takeover_for_inline_and_devin() {
    let tmp = crate::test_support::TestDir::new("bl-hw-recast-prompt");
    let hdir = tmp.path().join("hollywood");
    let old = identity(&HollywoodCfg {
        runner: "cmd".into(),
        model: "deepseek-v4.1-flash".into(),
        effort: None,
        key: None,
        default_cast: None,
    });
    let new = identity(&HollywoodCfg {
        runner: "claude".into(),
        model: "claude-opus-5-5".into(),
        effort: Some("xhigh".into()),
        key: None,
        default_cast: None,
    });

    let inline = hfirst_prompt_recast(&hdir, "claude", None, Some((&old, &new))).unwrap();
    assert!(inline.contains("## Taking over Hollywood"), "{inline}");
    assert!(
        inline
            .contains("re-cast from cmd / deepseek-v4.1-flash → claude / claude-opus-5-5 / xhigh"),
        "{inline}"
    );
    assert!(inline.contains("backlot hollywood report"), "{inline}");

    // Devin puts takeover inside prompts/hollywood.first.txt and passes the file path.
    let devin_new = identity(&HollywoodCfg {
        runner: "devin".into(),
        model: "m2".into(),
        effort: None,
        key: None,
        default_cast: None,
    });
    hprepare_launch_recast(&hdir, "devin", None, Some((&old, &devin_new))).unwrap();
    let devin_prompt =
        hfirst_prompt_recast(&hdir, "devin", None, Some((&old, &devin_new))).unwrap();
    assert!(devin_prompt.ends_with("hollywood.first.txt"));
    let body = fs::read_to_string(hdir.join("prompts/hollywood.first.txt")).unwrap();
    assert!(body.contains("Taking over Hollywood"), "{body}");
    assert!(
        body.contains("cmd / deepseek-v4.1-flash → devin / m2"),
        "{body}"
    );

    // Resume passes None.
    assert!(hfirst_prompt_recast(&hdir, "claude", Some("sess-1"), Some((&old, &new))).is_none());
}

#[test]
fn applied_setup_card_updates_config_records_done_and_restarts_once() {
    let (scratch, env) = scratch();
    let mut env = cfg_env(&env);
    let stub = scratch.files.join("claude");
    fs::write(&stub, "#!/bin/sh\necho 'claude 1.0'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    }
    env.vars
        .insert("PATH".into(), scratch.files.to_string_lossy().into_owned());
    let path = cfg_path(&env);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        concat!(
            "[hollywood]\n",
            "start = \"when_waiting\"\n",
            "runner = \"fake\"\n",
            "model = \"model-a\"\n",
            "effort = \"low\"\n",
            "key = \"alt+h\"\n",
        ),
    )
    .unwrap();

    let hdir = store::hollywood_dir(&scratch.dir);
    let old_actor = identity(&crate::config::hollywood(&env.vars).unwrap().unwrap());
    hrecord_as(
        &hdir,
        EventBy::role("stage"),
        HollywoodKind::RoleState {
            state: RoleMotion::Started,
            terminal: Some(HOLLYWOOD_TERMINAL.into()),
            actor: Some(old_actor),
            session_id: Some("sess-old".into()),
            reason: None,
        },
    )
    .unwrap();

    argv_ok(
        &env,
        &[
            "hollywood",
            "propose",
            "setup",
            "--runner",
            "claude",
            "--model",
            "claude-opus-5-5",
            "--effort",
            "xhigh",
            "--why",
            "upgrade model",
            "--as",
            "hollywood",
        ],
    );
    argv_ok(&env, &["card", "apply", "2", "--as", "user"]);

    crate::link::runtime().unwrap().block_on(async {
        let (client_side, server_side) = tokio::io::duplex(65_536);
        let launch_prompts = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
        let launch_prompts_srv = launch_prompts.clone();
        let removed = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let removed_srv = removed.clone();

        tokio::spawn(async move {
            let (r, mut w) = tokio::io::split(server_side);
            let mut reader = Reader::new(r);
            let hello = reader.read_line().await.unwrap().unwrap();
            let req: Value = serde_json::from_slice(&hello).unwrap();
            assert_eq!(req["method"], "hello");
            w.write_all(
                serde_json::to_string(&json!({
                    "jsonrpc": "2.0", "id": 0,
                    "result": {
                        "generation": 1, "version": "fake-core 0.0",
                        "framed": true,
                        "core": {"id": "core-fake-1", "name": "fake-core"},
                        "methods": ["terminal.run", "terminal.kill", "terminal.read"],
                        "features": [],
                    }
                }))
                .unwrap()
                .as_bytes(),
            )
            .await
            .unwrap();
            w.write_all(b"\n").await.unwrap();

            let term_json = |addr: &str| {
                json!({
                    "address": addr,
                    "name": HOLLYWOOD_TERMINAL,
                    "labels": {},
                    "argv": ["claude"],
                    "cwd": "/home/user",
                    "state": "live",
                    "created_ms": 1700000000000u64,
                    "screen": "exact",
                    "cols": 200,
                    "rows": 120,
                })
            };

            while let Ok(Some(frame)) = reader.read_frame().await {
                if frame.channel != 0 || frame.kind != Kind::Data {
                    continue;
                }
                let req: Value = serde_json::from_slice(&frame.payload).unwrap();
                let result = match req["method"].as_str().unwrap_or("") {
                    "terminal.kill" => {
                        removed_srv
                            .lock()
                            .unwrap()
                            .push(req["params"]["terminal"].as_str().unwrap().into());
                        term_json("t-old")
                    }
                    "terminal.run" => {
                        let argv: Vec<String> =
                            serde_json::from_value(req["params"]["argv"].clone()).unwrap();
                        launch_prompts_srv.lock().unwrap().push(argv);
                        term_json("t-new")
                    }
                    "terminal.read" => json!({ "text": "> ", "complete": true, "how": "screen" }),
                    _ => json!({}),
                };
                let reply = serde_json::to_vec(&json!({
                    "jsonrpc": "2.0",
                    "id": req["id"],
                    "result": result
                }))
                .unwrap();
                w.write_all(&Frame::data(0, reply).encode()).await.unwrap();
            }
        });

        let (read, write) = tokio::io::split(client_side);
        let client = hc_proto::client::Client::open(Box::pin(read), Box::pin(write), None)
            .await
            .unwrap();

        let mut stage = crate::daemon::Stage::for_test(client);
        stage.base = scratch.dir.path().to_path_buf();
        stage.env_vars = env.vars.clone();

        let live_old = vec![LiveTerminal {
            name: HOLLYWOOD_TERMINAL.into(),
            address: "t-old".into(),
            ..Default::default()
        }];
        let mut free = true;

        stage
            .hollywood(&live_old, &[], &mut free, 1_000)
            .await
            .unwrap();

        assert_eq!(read_start(&path).as_deref(), Some("when_waiting"));
        let new_cfg = crate::config::hollywood(&env.vars).unwrap().unwrap();
        assert_eq!(new_cfg.runner, "claude");
        assert_eq!(new_cfg.model, "claude-opus-5-5");
        assert_eq!(new_cfg.effort.as_deref(), Some("xhigh"));
        assert_eq!(new_cfg.key.as_deref(), Some("alt+h"));

        let evs = hevents(&hdir);
        let done_pos = evs
            .iter()
            .position(|e| matches!(e.kind, HollywoodKind::CardDone { .. }))
            .expect("card.done recorded");
        let (done_card, done_note) = match &evs[done_pos].kind {
            HollywoodKind::CardDone { card, note } => (*card, note.clone()),
            _ => unreachable!(),
        };
        assert_eq!(
            done_note,
            "Hollywood re-cast to claude / claude-opus-5-5 / xhigh"
        );
        let restart_pos = evs
            .iter()
            .position(|e| matches!(e.kind, HollywoodKind::RoleRestart { fresh: true, .. }))
            .expect("role.restart recorded");
        assert!(
            done_pos < restart_pos,
            "card.done must precede role.restart"
        );

        stage
            .hollywood(&live_old, &[], &mut free, 2_000)
            .await
            .unwrap();
        assert_eq!(*removed.lock().unwrap(), vec!["t-old".to_string()]);

        stage.hollywood(&[], &[], &mut free, 3_000).await.unwrap();
        {
            let launches = launch_prompts.lock().unwrap();
            assert_eq!(
                launches.len(),
                1,
                "expected exactly one new launch: {launches:?}"
            );
            let prompt_joined = launches[0].join(" ");
            assert!(
                prompt_joined.contains("Taking over Hollywood"),
                "{prompt_joined}"
            );
            assert!(
                prompt_joined.contains("fake / model-a / low → claude / claude-opus-5-5 / xhigh"),
                "{prompt_joined}"
            );
        }

        let live_new = vec![LiveTerminal {
            name: HOLLYWOOD_TERMINAL.into(),
            address: "t-new".into(),
            ..Default::default()
        }];
        stage
            .hollywood(&live_new, &[], &mut free, 4_000)
            .await
            .unwrap();
        assert_eq!(launch_prompts.lock().unwrap().len(), 1);

        let replayed = replay(&hevents(&hdir));
        assert!(replayed.role.restart.is_none());
        assert_eq!(
            hplan(Some(&new_cfg), &replayed.role, true, 5_000, 0),
            Vec::<HMove>::new()
        );
        let passive = crate::hnotice::passive_items(
            &std::collections::BTreeMap::new(),
            &replayed,
            &std::collections::BTreeSet::new(),
        );
        let expected_passive =
            format!("c{done_card} done — Hollywood re-cast to claude / claude-opus-5-5 / xhigh");
        assert!(
            passive.iter().any(|p| p.text == expected_passive),
            "expected passive card.done note '{expected_passive}' in {passive:?}"
        );
        let person_msg = backlot_model::hollywood::HMessage {
            seq: 99,
            ts_ms: 5_000,
            from: "user".into(),
            to: "hollywood".into(),
            studio: None,
            text: Some("hello".into()),
            file: None,
            source_seq: None,
            delivered: false,
            dropped: false,
            seen: false,
        };
        let payload =
            crate::hdeliver_fmt::person_payload(&[&person_msg], &[], &passive, &hdir).unwrap();
        assert!(
            payload.contains(&format!("(context: {expected_passive})")),
            "{payload}"
        );
    });
}

#[test]
fn prompt_and_cli_reference_cover_propose_setup_and_report_rule() {
    let prompt = crate::prompts::HOLLYWOOD;
    assert!(prompt.contains("propose setup"), "{prompt}");
    assert!(prompt.contains("backlot hollywood report"), "{prompt}");
    assert!(
        prompt.contains("you need to run X"),
        "expected report rule in Turn discipline: {prompt}"
    );
    let cli_ref = include_str!("../prompts/hollywood-cli.md");
    assert!(cli_ref.contains("propose setup"), "{cli_ref}");
}
