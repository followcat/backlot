//! H3 — Hollywood oversees two studios on the fake runner. One
//! `bl-hollywood` terminal comes up with its own folder, labels and
//! env; each studio's notice reaches its inbox naming the studio;
//! `tell hollywood` is delivered; a proposed-then-applied card is
//! carried out (`card.done`, the studio shows the change); a card
//! whose base moves after the apply ends `card.stale` with no
//! studio event; a `studio-new` card makes a third studio whose
//! producer receives the brief; a stage restart adopts
//! `bl-hollywood` and repeats nothing; and the summary document
//! carries the `hollywood` object. The second test: no
//! `[hollywood]`, none of it exists. Ignored by default; run with
//! `HC_TUI_TEST_CORE` set.

#![cfg(unix)]

#[path = "common/real_kit.rs"]
mod kit;
#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use kit::*;

use scratch::ScratchCore;

use serde_json::Value;

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

/// The `[hollywood]` section of the scratch's user config — the
/// stage reads it every tick, so it lands before `stage start`.
const HOLLYWOOD_CFG: &str =
    "[hollywood]\nrunner = \"fake\"\nmodel = \"model-h\"\neffort = \"high\"\n";

fn write_config(scratch: &ScratchCore, text: &str) {
    let dir = scratch.root.join("config/backlot");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("config.toml"), text).unwrap();
}

/// Hollywood's own ledger lines, parsed — `<state>/hollywood/`.
fn hledger(state: &Path) -> Vec<Value> {
    fs::read_to_string(state.join("hollywood").join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("hollywood ledger line"))
        .collect()
}

/// Hollywood's ledger reaches `n` events of `kind`.
fn wait_hkind(state: &Path, kind: &str, n: usize) -> Vec<Value> {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let found: Vec<Value> = hledger(state)
            .into_iter()
            .filter(|e| e["kind"] == kind)
            .collect();
        if found.len() >= n {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "wanted {n} {kind} events in the hollywood ledger: {:?}",
            hledger(state)
        );
        std::thread::sleep(POLL);
    }
}

/// A `messages/*-<label>.md` file under Hollywood's dir containing
/// `needle` — "a notice from each studio naming it" checks the body.
fn wait_hmessage(state: &Path, label: &str, needle: &str) -> String {
    let dir = state.join("hollywood").join("messages");
    let suffix = format!("-{label}.md");
    let deadline = Instant::now() + DEADLINE;
    loop {
        if dir.is_dir() {
            for e in fs::read_dir(&dir).unwrap() {
                let p = e.unwrap().path();
                if p.file_name()
                    .is_some_and(|n| n.to_string_lossy().ends_with(&suffix))
                {
                    let text = fs::read_to_string(&p).unwrap_or_default();
                    if text.contains(needle) {
                        return text;
                    }
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "no messages/*{suffix} under {} containing {needle:?}",
            dir.display()
        );
        std::thread::sleep(POLL);
    }
}

/// The newest `card.proposed`'s seq — the `cN` the user applies.
fn last_card(state: &Path) -> u64 {
    hledger(state)
        .iter()
        .filter(|e| e["kind"] == "card.proposed")
        .filter_map(|e| e["seq"].as_u64())
        .max()
        .expect("no card.proposed in the hollywood ledger")
}

/// A minimal studio up to a running `t1` — no stage needed: the
/// commands are ledger writes only.
fn studio(scratch: &ScratchCore, state: &Path, name: &str) {
    let repo = scratch.work.join(format!("repo-{name}"));
    git_repo(&repo);
    let cast = fixture(scratch, &format!("cast-{name}.toml"), CAST_TOML);
    let req = fixture(scratch, &format!("req-{name}.md"), "# requirement\n");
    let acc = fixture(scratch, &format!("acc-{name}.md"), "# acceptance\n");
    bl(
        scratch,
        state,
        name,
        &format!(
            "studio new --title {name} --repo {} --cast {}",
            repo.display(),
            cast.display()
        ),
    );
    bl(
        scratch,
        state,
        name,
        &format!(
            "req revise --file {} --summary v1 --as producer",
            req.display()
        ),
    );
    bl(
        scratch,
        state,
        name,
        &format!(
            "task new --title cart --kind feature --acceptance-file {} --as director",
            acc.display()
        ),
    );
    bl(
        scratch,
        state,
        name,
        "cast --task t1 --actor actor-a --reason first --as director",
    );
}

/// `task.state` events on `task` in `studio`'s ledger, newest last.
fn task_states(state: &Path, studio: &str, task: &str) -> Vec<Value> {
    ledger(state, studio)
        .into_iter()
        .filter(|e| e["kind"] == "task.state" && e["task"] == task)
        .collect()
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_hollywood_oversees_two_studios() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    write_config(&scratch, HOLLYWOOD_CFG);

    studio(&scratch, &state, "acme-shop");
    studio(&scratch, &state, "acme");
    bl_unpinned(&scratch, &state, "stage start");

    // The one terminal, its labels, its folder and its env.
    let hw = wait_terminal(&client, "bl-hollywood").await;
    assert_eq!(
        hw.labels.get("backlot.role").map(String::as_str),
        Some("hollywood"),
        "{hw:?}"
    );
    assert_eq!(
        hw.labels.get("herdr.space").map(String::as_str),
        Some("backlot"),
        "{hw:?}"
    );
    assert!(
        !hw.labels.contains_key("backlot.studio"),
        "hollywood labels a studio: {hw:?}"
    );
    let hdir = state.join("hollywood");
    wait_path(&hdir.join("AGENTS.md"));
    wait_path(&hdir.join("events.jsonl"));
    assert!(hdir.join("memory.md").is_file(), "no memory.md made");
    send_text(
        &client,
        &hw.address,
        "sh:env | sort > \"$BACKLOT_STATE_DIR/hw-env.txt\"",
    )
    .await;
    wait_path(&state.join("hw-env.txt"));
    let env_text = fs::read_to_string(state.join("hw-env.txt")).unwrap();
    assert!(env_text.contains("BACKLOT_ROLE=hollywood"), "{env_text}");
    assert!(
        env_text.contains(&format!("BACKLOT_STATE_DIR={}", state.display())),
        "{env_text}"
    );
    assert!(!env_text.contains("BACKLOT_STUDIO="), "{env_text}");

    // Both studios come up; each producer's prompt is one notice to
    // Hollywood naming its studio, delivered into its inbox.
    for s in ["acme-shop", "acme"] {
        let p = wait_terminal(&client, &format!("bl-{s}-producer")).await;
        wait_read(&client, &p.address, &format!("of {s}")).await;
        send_text(&client, &p.address, "work:blocked").await;
        wait_event(&state, s, "attention.raised", "producer");
    }
    wait_hmessage(&state, "prompt", "acme-shop");
    wait_hmessage(&state, "prompt", "acme");
    wait_inbox(&state, "fake-inbox-hollywood", "-prompt.md");

    // `tell hollywood` is delivered and recorded delivered —
    // solo or inside a digest, the `message.delivered` targeting
    // its seq is the record (D20).
    bl_unpinned(&scratch, &state, "tell hollywood hello-overseer");
    let deadline = Instant::now() + DEADLINE;
    loop {
        let ev = hledger(&state);
        let tell = ev
            .iter()
            .find(|e| e["kind"] == "message.sent" && e["text"] == "hello-overseer");
        let done = tell.is_some_and(|t| {
            ev.iter()
                .any(|e| e["kind"] == "message.delivered" && e["target"] == t["seq"])
        });
        if done {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the tell was never recorded delivered: {ev:?}"
        );
        std::thread::sleep(POLL);
    }

    // The document's hollywood object: live, the configured runner.
    let deadline = Instant::now() + DEADLINE;
    loop {
        let (_, doc) = wait_doc(&client).await;
        if doc["hollywood"]["state"] == "live" {
            assert_eq!(doc["hollywood"]["runner"], "fake", "{doc}");
            assert_eq!(doc["hollywood"]["model"], "model-h", "{doc}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "doc never showed hollywood live: {doc}"
        );
        tokio::time::sleep(POLL).await;
    }

    // A card the test proposes as Hollywood and applies as the user
    // is carried out: `task.state` paused lands `by: user` in
    // acme-shop, `card.done` in Hollywood's ledger.
    bl_unpinned(
        &scratch,
        &state,
        "hollywood propose pause --studio acme-shop --task t1 \
         --why hold-the-cart --as hollywood",
    );
    bl_unpinned(
        &scratch,
        &state,
        &format!("card apply c{}", last_card(&state)),
    );
    wait_hkind(&state, "card.done", 1);
    wait_kind(&state, "acme-shop", "task.state", 1);
    let states = task_states(&state, "acme-shop", "t1");
    assert_eq!(states.len(), 1, "{states:?}");
    assert_eq!(states[0]["state"], "paused", "{states:?}");
    assert_eq!(states[0]["by"]["role"], "user", "{states:?}");
    assert!(
        states[0]["reason"].as_str().unwrap().starts_with("card c"),
        "{states:?}"
    );

    // The stage-only restart: `bl-hollywood` is adopted — same
    // terminal — and with the stage dead two pause cards on acme's
    // t1 apply while its base still says running. The first lands
    // `card.done`; the second's base moved at execution — `card.stale`
    // by the stage, no second task.state.
    let hw_address = hw.address.clone();
    kill_stage(&client).await;
    for why in ["first-hold", "second-hold"] {
        bl_unpinned(
            &scratch,
            &state,
            &format!("hollywood propose pause --studio acme --task t1 --why {why} --as hollywood"),
        );
        bl_unpinned(
            &scratch,
            &state,
            &format!("card apply c{}", last_card(&state)),
        );
    }
    bl_unpinned(&scratch, &state, "stage start");
    let hw2 = wait_terminal(&client, "bl-hollywood").await;
    assert_eq!(hw2.address, hw_address, "bl-hollywood was not adopted");
    wait_hkind(&state, "card.stale", 1);
    let stale = hledger(&state)
        .into_iter()
        .find(|e| e["kind"] == "card.stale")
        .expect("no card.stale");
    assert_eq!(stale["by"]["role"], "stage", "{stale}");
    let states = task_states(&state, "acme", "t1");
    assert_eq!(
        states.len(),
        1,
        "the stale card's action reached the studio: {states:?}"
    );
    // The role's `live` record and the terminal both survived —
    // the reconcile holds, exactly as a studio role's does: no
    // relaunch, no lost, no resumed.
    let moved = hledger(&state)
        .into_iter()
        .filter(|e| e["kind"] == "role.state" && (e["state"] == "lost" || e["state"] == "resumed"))
        .count();
    assert_eq!(moved, 0, "a stage-only restart recorded a loss");
    let sends = hledger(&state)
        .into_iter()
        .filter(|e| e["kind"] == "message.sent" && e["file"].is_string())
        .count();
    let delivered = hledger(&state)
        .into_iter()
        .filter(|e| e["kind"] == "message.delivered")
        .count();
    assert!(
        delivered <= sends,
        "a send repeated past its record: {delivered} > {sends}"
    );

    // A studio-new card: the draft re-validates, `studio.created`
    // lands with the cast in place, and the producer's first
    // message is the brief — delivered once its terminal is up.
    let repo3 = scratch.work.join("repo-beta");
    git_repo(&repo3);
    let cast3 = fixture(&scratch, "cast-beta.toml", CAST_TOML);
    let brief3 = fixture(&scratch, "brief-beta.md", "# the beta brief\n");
    bl_unpinned(
        &scratch,
        &state,
        &format!(
            "hollywood propose studio-new --studio acme-beta --title acme-beta \
             --repo {} --cast {} --brief {} --why a-third-lot --as hollywood",
            repo3.display(),
            cast3.display(),
            brief3.display()
        ),
    );
    bl_unpinned(
        &scratch,
        &state,
        &format!("card apply c{}", last_card(&state)),
    );
    wait_kind(&state, "acme-beta", "studio.created", 1);
    let created = ledger(&state, "acme-beta")
        .into_iter()
        .find(|e| e["kind"] == "studio.created")
        .expect("no studio.created");
    assert_eq!(created["by"]["role"], "user", "{created}");
    let queued = ledger(&state, "acme-beta")
        .into_iter()
        .find(|e| e["kind"] == "message.queued" && e["purpose"] == "brief")
        .expect("no brief message queued");
    assert_eq!(queued["to"], "producer", "{queued}");
    wait_terminal(&client, "bl-acme-beta-producer").await;
    wait_inbox(&state, "fake-inbox-acme-beta-producer", "-brief-c");

    drop(client);
    scratch.finish().expect("scratch teardown");
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_no_hollywood_without_config() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    // Deliberately no config file at all — `config::hollywood` is
    // `Ok(None)`, the absent case.

    studio(&scratch, &state, "acme");
    bl_unpinned(&scratch, &state, "stage start");
    let p = wait_terminal(&client, "bl-acme-producer").await;
    wait_read(&client, &p.address, "of acme").await;

    // Three ticks' room: no terminal, no folder, no document object.
    tokio::time::sleep(Duration::from_secs(3)).await;
    for t in live_terminals(&client).await {
        assert_ne!(t.name.as_deref(), Some("bl-hollywood"), "{t:?}");
    }
    assert!(
        !state.join("hollywood").exists(),
        "a hollywood dir appeared without the config"
    );
    let (_, doc) = wait_doc(&client).await;
    assert!(
        doc.get("hollywood").is_none() || doc["hollywood"].is_null(),
        "the document carries hollywood with no config: {doc}"
    );

    drop(client);
    scratch.finish().expect("scratch teardown");
}
