//! PR-4 fail-first tests: `guard_git` git-write blocks, `cast --readonly`,
//! `deliver` clean-worktree / unmoved-HEAD / no-commits enforcement,
//! audit integration refusal, per-runner `readonly` settings, and
//! clean audit start prompts + prompt instructions.

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::process::ExitCode;

use backlot_model::EventBy;
use backlot_model::event::{
    ActorIdentity, DesignGate, EventKind, Priority, RoleConfigs, StudioMode,
};
use backlot_model::state::{
    CastRecord, DeliveryRecord, Message, StudioConfig, StudioState, Task, TaskStatus,
};

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::guard::run_in;
use crate::hook::run_io_env;
use crate::prompts;
use crate::reconcile::{RoleKind, Start};
use crate::store::{self, Pending};
use crate::test_support::TestDir;

fn runner_id(runner: &str, profile: &str) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: if runner == "cline" {
            "cline-pass/sonnet"
        } else {
            "m"
        }
        .into(),
        effort: (!matches!(runner, "cursor" | "cmd" | "opencode" | "devin")).then(|| "high".into()),
        permission_profile: profile.into(),
    }
}

fn sargs(cmd: &[&str]) -> Vec<String> {
    cmd.iter().map(|s| (*s).to_string()).collect()
}

fn append_raw(dir: &Path, role: &str, kind: EventKind) {
    store::append(dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: backlot_model::NewEvent::new(EventBy::role(role), kind),
            files: Vec::new(),
        })
    })
    .unwrap();
}

#[test]
fn guard_refuses_all_git_writes_and_allows_reads_on_readonly() {
    let (scratch, base_env) = scratch();
    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("design_gate = \"off\"\n{CAST_TOML}"),
    );
    ok(
        &base_env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let req = fixture(&scratch, "req.md", "# req\n");
    ok(
        &base_env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    let acc = fixture(&scratch, "acc.md", "# acc\n");
    ok(
        &base_env,
        &format!("task new --title t1 --kind audit --acceptance-file {acc} --as director"),
    );
    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    let mut ro_actor = runner_id("runner-a", "readonly");
    ro_actor.model = "model-a".into();
    append_raw(
        &sdir,
        "director",
        EventKind::TaskCast {
            task: "t1".into(),
            candidate: "actor-a".into(),
            actor: ro_actor,
            reason: "audit".into(),
            priority: None,
        },
    );
    let bin = scratch.dir.join("usr-bin");
    fs::create_dir_all(&bin).unwrap();
    let fake_git = bin.join("git");
    fs::write(&fake_git, "#!/bin/sh\necho GIT_OK\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_git, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let work = work_area(&scratch, "t1");
    let mut vars = base_env.vars.clone();
    vars.insert("PATH".into(), bin.to_string_lossy().into_owned());
    vars.insert("BACKLOT_ROLE".into(), "actor-t1-c1".into());
    let ro_env = Env { vars };
    for cmd in [
        "git add .",
        "git commit -m oops",
        "git apply x.patch",
        "git checkout -- f",
        "git reset --hard",
        "git merge main",
        "git rebase main",
        "git cherry-pick HEAD",
        "git stash",
        "git push origin main",
        "git tag v1",
        "git branch -d main",
        "git branch -D main",
        "git branch -m main other",
    ] {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let (code, err) = run_in(&sargs(&parts), &ro_env, &work, false);
        assert_ne!(code, ExitCode::SUCCESS, "readonly allowed write: {cmd}");
        assert!(err.is_some(), "expected refusal for {cmd}");
    }
    for cmd in [
        "git status",
        "git log -1",
        "git diff",
        "git show HEAD",
        "git blame f",
        "git grep base",
        "git branch --list",
    ] {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let (code, err) = run_in(&sargs(&parts), &ro_env, &work, false);
        assert_eq!(
            code,
            ExitCode::SUCCESS,
            "readonly blocked read {cmd}: {err:?}"
        );
        assert!(err.is_none(), "{cmd}: {err:?}");
    }
    let mut dvars = base_env.vars.clone();
    dvars.insert("PATH".into(), bin.to_string_lossy().into_owned());
    dvars.insert("BACKLOT_ROLE".into(), "director".into());
    let dir_env = Env { vars: dvars };
    let (code, err) = run_in(
        &sargs(&["git", "merge", "backlot/acme-shop/t1"]),
        &dir_env,
        &work,
        false,
    );
    assert_ne!(
        code,
        ExitCode::SUCCESS,
        "director must not merge audit branch"
    );
    assert!(err.is_some(), "expected audit merge refusal");
}

#[test]
fn cli_cast_readonly_and_deliver_constraints() {
    let (scratch, env) = scratch();
    let repo = shared_repo(&scratch);
    let cast_file = fixture(
        &scratch,
        "cast.toml",
        &format!("design_gate = \"off\"\n{CAST_TOML}"),
    );
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast_file}",
            repo.display()
        ),
    );
    let req = fixture(&scratch, "req.md", "# req\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    let acc = fixture(&scratch, "acc.md", "# audit criteria\n");
    ok(
        &env,
        &format!("task new --title audit-1 --kind audit --acceptance-file {acc} --as director"),
    );
    ok(
        &env,
        &format!("task new --title feat-2 --kind feature --acceptance-file {acc} --as director"),
    );

    // Non-readonly cast onto an audit task is refused.
    let _ = refused(&env, "cast --task t1 --actor actor-a --as director");
    // `--readonly` lowers actor-a without editing cast.toml; feat-2 still gets worktree.
    ok(
        &env,
        "cast --task t1 --actor actor-a --readonly --as director",
    );
    ok(&env, "cast --task t2 --actor actor-b --as director");
    let st: serde_json::Value = serde_json::from_str(&ok(&env, "status --json")).unwrap();
    assert_eq!(
        st["tasks"]["t1"]["cast"]["actor"]["permission_profile"],
        "readonly"
    );
    assert_eq!(
        st["tasks"]["t2"]["cast"]["actor"]["permission_profile"],
        "worktree"
    );

    let work = shared_area(&scratch, "t1");
    let rep = fixture(&scratch, "audit-report.md", "# Audit verdict\nclean\n");
    // Passing --commits on readonly refuses.
    let base = git(&work, &["rev-parse", "HEAD"]);
    let _ = refused(
        &env,
        &format!("deliver --commits {base}..{base} --report {rep} --as actor-t1-c1 --task t1"),
    );
    // Dirty worktree refuses.
    fs::write(work.join("scratch.txt"), "untracked").unwrap();
    let _ = refused(
        &env,
        &format!("deliver --report {rep} --as actor-t1-c1 --task t1"),
    );
    fs::remove_file(work.join("scratch.txt")).unwrap();
    // Moved HEAD refuses.
    let _moved = commit_in(&work, "illegal commit");
    let _ = refused(
        &env,
        &format!("deliver --report {rep} --as actor-t1-c1 --task t1"),
    );

    // Clean worktree with unmoved HEAD succeeds on t3 and delivers empty commits.
    ok(
        &env,
        &format!("task new --title audit-2 --kind audit --acceptance-file {acc} --as director"),
    );
    ok(
        &env,
        "cast --task t3 --actor actor-a --readonly --as director",
    );
    let _w3 = shared_area(&scratch, "t3");
    ok(
        &env,
        &format!("deliver --report {rep} --as actor-t3-c1 --task t3"),
    );
    let st2: serde_json::Value = serde_json::from_str(&ok(&env, "status --json")).unwrap();
    assert_eq!(st2["tasks"]["t3"]["deliveries"][0]["commits"], "");
    ok(
        &env,
        "review --task t3 --delivery 1 --verdict accepted --conclusion clean --as director",
    );
    // Integrating an audit task as a dependency refuses.
    let _ = refused(
        &env,
        &format!(
            "task new --title follow --kind feature --acceptance-file {acc} --depends-on t3 --as director"
        ),
    );
}

#[test]
fn agy_hook_denies_file_writes_on_readonly_role() {
    let (s, mut env) = scratch();
    let cast = fixture(
        &s,
        "cast.toml",
        &format!("design_gate = \"off\"\n{CAST_TOML}"),
    );
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let req = fixture(&s, "req.md", "# req\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    let acc = fixture(&s, "acc.md", "# acc\n");
    ok(
        &env,
        &format!("task new --title t1 --kind audit --acceptance-file {acc} --as director"),
    );
    ok(
        &env,
        "cast --task t1 --actor actor-a --readonly --as director",
    );
    env.vars.insert("BACKLOT_ROLE".into(), "actor-t1-c1".into());
    let sig = s.files.join("sig.jsonl");
    let args = vec![
        "--runner".into(),
        "agy".into(),
        "--file".into(),
        sig.to_string_lossy().into(),
    ];
    let mut out = Vec::new();
    let payload = r#"{"conversationId":"c1","hookEventName":"PreToolUse","toolCall":{"name":"write_to_file","args":{"TargetFile":"foo.rs"}}}"#;
    let code = run_io_env(
        &args,
        &env,
        &s.files,
        std::time::Duration::from_secs(2),
        &mut Cursor::new(payload.as_bytes()),
        &mut out,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    let v: serde_json::Value =
        serde_json::from_str(String::from_utf8(out).unwrap().trim()).unwrap();
    assert_eq!(
        v["decision"], "deny",
        "agy hook must deny write_to_file on readonly role"
    );
}

fn mk_task(id: &str, kind: &str, acc: &str, actor: ActorIdentity, report: Option<&str>) -> Task {
    Task {
        id: id.into(),
        title: id.into(),
        task_kind: kind.into(),
        requirement_version: 1,
        acceptance: acc.into(),
        dependencies: Vec::new(),
        status: if report.is_some() {
            TaskStatus::Accepted
        } else {
            TaskStatus::Running
        },
        paused_from: None,
        rework_rounds: 0,
        priority: Priority::Normal,
        cast: Some(CastRecord {
            seq: 4,
            by: "director".into(),
            candidate: "a".into(),
            actor: actor.clone(),
            reason: String::new(),
            priority: None,
            ts_ms: 1,
        }),
        casts: Vec::new(),
        deliveries: report
            .map(|r| DeliveryRecord {
                seq: 6,
                n: 1,
                actor,
                commits: "111..222".into(),
                report: r.into(),
                checks: Vec::new(),
                requirement_version: 1,
                usage: None,
                ts_ms: 2,
            })
            .into_iter()
            .collect(),
        checked: Vec::new(),
        reviews: Vec::new(),
        finding_reports: Vec::new(),
        state_changes: Vec::new(),
        created_seq: 3,
        created_ts_ms: 1,
        created_by: "director".into(),
        last_ts_ms: 2,
        closed_ts_ms: None,
        closed_seq: None,
        actor_started_ms: None,
    }
}

#[test]
fn audit_start_prompt_clean_context_and_role_instructions() {
    let dir = TestDir::new("bl-ro-prompt");
    let actor_id = runner_id("fake", "worktree");
    let ro_id = runner_id("fake", "readonly");
    let mut state = StudioState {
        requirement_version: 1,
        studio: Some(StudioConfig {
            title: "acme-shop".into(),
            repo: "/home/user/acme-shop".into(),
            members: Vec::new(),
            no_git: false,
            mode: StudioMode::Standard,
            roles: RoleConfigs {
                producer: runner_id("fake", "standard"),
                director: runner_id("fake", "standard"),
                writer: runner_id("fake", "standard"),
            },
            candidates: Vec::new(),
            rework_cap: 2,
            stall_limit_s: 600,
            nudge_after_s: 180,
            escalate_after_s: 600,
            check_env: Vec::new(),
            checks: Vec::new(),
            task_budget_s: None,
            limits: BTreeMap::new(),
            max_actors: 4,
            push: Default::default(),
            design_gate: DesignGate::Off,
        }),
        ..StudioState::default()
    };
    state.messages.push(Message {
        seq: 5,
        from: "director".into(),
        to: "actor-t1-c1".into(),
        file: Some("messages/BUILD-DISCUSSION-THREAD.md".into()),
        purpose: None,
        source_seq: None,
        delivered: true,
        dropped: false,
        dropped_reason: None,
        ts_ms: 1,
        delivered_ms: None,
    });
    state.tasks.insert(
        "t1".into(),
        mk_task(
            "t1",
            "feature",
            "plan/t1-acc.md",
            actor_id,
            Some("BUILDER-SECRET-REPORT.md"),
        ),
    );
    state.tasks.insert(
        "t2".into(),
        mk_task("t2", "audit", "plan/t2-audit-acc.md", ro_id.clone(), None),
    );

    let start = Start {
        role: "actor-t2-c1".into(),
        kind: RoleKind::Actor,
        task: Some("t2".into()),
        terminal: "bl-shop-t2".into(),
        actor: ro_id,
        repo: "/home/user/acme-shop".into(),
        members: Vec::new(),
        no_git: false,
        resume: None,
    };
    let file = prompts::prepare(&dir, "acme-shop", &start, &state).unwrap();
    let prompt_txt = fs::read_to_string(&file).unwrap();
    assert!(prompt_txt.contains("plan/t2-audit-acc.md"), "{prompt_txt}");
    assert!(
        prompt_txt.contains("backlot deliver --report <file>"),
        "readonly prompt must explain report-only deliver: {prompt_txt}"
    );
    for leak in [
        "BUILDER-SECRET-REPORT",
        "BUILD-DISCUSSION-THREAD",
        "plan/t1-acc.md",
    ] {
        assert!(
            !prompt_txt.contains(leak),
            "clean audit context leaked `{leak}`:\n{prompt_txt}"
        );
    }

    let dir_txt = prompts::template(RoleKind::Director);
    assert!(
        dir_txt.contains("--readonly")
            && dir_txt.contains("--kind audit")
            && dir_txt.contains("backlot finding"),
        "{dir_txt}"
    );
    let wri_txt = prompts::template(RoleKind::Writer);
    assert!(wri_txt.contains("audit"), "{wri_txt}");
}
