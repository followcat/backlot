//! Opt-in real-core tests for `backlot studio delete`:
//! terminal isolation (roles + shells, leaving other studios & stage untouched),
//! reopened producer on finished studio, dry-run no-op,
//! dirty worktree `--force` gating, archive + `studio.deleted` event,
//! `studio list` exclusion, `records` preservation, id re-creation, and
//! live stage daemon health without restarts.
//!
//! Ignored by default; run with `HC_TUI_TEST_CORE` set.

#![cfg(unix)]

#[path = "common/real_kit.rs"]
mod kit;
#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use hc_proto::terminal::RunParams;
use kit::*;
use scratch::ScratchCore;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

fn stage_log_text(state: &Path) -> String {
    fs::read_to_string(state.join("stage.log")).unwrap_or_default()
}

fn setup_accepted_delivery(
    scratch: &ScratchCore,
    state: &Path,
    studio: &str,
    repo: &Path,
    merge_branch: bool,
) {
    let req = fixture(scratch, &format!("req-{studio}.md"), "# req\n");
    let acc = fixture(scratch, &format!("acc-{studio}.md"), "# acc\n");
    let rep = fixture(scratch, &format!("rep-{studio}.md"), "# rep\n");
    bl(
        scratch,
        state,
        studio,
        &format!(
            "req revise --file {} --summary v1 --as producer",
            req.display()
        ),
    );
    bl(
        scratch,
        state,
        studio,
        &format!(
            "task new --title T1 --kind feature --acceptance-file {} --as director",
            acc.display()
        ),
    );
    bl(
        scratch,
        state,
        studio,
        "cast --task t1 --actor actor-a --reason fit --as director",
    );
    let work = studio_dir(state, studio).join("work/t1");
    fs::create_dir_all(work.parent().unwrap()).unwrap();
    let branch = format!("backlot/{studio}/t1");
    git(
        repo,
        &["worktree", "add", "-b", &branch, work.to_str().unwrap()],
    );
    let base_sha = git(&work, &["rev-parse", "HEAD"]).trim().to_string();
    fs::write(work.join("deliverable.txt"), "done\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-m", "deliver"]);
    let tip = git(&work, &["rev-parse", "HEAD"]).trim().to_string();
    bl(
        scratch,
        state,
        studio,
        &format!(
            "deliver --task t1 --commits {base_sha}..{tip} --report {} --as actor-t1-c1",
            rep.display()
        ),
    );
    bl(
        scratch,
        state,
        studio,
        "review --task t1 --delivery 1 --verdict accepted --conclusion ok --as director",
    );
    if merge_branch {
        git(repo, &["merge", "--ff-only", &branch]);
    }
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_studio_delete_end_to_end_under_live_stage() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");

    let repo_a = scratch.work.join("repo-shop");
    let repo_b = scratch.work.join("repo-beta");
    git_repo(&repo_a);
    git_repo(&repo_b);
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);

    bl(
        &scratch,
        &state,
        "acme-shop",
        &format!(
            "studio new --title Shop --repo {} --cast {}",
            repo_a.display(),
            cast.display()
        ),
    );
    bl(
        &scratch,
        &state,
        "beta-tools",
        &format!(
            "studio new --title Beta --repo {} --cast {}",
            repo_b.display(),
            cast.display()
        ),
    );
    setup_accepted_delivery(&scratch, &state, "acme-shop", &repo_a, true);
    bl_unpinned(&scratch, &state, "stage start");

    wait_terminal(&client, "bl-acme-shop-producer").await;
    wait_terminal(&client, "bl-beta-tools-producer").await;

    // Open a studio shell for acme-shop and an unrelated plain user terminal.
    client
        .terminal_run(RunParams {
            argv: vec!["sleep".into(), "60".into()],
            name: Some("acme-shell".into()),
            labels: BTreeMap::from([
                ("backlot.studio".into(), "acme-shop".into()),
                ("backlot.shell".into(), "1".into()),
            ]),
            no_parent: true,
            no_notify: true,
            ..Default::default()
        })
        .await
        .expect("spawn acme-shell");
    client
        .terminal_run(RunParams {
            argv: vec!["sleep".into(), "60".into()],
            name: Some("plain-shell".into()),
            no_parent: true,
            no_notify: true,
            ..Default::default()
        })
        .await
        .expect("spawn plain-shell");

    // Finish acme-shop, then reopen its producer before deleting.
    bl(
        &scratch,
        &state,
        "acme-shop",
        "studio finish --reason done --as user",
    );
    wait_absent(&client, "bl-acme-shop-producer").await;
    bl(
        &scratch,
        &state,
        "acme-shop",
        "reopen --person producer --as user",
    );
    wait_terminal(&client, "bl-acme-shop-producer").await;

    // Dry-run prints plan and changes nothing.
    let dry_json = bl_unpinned(&scratch, &state, "studio delete acme-shop --dry-run --json");
    let dry: Value = serde_json::from_str(&dry_json).expect("dry-run json");
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["archived"], false);
    let closed = dry["closed_terminals"].as_array().unwrap();
    assert!(closed.iter().any(|t| t == "bl-acme-shop-producer"), "{dry}");
    assert!(closed.iter().any(|t| t == "acme-shell"), "{dry}");
    assert!(
        terminal_named(&client, "bl-acme-shop-producer")
            .await
            .is_some()
    );
    assert!(terminal_named(&client, "acme-shell").await.is_some());
    assert!(studio_dir(&state, "acme-shop").exists());

    // Real delete closes acme-shop's reopened producer AND its shell, leaving
    // beta-tools, plain-shell, and backlot-stage untouched.
    let del_json = bl_unpinned(&scratch, &state, "studio delete acme-shop --json");
    let del: Value = serde_json::from_str(&del_json).expect("delete json");
    assert_eq!(del["archived"], true, "{del}");
    wait_absent(&client, "bl-acme-shop-producer").await;
    wait_absent(&client, "acme-shell").await;
    assert!(
        terminal_named(&client, "bl-beta-tools-producer")
            .await
            .is_some()
    );
    assert!(terminal_named(&client, "plain-shell").await.is_some());
    assert!(terminal_named(&client, "backlot-stage").await.is_some());

    // Archive exists with `studio.deleted` event; active dir is gone.
    assert!(!studio_dir(&state, "acme-shop").exists());
    let arch_rel = del["archive"].as_str().unwrap();
    let arch_dir = state.join(arch_rel);
    assert!(arch_dir.join("events.jsonl").is_file());
    let arch_events: Vec<Value> = fs::read_to_string(arch_dir.join("events.jsonl"))
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(arch_events.iter().any(|e| {
        e["kind"] == "studio.deleted" && e["by"]["role"] == "user" && e["archive"] == arch_rel
    }));

    // `studio list` excludes acme-shop; `records` still counts archived actors.
    let list_json = bl_unpinned(&scratch, &state, "studio list --json");
    assert!(!list_json.contains("acme-shop"), "{list_json}");
    assert!(list_json.contains("beta-tools"), "{list_json}");
    let recs: Value =
        serde_json::from_str(&bl_unpinned(&scratch, &state, "records --json")).unwrap();
    assert_eq!(recs[0]["tasks"], 1, "{recs}");

    // Running stage publishes doc without acme-shop and keeps scheduling beta-tools
    // with zero error loops / "corrupt ledger".
    let deadline = Instant::now() + DEADLINE;
    loop {
        let (_, doc) = wait_doc(&client).await;
        let ids: Vec<&str> = doc["studios"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s["id"].as_str())
            .collect();
        if !ids.contains(&"acme-shop") && ids.contains(&"beta-tools") {
            break;
        }
        assert!(Instant::now() < deadline, "acme-shop never left doc: {doc}");
        tokio::time::sleep(POLL).await;
    }
    let req_b = fixture(&scratch, "req-b.md", "# req b\n");
    let acc_b = fixture(&scratch, "acc-b.md", "# acc b\n");
    bl(
        &scratch,
        &state,
        "beta-tools",
        &format!(
            "req revise --file {} --summary v1 --as producer",
            req_b.display()
        ),
    );
    bl(
        &scratch,
        &state,
        "beta-tools",
        &format!(
            "task new --title T1 --kind feature --acceptance-file {} --as director",
            acc_b.display()
        ),
    );
    bl(
        &scratch,
        &state,
        "beta-tools",
        "cast --task t1 --actor actor-a --reason fit --as director",
    );
    wait_terminal(&client, "bl-beta-tools-t1").await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let log = stage_log_text(&state);
    assert!(
        !log.contains("corrupt"),
        "unexpected corrupt in stage.log:\n{log}"
    );
    assert!(
        !log.contains("No such file"),
        "unexpected ENOENT in stage.log:\n{log}"
    );

    // Re-creating the same id `acme-shop` works and gets a fresh ledger.
    let repo_a2 = scratch.work.join("repo-shop-2");
    git_repo(&repo_a2);
    bl(
        &scratch,
        &state,
        "acme-shop",
        &format!(
            "studio new --title Shop2 --repo {} --cast {}",
            repo_a2.display(),
            cast.display()
        ),
    );
    let fresh = ledger(&state, "acme-shop");
    assert!(!fresh.is_empty());
    assert_eq!(fresh[0]["seq"], 1);
    assert_eq!(fresh[0]["kind"], "studio.created");
    assert!(
        !fresh
            .iter()
            .any(|e| e["kind"] == "studio.deleted" || e["kind"] == "task.created"),
        "fresh ledger must not carry old events: {fresh:?}"
    );

    drop(client);
    scratch.finish().expect("scratch teardown");
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_studio_delete_dirty_worktree_requires_force() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");

    let repo = scratch.work.join("repo-dirty");
    git_repo(&repo);
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    bl(
        &scratch,
        &state,
        "acme-shop",
        &format!(
            "studio new --title Dirty --repo {} --cast {}",
            repo.display(),
            cast.display()
        ),
    );
    setup_accepted_delivery(&scratch, &state, "acme-shop", &repo, false);
    fs::write(
        studio_dir(&state, "acme-shop").join("work/t1/dirty.txt"),
        "uncommitted\n",
    )
    .unwrap();

    // Without --force: non-zero exit, JSON has kept items, nothing archived.
    let out = bl_run(env(&scratch, &state), "studio delete acme-shop --json");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let v: Value = serde_json::from_str(&stderr).expect("refused json");
    assert_eq!(v["archived"], false, "{stderr}");
    assert!(!v["kept"].as_array().unwrap().is_empty(), "{stderr}");
    assert!(studio_dir(&state, "acme-shop").exists());

    // With --force: succeeds and archives.
    let ok_json = bl_unpinned(&scratch, &state, "studio delete acme-shop --force --json");
    let v2: Value = serde_json::from_str(&ok_json).expect("force json");
    assert_eq!(v2["archived"], true, "{ok_json}");
    assert!(!studio_dir(&state, "acme-shop").exists());

    drop(client);
    scratch.finish().expect("scratch teardown");
}
