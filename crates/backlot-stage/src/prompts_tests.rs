//! Prompt rendering: every placeholder fills, every `backlot`
//! command line the four prompts name parses through the real
//! argument parser, and the first prompt a fresh start gets is the
//! one line pointing at the rendered file.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use backlot_model::event::{ActorIdentity, CheckDecl, Priority, RoleConfigs, StudioMode};
use backlot_model::state::{StudioConfig, StudioState, Task, TaskStatus};

use crate::args;
use crate::prompts::*;
use crate::reconcile::{RoleKind, Start};
use crate::test_support::TestDir;

fn tmp() -> TestDir {
    TestDir::new("backlot-prompt-test")
}

fn actor() -> ActorIdentity {
    ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "plain".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

fn start(kind: RoleKind, role: &str, task: Option<&str>) -> Start {
    Start {
        role: role.into(),
        kind,
        task: task.map(String::from),
        terminal: format!("bl-shop-{role}"),
        actor: actor(),
        repo: "/home/user/acme-shop".into(),
        members: Vec::new(),
        no_git: false,
        resume: None,
    }
}

/// A studio at requirement v2 with one task whose acceptance list
/// lives in the ledger copy.
fn state_with_task() -> StudioState {
    let mut state = StudioState {
        requirement_version: 2,
        ..StudioState::default()
    };
    state.tasks.insert(
        "t1".into(),
        Task {
            id: "t1".into(),
            title: "front page".into(),
            task_kind: "feature".into(),
            requirement_version: 1,
            acceptance: "plan/9-t1-acceptance.md".into(),
            dependencies: Vec::new(),
            status: TaskStatus::Running,
            paused_from: None,
            rework_rounds: 0,
            priority: Priority::Normal,
            cast: None,
            casts: Vec::new(),
            deliveries: Vec::new(),
            checked: Vec::new(),
            reviews: Vec::new(),
            finding_reports: Vec::new(),
            state_changes: Vec::new(),
            created_seq: 9,
            created_ts_ms: 0,
            created_by: "director".into(),
            last_ts_ms: 0,
            closed_ts_ms: None,
            closed_seq: None,
            actor_started_ms: None,
        },
    );
    state
}

/// A studio config carrying `checks` — the rest of the cast is the
/// prompt's neutral fixture.
fn studio_config(checks: Vec<CheckDecl>) -> StudioConfig {
    StudioConfig {
        title: "acme-shop".into(),
        repo: "/home/user/acme-shop".into(),
        members: Vec::new(),
        no_git: false,
        mode: StudioMode::Standard,
        roles: RoleConfigs {
            producer: actor(),
            director: actor(),
            writer: actor(),
        },
        candidates: Vec::new(),
        rework_cap: 2,
        stall_limit_s: 600,
        nudge_after_s: 180,
        escalate_after_s: 600,
        check_env: Vec::new(),
        checks,
        task_budget_s: None,
        limits: BTreeMap::new(),
    }
}

/// Render fills every placeholder: no `{name}` survives in any role's
/// output, and the concrete values land where they should.
#[test]
fn render_fills_every_placeholder() {
    let dir = tmp();
    let state = state_with_task();
    for (kind, role, task) in [
        (RoleKind::Producer, "producer", None),
        (RoleKind::Writer, "writer", None),
        (RoleKind::Director, "director", None),
        (RoleKind::Actor, "actor-t1-c1", Some("t1")),
    ] {
        let st = start(kind, role, task);
        let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains('{'), "{role} left a placeholder: {text}");
        assert!(text.contains("`acme-shop`"), "{role} misses the studio");
        if matches!(kind, RoleKind::Writer | RoleKind::Actor) {
            let req = dir.join("req/v2.md").to_string_lossy().into_owned();
            assert!(text.contains(&req), "{role} misses the requirement file");
            assert!(text.contains("v2"), "{role} misses the version");
        }
        if matches!(kind, RoleKind::Producer | RoleKind::Director) {
            let req = dir.join("req").to_string_lossy().into_owned();
            assert!(text.contains(&req), "{role} misses the req folder");
        }
    }
    // The actor's copy carries its task, title, acceptance and branch.
    let st = start(RoleKind::Actor, "actor-t1-c1", Some("t1"));
    let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("task `t1`: front page"), "{text}");
    let accept = dir
        .join("plan/9-t1-acceptance.md")
        .to_string_lossy()
        .into_owned();
    assert!(text.contains(&accept), "{text}");
    assert!(text.contains("backlot/acme-shop/t1"), "{text}");
    let work = dir.join("work/t1").to_string_lossy().into_owned();
    assert!(text.contains(&work), "{text}");
    // The director's names the integration branch instead — and the
    // merge line carries the task-branch form S11c had to dig out of
    // the binary.
    let st = start(RoleKind::Director, "director", None);
    let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("backlot/acme-shop/integration"), "{text}");
    assert!(
        text.contains("`git merge backlot/acme-shop/<task> -m <message>`"),
        "{text}"
    );
}

/// S9g: the actor's Delivering section names each declared check —
/// one line per `[[checks]]` entry, `name` and `run` — and says the
/// `--check <name>` it reports is one of them; with none declared it
/// says so and the deliver command takes no `--check`.
#[test]
fn the_actor_prompt_names_the_declared_checks() {
    let dir = tmp();
    let mut state = state_with_task();
    let st = start(RoleKind::Actor, "actor-t1-c1", Some("t1"));

    state.studio = Some(studio_config(vec![
        CheckDecl {
            name: "units".into(),
            run: "cargo test --lib".into(),
            timeout_s: 30,
        },
        CheckDecl {
            name: "lint".into(),
            run: "cargo clippy".into(),
            timeout_s: 60,
        },
    ]));
    let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
    let text = fs::read_to_string(&file).unwrap();
    for want in [
        "`units`: `cargo test --lib`",
        "`lint`: `cargo clippy`",
        "--check <name>=pass",
        "--check <name>=fail",
        "`<name>` is one of",
    ] {
        assert!(text.contains(want), "actor prompt misses {want}:\n{text}");
    }
    assert!(!text.contains("declares no checks"), "{text}");
    // One line per declared check, in declared order.
    let units = text.find("units").unwrap();
    let lint = text.find("lint").unwrap();
    assert!(units < lint, "check order flipped:\n{text}");

    // No declared checks: the prompt says so and the command drops
    // the flag — `--check` survives only inside the explanation.
    state.studio = Some(studio_config(Vec::new()));
    let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("declares no checks"), "{text}");
    assert!(
        text.contains("`backlot deliver --commits <A..B> --report <file>`"),
        "{text}"
    );
    assert!(!text.contains("--check <name>"), "{text}");
}

/// Producer and director start before any requirement exists: the
/// rendered prompt names the req folder and shows neither an empty
/// file nor a version zero — writer and actor, who start after a
/// requirement exists, keep the concrete file and version.
#[test]
fn pre_requirement_prompt_names_the_req_folder() {
    let dir = tmp();
    let state = StudioState::default();
    let req_dir = dir.join("req").to_string_lossy().into_owned();
    for kind in [RoleKind::Producer, RoleKind::Director] {
        let st = start(kind, kind.name(), None);
        let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains("``"), "{kind:?} has empty backticks: {text}");
        // `v0` as a rendered version — the check reads the text with
        // the temp dir's own name scrubbed: a CI TMPDIR suffix is
        // letters and digits and can itself carry `v0`, and every
        // rendered path sits under it.
        let outside_paths = text.replace(&dir.to_string_lossy().into_owned(), "");
        assert!(!outside_paths.contains("v0"), "{kind:?} shows v0: {text}");
        assert!(
            text.contains(&req_dir),
            "{kind:?} misses the req folder: {text}"
        );
    }
}

/// Every `backlot ...` span the four prompts name goes through the
/// real argument parser — a renamed flag or command fails here, not
/// in front of an agent.
#[test]
fn every_command_in_the_prompts_parses() {
    for (kind, name) in [
        (RoleKind::Producer, "producer"),
        (RoleKind::Writer, "writer"),
        (RoleKind::Director, "director"),
        (RoleKind::Actor, "actor"),
    ] {
        let text = template(kind);
        let mut found = 0;
        for span in text.split('`').skip(1).step_by(2) {
            let Some(cmd) = span.strip_prefix("backlot ") else {
                continue;
            };
            found += 1;
            let words: Vec<String> = cmd.split(' ').map(String::from).collect();
            assert!(
                args::parse(&words).is_ok(),
                "{name}.md: `backlot {cmd}` does not parse"
            );
        }
        assert!(found > 0, "{name}.md names no backlot command");
    }
}

/// R1-1: Hollywood's prompt — every `backlot` span parses through
/// the real argument parser, and every line "What you may run" names
/// is both inside the role's gate (reads, its own ledger, a producer
/// `tell`, the studio-new dry run) and covered by each pre-allowing
/// runner's rule list — a command the prompt teaches but the launch
/// does not allow fails here, not in front of an agent.
#[test]
fn hollywood_prompt_commands_parse_and_stay_preallowed() {
    /// Quotes group words; nothing else is special.
    fn shell_words(cmd: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut quote = None;
        for c in cmd.chars() {
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => cur.push(c),
                None if c == '"' || c == '\'' => quote = Some(c),
                None if c.is_whitespace() => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                None => cur.push(c),
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    }

    /// A pre-allow rule → the command prefix it allows.
    fn rule_prefix(rule: &str) -> &str {
        let inner = rule
            .strip_prefix("Bash(")
            .or_else(|| rule.strip_prefix("Shell("))
            .unwrap_or(rule);
        inner
            .trim_end_matches(')')
            .trim_end_matches(":*")
            .trim_end_matches(" *")
    }

    let covered = |rules: &[String], cmd: &str| {
        rules
            .iter()
            .any(|p| cmd == *p || cmd.starts_with(&format!("{p} ")))
    };
    let claude: Vec<String> =
        crate::runners::claude::allowed_tools("/state/hollywood", "hollywood", &[])
            .split(',')
            .filter(|r| r.starts_with("Bash("))
            .map(|r| rule_prefix(r).to_string())
            .collect();
    let cursor: Vec<String> = crate::runners::cursor::allow_rules("hollywood", &[])
        .iter()
        .map(|r| rule_prefix(r).to_string())
        .collect();
    let cmd_runner: Vec<String> = crate::runners::cmd::allow_rules("hollywood", &[])
        .iter()
        .map(|r| rule_prefix(r).to_string())
        .collect();

    // Every backticked `backlot` span parses — a renamed flag or
    // action fails here, not in front of an agent.
    let mut all = 0;
    for span in HOLLYWOOD.split('`').skip(1).step_by(2) {
        if let Some(c) = span.strip_prefix("backlot ") {
            all += 1;
            assert!(
                args::parse(&shell_words(c)).is_ok(),
                "hollywood.md: `backlot {c}` does not parse"
            );
        }
    }
    assert!(all > 0, "hollywood.md names no backlot command");

    // "What you may run" is the contract: each line is inside the
    // role's gate and every pre-allowing runner covers it.
    let section = HOLLYWOOD
        .split("## What you may run")
        .nth(1)
        .expect("the prompt has a What-you-may-run section")
        .split("## ")
        .next()
        .unwrap();
    let mut seen = 0;
    for span in section.split('`').skip(1).step_by(2) {
        let Some(c) = span.strip_prefix("backlot ") else {
            continue;
        };
        seen += 1;
        let (_, cmd) = args::parse(&shell_words(c)).unwrap();
        match cmd {
            args::Cmd::Status { .. } | args::Cmd::Records { .. } | args::Cmd::Hollywood(_) => {}
            args::Cmd::Tell { role, .. } => assert_eq!(role, "producer"),
            args::Cmd::StudioNew { dry_run, .. } => {
                assert!(dry_run, "`backlot {c}` is not the dry run")
            }
            _ => panic!("`backlot {c}` is outside Hollywood's gate"),
        }
        for (runner, rules) in [
            ("claude", &claude),
            ("cursor", &cursor),
            ("cmd", &cmd_runner),
        ] {
            assert!(
                covered(rules, &format!("backlot {c}")),
                "{runner}'s pre-allow misses `backlot {c}`"
            );
        }
    }
    assert!(seen > 0, "the prompt names no runnable command");
}

/// Once the plan and the acceptance list are both published, the
/// writer's last step is telling the producer — the dispatch is
/// the producer's to send, not the director's to take.
#[test]
fn writer_done_goes_through_the_producer() {
    let text = template(RoleKind::Writer);
    let tail = text.rsplit("published").next().unwrap();
    assert!(
        tail.contains("backlot tell producer --file <file>"),
        "writer.md does not end by telling the producer: {tail}"
    );
}

/// The restart run (S8r): a director told only "report at the
/// nodes" asked for the node's name on its own screen, where nobody
/// reads it, and a recovered producer reviewed a task the director
/// had already accepted. The director names the integration node and
/// routes questions through `tell`; the producer leaves reviews to
/// the director after a dispatch.
#[test]
fn integration_report_and_review_have_one_owner() {
    let director = template(RoleKind::Director);
    for want in [
        "`backlot report --node integration --file <file>`",
        "Nobody reads your screen",
        "`backlot tell producer --file <file>`",
    ] {
        assert!(director.contains(want), "director.md lacks {want}");
    }
    let producer = template(RoleKind::Producer);
    let after = producer
        .split("backlot dispatch --file <file>")
        .nth(1)
        .unwrap();
    assert!(
        after.contains("do not review the tasks yourself") && after.contains("`integration` node"),
        "producer.md does not leave reviews to the director after a dispatch: {after}"
    );
}

/// All four prompts carry the shared one-command rule verbatim —
/// a chained command falls outside `Bash(backlot *)` and waits on
/// a person — and nothing outside that rule instructs a chained
/// shell, an automatic permission mode or a bypass flag.
#[test]
fn prompts_share_the_one_command_rule() {
    const SHARED_RULE: &str = concat!(
        "- Run `backlot` commands one per shell call, exactly as written:\n",
        "  no `&&`, `;`, pipes, `cd` or shell variables around them.\n",
        "  These run without asking; anything chained stops at a\n",
        "  permission prompt and waits for a person. A file that is only\n",
        "  input to a `backlot` command goes in your session's scratchpad\n",
        "  directory — your system prompt names it — never the repo or\n",
        "  your work area: write it with your file-writing tool, then\n",
        "  pass its absolute path. Read files with your file-reading\n",
        "  tool, not `cat`."
    );
    for (kind, name) in [
        (RoleKind::Producer, "producer"),
        (RoleKind::Writer, "writer"),
        (RoleKind::Director, "director"),
        (RoleKind::Actor, "actor"),
    ] {
        let text = template(kind);
        assert!(
            text.contains(SHARED_RULE),
            "{name}.md misses the shared one-command rule"
        );
        let rest = text.replace(SHARED_RULE, "");
        for bad in ["&&", "auto mode", "skip-permissions"] {
            assert!(
                !rest.contains(bad),
                "{name}.md instructs `{bad}` outside the shared rule"
            );
        }
    }
}

/// R14: an experimental studio appends the research-level guidance
/// to every role's prompt; a standard studio's rendered prompts are
/// byte-identical to what they always were.
#[test]
fn experimental_mode_appends_the_research_guidance() {
    let dir = tmp();
    for (kind, role, task, want) in [
        (
            RoleKind::Producer,
            "producer",
            None,
            "research questions and hypotheses",
        ),
        (
            RoleKind::Director,
            "director",
            None,
            "backlot cast --task <task> --suggest",
        ),
        (RoleKind::Writer, "writer", None, "falsify the hypothesis"),
        (
            RoleKind::Actor,
            "actor-t1-c1",
            Some("t1"),
            "Never fabricate a metric",
        ),
    ] {
        let st = start(kind, role, task);
        // Standard: no appendix at all — the template is the text.
        let mut state = state_with_task();
        state.studio = Some(studio_config(Vec::new()));
        let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
        let std_text = fs::read_to_string(&file).unwrap();
        assert!(
            !std_text.contains("## Experimental production"),
            "{role}: standard prompt carries the appendix"
        );
        // Experimental: the appendix lands and names the mode.
        state.studio.as_mut().unwrap().mode = StudioMode::Experimental;
        let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
        let exp_text = fs::read_to_string(&file).unwrap();
        assert!(
            exp_text.starts_with(&std_text),
            "{role}: the appendix is not a clean suffix"
        );
        assert!(
            exp_text.contains("## Experimental production"),
            "{role} misses the appendix"
        );
        assert!(exp_text.contains(want), "{role} misses '{want}'");
        // The backticked command inside it still parses.
        let appended = exp_text.strip_prefix(&std_text).unwrap();
        for span in appended.split('`').skip(1).step_by(2) {
            let Some(c) = span.strip_prefix("backlot ") else {
                continue;
            };
            let words: Vec<String> = c.split(' ').map(String::from).collect();
            assert!(
                args::parse(&words).is_ok(),
                "{role}: `backlot {c}` fails to parse"
            );
        }
    }
}

/// A recast start's prompt says it takes the role over and names the
/// two reads it owes first; a same-identity start gains nothing.
#[test]
fn a_recast_start_gets_the_takeover_section() {
    use backlot_model::state::RoleState;
    let dir = tmp();
    let mut state = StudioState {
        studio: Some(studio_config(Vec::new())),
        ..StudioState::default()
    };
    state.roles.insert(
        "director".into(),
        RoleState {
            live: true,
            actor: Some(actor()),
            ..RoleState::default()
        },
    );
    let mut recast = start(RoleKind::Director, "director", None);
    recast.actor = ActorIdentity {
        model: "other".into(),
        ..actor()
    };
    state.recasts.push(backlot_model::state::RecastRecord {
        role: "director".into(),
        actor: recast.actor.clone(),
        reason: None,
        card: None,
        seq: 1,
        ts_ms: 0,
    });
    let text = fs::read_to_string(prepare(&dir, "acme-shop", &recast, &state).unwrap()).unwrap();
    assert!(text.contains("Taking over this role"), "{text}");
    assert!(text.contains("backlot status --studio acme-shop"), "{text}");
    assert!(
        text.contains("backlot records --studio acme-shop"),
        "{text}"
    );

    // The same identity is not a takeover.
    let same = start(RoleKind::Director, "director", None);
    let text = fs::read_to_string(prepare(&dir, "acme-shop", &same, &state).unwrap()).unwrap();
    assert!(!text.contains("Taking over this role"), "{text}");
}

/// The first prompt is one line pointing at the rendered file; for
/// `devin` the line itself lands in a file the argv names.
#[test]
fn first_prompt_is_one_line() {
    let file = PathBuf::from("/state/studios/acme-shop/prompts/actor-t1-c1.md");
    let line = first_line(&file);
    assert_eq!(
        line,
        "Read /state/studios/acme-shop/prompts/actor-t1-c1.md and follow it."
    );
    let dir = tmp();
    fs::create_dir_all(dir.join("prompts")).unwrap();
    let f = first_file(&dir, "actor-t1-c1", &line).unwrap();
    assert_eq!(f, dir.join("prompts").join("actor-t1-c1.first.txt"));
    assert_eq!(fs::read_to_string(&f).unwrap(), format!("{line}\n"));
}

/// The producer's questions reach the person: every one is also a
/// `tell user`, and the first "what should we build" goes that way
/// when no requirement is recorded yet. The terminal stays the
/// conversation.
#[test]
fn the_producer_records_every_question_as_a_tell() {
    let text = template(RoleKind::Producer);
    assert!(
        text.contains("`backlot tell user --text <question>`"),
        "{text}"
    );
    assert!(
        text.contains("When no requirement is recorded yet"),
        "{text}"
    );
    assert!(
        text.contains("\"What would you like us to build?\""),
        "{text}"
    );
    assert!(
        text.contains("Every question you put to the user also goes through"),
        "{text}"
    );
}

/// `backlot tell user` is in the producer's pre-allowed commands for
/// every runner with a pre-allow list.
#[test]
fn the_producers_tell_user_is_preallowed() {
    let claude = crate::runners::claude::allowed_tools("/state/studios/acme-shop", "producer", &[]);
    assert!(
        claude.split(',').any(|rule| rule == "Bash(backlot *)"),
        "{claude}"
    );
    for (runner, rules) in [
        (
            "cursor",
            crate::runners::cursor::allow_rules("producer", &[]),
        ),
        ("cmd", crate::runners::cmd::allow_rules("producer", &[])),
    ] {
        assert!(
            rules.iter().any(|rule| rule == "Shell(backlot)"),
            "{runner}: {rules:?}"
        );
    }
}
