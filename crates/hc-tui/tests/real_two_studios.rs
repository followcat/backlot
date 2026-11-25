//! H2 — two live studios on one core, one stage, fake runner only.
//! `acme-shop` and `acme` run a whole production side by side on their
//! own scratch repos with the same task id and the same role names —
//! the prefix-sharing names are the worst case for ownership by name
//! instead of label. Steps interleave: a prompt blocks one studio
//! while the other delivers, a stage restart adopts every role of
//! both, and each reaches `studio.finished` with its ledger, its
//! messages, its checks, its repo branches and its terminals still
//! its own. Ignored by default; run with `HC_TUI_TEST_CORE` set.

#![cfg(unix)]

#[path = "common/real_kit.rs"]
mod kit;
#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use kit::*;

use scratch::ScratchCore;

use hc_proto::client::Client;
use hc_proto::terminal::ReadParams;

use serde_json::Value;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// One of the two studios under test: the name every helper needs —
/// its `backlot` calls, its ledger and its terminals — plus its own
/// repository and state directory.
struct Lot<'a> {
    name: &'static str,
    scratch: &'a ScratchCore,
    state: &'a Path,
    repo: PathBuf,
}

impl<'a> Lot<'a> {
    fn new(scratch: &'a ScratchCore, state: &'a Path, name: &'static str, repo: PathBuf) -> Self {
        Lot {
            name,
            scratch,
            state,
            repo,
        }
    }
    fn dir(&self) -> PathBuf {
        studio_dir(self.state, self.name)
    }
    fn bl(&self, args: &str) -> String {
        bl(self.scratch, self.state, self.name, args)
    }
    fn bl_fail(&self, args: &str) -> String {
        bl_fail(self.scratch, self.state, self.name, args)
    }
    fn ledger(&self) -> Vec<Value> {
        ledger(self.state, self.name)
    }
    fn wait_event(&self, kind: &str, role: &str) -> Value {
        wait_event(self.state, self.name, kind, role)
    }
    fn wait_kind(&self, kind: &str, n: usize) {
        wait_kind(self.state, self.name, kind, n)
    }
    fn wait_checked(&self) -> Value {
        wait_checked(self.state, self.name)
    }
    async fn wait_studio(&self, client: &Client, want: fn(&Value) -> bool) -> Value {
        wait_studio(client, self.name, want).await
    }
    fn inbox(&self, role: &str) -> String {
        inbox_name(self.name, role)
    }
    fn wait_inbox(&self, role: &str, needle: &str) -> String {
        wait_inbox(self.state, &self.inbox(role), needle)
    }
    fn terminal(&self, leaf: &str) -> String {
        format!("bl-{}-{leaf}", self.name)
    }
    fn has_event(&self, kind: &str) -> bool {
        self.ledger().iter().any(|e| e["kind"] == kind)
    }
    /// Every file under the studio's `messages/` concatenated — how
    /// "the other studio's tell never landed here" is proven.
    fn message_texts(&self) -> String {
        let dir = self.dir().join("messages");
        let mut all = String::new();
        if dir.is_dir() {
            for e in fs::read_dir(dir).unwrap() {
                all.push_str(&fs::read_to_string(e.unwrap().path()).unwrap_or_default());
            }
        }
        all
    }
}

/// The cast both studios share: `[limits] fake = 3` — the minimum
/// legal cap, producer+director+one — and one declared check that
/// prints its detached worktree's HEAD and common dir: the log
/// proves which repo the run really used.
const TWO_STUDIO_CAST: &str = r#"
[limits]
fake = 3

[[checks]]
name = "units"
run = "git rev-parse HEAD && git rev-parse --git-common-dir"
timeout_s = 30
"#;

/// `status --all` once it names both studios — the user-facing view
/// of the same shared state the document publishes.
fn status_all(scratch: &ScratchCore, state: &Path) -> String {
    let out = bl_unpinned(scratch, state, "status --all");
    assert!(out.contains("studio acme-shop:"), "{out}");
    assert!(out.contains("studio acme:"), "{out}");
    out
}

/// `status --all --json`'s studios map.
fn status_json(scratch: &ScratchCore, state: &Path) -> Value {
    let out = bl_unpinned(scratch, state, "status --all --json");
    serde_json::from_str(&out).expect("status --all --json")
}

/// The whole screen of a terminal — the cross-send absence proof
/// reads all scrollback, not just the tail.
async fn read_screen(client: &Client, terminal: &str) -> String {
    client
        .terminal_read(ReadParams {
            terminal: terminal.into(),
            lines: 0,
            caller: None,
        })
        .await
        .map(|r| r.text)
        .unwrap_or_default()
}

/// `git` expected to fail — the proof a sha never reached a repo.
fn git_fails(repo: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git")
        .status
        .success();
    assert!(
        !ok,
        "git {args:?} unexpectedly succeeded in {}",
        repo.display()
    );
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_two_studios_side_by_side() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");

    // Two studios, each its own repository, cast file and fixtures —
    // same task ids and role names, prefix-sharing studio names.
    let a = Lot::new(
        &scratch,
        &state,
        "acme-shop",
        scratch.work.join("repo-shop"),
    );
    let b = Lot::new(&scratch, &state, "acme", scratch.work.join("repo-acme"));
    git_repo(&a.repo);
    git_repo(&b.repo);
    let cast_a = fixture(
        &scratch,
        "cast-shop.toml",
        &format!("{CAST_TOML}{TWO_STUDIO_CAST}"),
    );
    let cast_b = fixture(
        &scratch,
        "cast-acme.toml",
        &format!("{CAST_TOML}{TWO_STUDIO_CAST}"),
    );
    let files = |s: &str| {
        (
            fixture(&scratch, &format!("req-{s}.md"), "# requirement\n"),
            fixture(&scratch, &format!("acc-{s}.md"), "# acceptance\n"),
            fixture(&scratch, &format!("plan-{s}.md"), "# plan\n"),
            fixture(&scratch, &format!("note-{s}.md"), "director: go\n"),
            fixture(&scratch, &format!("report-{s}.md"), "# report\n"),
        )
    };
    let (req_a, acc_a, plan_a, note_a, report_a) = files("shop");
    let (req_b, acc_b, plan_b, note_b, report_b) = files("acme");

    a.bl(&format!(
        "studio new --title acme-shop --repo {} --cast {}",
        a.repo.display(),
        cast_a.display()
    ));
    b.bl(&format!(
        "studio new --title acme --repo {} --cast {}",
        b.repo.display(),
        cast_b.display()
    ));
    bl_unpinned(&scratch, &state, "stage start");

    // Both studios' fixed roles come up on the same core, each
    // terminal labelled to its own studio.
    for (lot, role) in [
        (&a, "producer"),
        (&a, "director"),
        (&b, "producer"),
        (&b, "director"),
    ] {
        let t = wait_terminal(&client, &lot.terminal(role)).await;
        check_labels(&t, lot.name, role, None);
    }
    wait_read(&client, &a.terminal("producer"), "of acme-shop").await;
    wait_read(&client, &b.terminal("producer"), "of acme").await;
    assert_terminal_partition(
        &client,
        &["acme-shop", "acme"],
        &[
            "bl-acme-shop-producer",
            "bl-acme-shop-director",
            "bl-acme-producer",
            "bl-acme-director",
        ],
    )
    .await;
    status_all(&scratch, &state);
    a.wait_studio(&client, |s| s["state"] == "live").await;
    b.wait_studio(&client, |s| s["state"] == "live").await;

    // Requirement and plan, interleaved — each writer joins on its
    // own studio's message. Six `fake` roles live over two `fake = 3`
    // caps prove the cap counts per studio, never summed across them.
    for (lot, req) in [(&a, &req_a), (&b, &req_b)] {
        lot.bl(&format!(
            "req revise --file {} --summary v1 --as producer",
            req.display()
        ));
    }
    a.bl("plan request --as producer");
    b.bl("plan request --as producer");
    for (lot, plan, acc) in [(&a, &plan_a, &acc_a), (&b, &plan_b, &acc_b)] {
        let w = wait_terminal(&client, &lot.terminal("writer")).await;
        check_labels(&w, lot.name, "writer", None);
        lot.bl(&format!(
            "plan publish --kind plan --file {} --as writer",
            plan.display()
        ));
        lot.bl(&format!(
            "plan publish --kind acceptance --file {} --as writer",
            acc.display()
        ));
    }
    assert_terminal_partition(
        &client,
        &["acme-shop", "acme"],
        &[
            "bl-acme-shop-producer",
            "bl-acme-shop-director",
            "bl-acme-shop-writer",
            "bl-acme-producer",
            "bl-acme-director",
            "bl-acme-writer",
        ],
    )
    .await;

    // Dispatch frees each writer's slot; each director hears its own
    // producer's note — the inbox files are studio-keyed.
    for (lot, note) in [(&a, &note_a), (&b, &note_b)] {
        lot.bl(&format!("dispatch --file {} --as producer", note.display()));
    }
    wait_absent(&client, &a.terminal("writer")).await;
    wait_absent(&client, &b.terminal("writer")).await;
    a.wait_inbox("director", "-note-shop.md");
    b.wait_inbox("director", "-note-acme.md");

    // Same task id, same candidate, same cast number — only the
    // studio separates `actor-t1-c1` in `acme-shop` from the one in
    // `acme`.
    for (lot, acc) in [(&a, &acc_a), (&b, &acc_b)] {
        lot.bl(&format!(
            "task new --title cart --kind feature --acceptance-file {} --as director",
            acc.display()
        ));
        lot.bl("cast --task t1 --actor actor-a --reason first --as director");
    }
    let act_a = wait_terminal(&client, &a.terminal("t1")).await;
    let act_b = wait_terminal(&client, &b.terminal("t1")).await;
    check_labels(&act_a, "acme-shop", "actor-t1-c1", Some("t1"));
    check_labels(&act_b, "acme", "actor-t1-c1", Some("t1"));
    assert_eq!(
        act_a.cwd.as_str(),
        a.dir().join("work/t1").to_str().unwrap()
    );
    assert_eq!(
        act_b.cwd.as_str(),
        b.dir().join("work/t1").to_str().unwrap()
    );
    // Each repo grows only its own studio's branches.
    for (repo, owner, other) in [
        (&a.repo, "acme-shop", "acme"),
        (&b.repo, "acme", "acme-shop"),
    ] {
        for branch in ["integration", "t1"] {
            assert!(
                git(
                    repo,
                    &["branch", "--list", &format!("backlot/{owner}/{branch}")]
                )
                .contains(branch),
                "no backlot/{owner}/{branch} in {}",
                repo.display()
            );
        }
        assert_eq!(
            git(repo, &["branch", "--list", &format!("backlot/{other}/*")]).trim(),
            "",
            "backlot/{other}/* reached {}",
            repo.display()
        );
    }

    // The mid-production stage restart: every role of both studios
    // is adopted — same terminals, no role.lost, no role.resumed.
    kill_stage(&client).await;
    bl_unpinned(&scratch, &state, "stage start");
    wait_terminal(&client, "backlot-stage").await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    for lot in [&a, &b] {
        for e in lot.ledger() {
            assert!(
                e["kind"] != "role.lost" && e["kind"] != "role.resumed",
                "{}: a stage-only restart recorded a loss: {e}",
                lot.name
            );
        }
        for leaf in ["producer", "director", "t1"] {
            assert!(
                terminal_named(&client, &lot.terminal(leaf)).await.is_some(),
                "{}'s {leaf} is gone",
                lot.name
            );
        }
    }
    for (lot, old) in [(&a, &act_a), (&b, &act_b)] {
        let t = terminal_named(&client, &lot.terminal("t1")).await.unwrap();
        assert_eq!(
            t.address, old.address,
            "{}'s actor was relaunched",
            lot.name
        );
    }

    // A tell to one producer is that studio's business only.
    let tell_a = fixture(&scratch, "tell-shop.md", "for the shop only\n");
    let tell_b = fixture(&scratch, "tell-acme.md", "for acme only\n");
    a.bl(&format!(
        "tell producer --file {} --as user",
        tell_a.display()
    ));
    b.bl(&format!(
        "tell producer --file {} --as user",
        tell_b.display()
    ));
    a.wait_inbox("producer", "-tell-shop.md");
    b.wait_inbox("producer", "-tell-acme.md");
    message_file(&a.dir(), "tell-shop");
    message_file(&b.dir(), "tell-acme");

    // A's actor hits a permission prompt — and B still delivers and
    // checks while A waits.
    send_text(&client, &act_a.address, "work:blocked").await;
    let raised = a.wait_event("attention.raised", "actor-t1-c1");
    assert_eq!(raised["reason"], "permission prompt", "{raised}");
    let work_b = b.dir().join("work/t1");
    let (b1, b2) = (commit_in(&work_b, "b1"), commit_in(&work_b, "b2"));
    b.bl(&format!(
        "deliver --commits {b1}..{b2} --report {} --as actor-t1-c1 --task t1",
        report_b.display()
    ));
    let checked_b = b.wait_checked();
    assert_eq!(checked_b["exit_code"], 0, "{checked_b}");
    let log_b = fs::read_to_string(b.dir().join(checked_b["output"].as_str().unwrap()))
        .expect("acme's check log");
    assert!(log_b.contains(&b2), "acme's check ran elsewhere: {log_b}");
    assert!(
        log_b.contains("repo-acme"),
        "acme's check ran elsewhere: {log_b}"
    );

    // Both actors now blocked: each studio's approve lands only on
    // its own terminal — `enter` on A's, `esc` on B's, never crossed.
    send_text(&client, &act_b.address, "work:blocked").await;
    b.wait_event("attention.raised", "actor-t1-c1");
    a.bl("approve --person actor-t1-c1 --decision allow --as director");
    wait_read(&client, &act_a.address, "fake-approved: enter").await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let screen_b = read_screen(&client, &act_b.address).await;
    assert!(
        screen_b.contains("fake-work: blocked") && !screen_b.contains("fake-approved"),
        "the shop's approval reached acme's actor: {screen_b}"
    );
    b.bl("approve --person actor-t1-c1 --decision deny --as director");
    wait_read(&client, &act_b.address, "fake-approved: esc").await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let screen_a = read_screen(&client, &act_a.address).await;
    assert!(
        screen_a.matches("fake-approved: enter").count() == 1
            && !screen_a.contains("fake-approved: esc"),
        "acme's approval reached the shop's actor: {screen_a}"
    );

    // A delivers and checks too; each ledger's `task.delivered`
    // names its own repo's sha range.
    let work_a = a.dir().join("work/t1");
    let (a1, a2) = (commit_in(&work_a, "a1"), commit_in(&work_a, "a2"));
    a.bl(&format!(
        "deliver --commits {a1}..{a2} --report {} --as actor-t1-c1 --task t1",
        report_a.display()
    ));
    let checked_a = a.wait_checked();
    assert_eq!(checked_a["exit_code"], 0, "{checked_a}");
    let log_a = fs::read_to_string(a.dir().join(checked_a["output"].as_str().unwrap()))
        .expect("acme-shop's check log");
    assert!(
        log_a.contains(&a2),
        "acme-shop's check ran elsewhere: {log_a}"
    );
    assert!(
        log_a.contains("repo-shop"),
        "acme-shop's check ran elsewhere: {log_a}"
    );
    for (lot, range) in [(&a, format!("{a1}..{a2}")), (&b, format!("{b1}..{b2}"))] {
        let delivered = lot
            .ledger()
            .into_iter()
            .find(|e| e["kind"] == "task.delivered")
            .expect("no task.delivered");
        assert_eq!(delivered["commits"], range, "{delivered}");
    }
    // Finishing is refused while a studio's own task is open.
    let refused = a.bl_fail("studio finish --as user");
    assert!(refused.contains("t1"), "{refused}");

    // Both deliveries accepted; each actor closes inside its own
    // studio, and each task branch merges into its own repo's
    // integration branch — the other repo untouched.
    for lot in [&a, &b] {
        lot.bl("review --task t1 --delivery 1 --verdict accepted --conclusion done --as director");
    }
    wait_absent(&client, &a.terminal("t1")).await;
    wait_absent(&client, &b.terminal("t1")).await;
    for (lot, sha) in [(&a, &a2), (&b, &b2)] {
        git(
            &lot.dir().join("work/integration"),
            &["merge", "--no-edit", &format!("backlot/{}/t1", lot.name)],
        );
        git(
            &lot.repo,
            &[
                "merge-base",
                "--is-ancestor",
                sha,
                &format!("backlot/{}/integration", lot.name),
            ],
        );
    }
    for (repo, sha) in [(&b.repo, &a2), (&a.repo, &b2)] {
        git_fails(repo, &["cat-file", "-e", sha]);
    }

    // `acme-shop` finishes while `acme` keeps running: its roles
    // close, the document and `status --all` keep both — one
    // `finished`, one `live`.
    a.bl("studio finish --reason accepted --as user");
    a.wait_kind("studio.finished", 1);
    wait_absent(&client, &a.terminal("producer")).await;
    wait_absent(&client, &a.terminal("director")).await;
    a.wait_studio(&client, |s| s["state"] == "finished").await;
    b.wait_studio(&client, |s| s["state"] == "live").await;
    assert!(!b.has_event("studio.finished"), "acme finished early");
    assert_terminal_partition(
        &client,
        &["acme-shop", "acme"],
        &["bl-acme-producer", "bl-acme-director"],
    )
    .await;
    let j = status_json(&scratch, &state);
    assert!(!j["studios"]["acme-shop"]["finished"].is_null(), "{j}");
    assert!(j["studios"]["acme"]["finished"].is_null(), "{j}");

    // `acme` finishes its own way; both studios end finished, the
    // stage terminal the only `backlot` one left.
    b.bl("studio finish --reason accepted --as user");
    b.wait_kind("studio.finished", 1);
    wait_absent(&client, &b.terminal("producer")).await;
    wait_absent(&client, &b.terminal("director")).await;
    b.wait_studio(&client, |s| s["state"] == "finished").await;
    a.wait_studio(&client, |s| s["state"] == "finished").await;
    status_all(&scratch, &state);
    let j = status_json(&scratch, &state);
    assert!(!j["studios"]["acme"]["finished"].is_null(), "{j}");
    assert_terminal_partition(&client, &["acme-shop", "acme"], &[]).await;

    // The ledger isolation, whole: each `role.started` names its own
    // studio's `fake-bl-<studio>-*` sessions; each `task.delivered`
    // its own range; each `message.delivered` targets a message its
    // own ledger queued.
    for (lot, shas) in [(&a, vec![a1, a2]), (&b, vec![b1, b2])] {
        let events = lot.ledger();
        for e in &events {
            if e["kind"] == "role.started" {
                let sid = e["session_id"].as_str().unwrap_or_default();
                assert!(
                    sid.starts_with(&format!("fake-bl-{}-", lot.name)),
                    "{}: foreign session {sid}: {e}",
                    lot.name
                );
            }
            if e["kind"] == "message.delivered" {
                let target = e["target"].as_u64().expect("delivered target");
                assert!(
                    events
                        .iter()
                        .any(|q| q["kind"] == "message.queued"
                            && q["seq"].as_u64() == Some(target)),
                    "{}: delivered seq {target} was never queued here",
                    lot.name
                );
            }
        }
        for sha in shas {
            assert_eq!(
                git(&lot.repo, &["cat-file", "-t", &sha]).trim(),
                "commit",
                "{sha} missing in {}",
                lot.repo.display()
            );
        }
    }
    // Neither tell crossed: the other studio's producer inbox and
    // messages dir never saw it.
    assert!(
        !fs::read_to_string(state.join(b.inbox("producer")))
            .unwrap_or_default()
            .contains("tell-shop.md"),
        "the shop tell reached acme's producer"
    );
    assert!(
        !fs::read_to_string(state.join(a.inbox("producer")))
            .unwrap_or_default()
            .contains("tell-acme.md"),
        "the acme tell reached the shop's producer"
    );
    assert!(!a.message_texts().contains("for acme only"));
    assert!(!b.message_texts().contains("for the shop only"));

    drop(client);
    scratch.finish().expect("scratch teardown");
}
