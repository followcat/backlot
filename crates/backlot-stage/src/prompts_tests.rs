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
        max_actors: backlot_model::event::default_max_actors(),
        push: Default::default(),
        design_gate: backlot_model::event::DesignGate::Off,
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
    assert!(
        !text.contains("Repo notes approved"),
        "unexpected repo notes notice without approved notes: {text}"
    );

    // Case 2: when approved repo notes exist for the studio's canonical repo,
    // `{repo_notes}` renders in `director` and `writer` pointing to
    // `<studio_dir>/repo-notes.md` (mode 0400), while `producer` and `actor`
    // stay untouched.
    let base = tmp();
    let repo_dir = base.join("shop-repo");
    fs::create_dir_all(&repo_dir).unwrap();
    let canon_repo = crate::args_studio::canonical_repo(repo_dir.to_str().unwrap());
    let notes_dir = base.join("repo-notes");
    fs::create_dir_all(&notes_dir).unwrap();
    let notes_path = notes_dir.join(format!(
        "{}.md",
        backlot_model::repo_notes::repo_hash_prefix(&canon_repo)
    ));
    let notes_body = format!(
        "# Repo Notes: {canon_repo}\n\n## build-cmd\n<!-- card c1 -->\nRun cargo test --lib.\n"
    );
    fs::write(&notes_path, &notes_body).unwrap();

    let sdir = base.join("studios/acme-shop");
    fs::create_dir_all(&sdir).unwrap();
    let expected_snapshot = sdir.join("repo-notes.md");
    let expected_line = format!(
        "Repo notes approved by the person in earlier studios on this repo: read {} before planning.",
        expected_snapshot.display()
    );
    for (kind, role, task) in [
        (RoleKind::Producer, "producer", None),
        (RoleKind::Writer, "writer", None),
        (RoleKind::Director, "director", None),
        (RoleKind::Actor, "actor-t1-c1", Some("t1")),
    ] {
        let mut st = start(kind, role, task);
        st.repo = canon_repo.clone();
        let file = prepare(&sdir, "acme-shop", &st, &state).unwrap();
        let rendered = fs::read_to_string(&file).unwrap();
        assert!(
            !rendered.contains('{'),
            "{role} left a placeholder: {rendered}"
        );
        if matches!(kind, RoleKind::Director | RoleKind::Writer) {
            assert!(
                rendered.contains(&expected_line),
                "{role} misses `{expected_line}`:\n{rendered}"
            );
        } else {
            assert!(
                !rendered.contains("repo-notes.md"),
                "{role} unexpectedly includes repo-notes.md:\n{rendered}"
            );
        }
    }
    assert_eq!(fs::read_to_string(&expected_snapshot).unwrap(), notes_body);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&expected_snapshot)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o400, "studio repo-notes snapshot must be 0400");
    }
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

/// Neutral stand-ins for placeholder tokens (`<seq>`, `<json>`,
/// `<file>`, etc.) so readable prompt commands parse through
/// `args::parse`.
fn substitute_prompt_placeholder(token: &str) -> String {
    let mut out = token.to_string();
    for (needle, val) in [
        ("<seq>", "1"),
        ("<n>", "1"),
        ("<r>", "1"),
        ("<v>", "1"),
        ("<json>", "{}"),
        ("<file>", "/tmp/f"),
    ] {
        out = out.replace(needle, val);
    }
    out
}

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
            let words: Vec<String> = shell_words(cmd)
                .into_iter()
                .map(|w| substitute_prompt_placeholder(&w))
                .collect();
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

    // "What you may run" defers to the generated reference: the file
    // the stage writes carries it in place of the marker, and every
    // command it lists is inside the role's gate and every
    // pre-allowing runner covers it. The reference names commands in
    // their usage form, so a concrete line is built from each pick:
    // the parser checks the command and flags, and the pre-allow lists
    // check the runner would not stop at a prompt.
    let agents = crate::hwcli::agents_md();
    assert!(
        !agents.contains(crate::hwcli::MARKER),
        "the marker survived"
    );
    assert!(
        agents.contains("### Command reference"),
        "the agents file has no command reference"
    );
    let mut seen = 0;
    for pick in crate::hwcli::picks() {
        let prefix = pick.trim_end();
        assert!(
            agents.contains(prefix),
            "the reference misses `{prefix}`:\n{agents}"
        );
        // A concrete command line from the prefix: `status`, `records`
        // and the Hollywood verbs need only their prefix; `tell` and
        // `studio new` are pinned to what the gate allows.
        let concrete = match prefix {
            "backlot tell" => "backlot tell producer --file m.md",
            "backlot progress" => "backlot progress \"working\"",
            "backlot studio new" => {
                "backlot studio new --title T --repo /home/user/r --cast c.toml --dry-run"
            }
            "backlot studio cast set" => "backlot studio cast set --file c.toml --dry-run",
            "backlot hollywood propose" => {
                "backlot hollywood propose pause --studio s --task t1 --why x"
            }
            "backlot hollywood report" => "backlot hollywood report \"h\"",
            "backlot hollywood withdraw" => "backlot hollywood withdraw c1 --reason stale",
            // The guide's own reading forms: the listing, a topic and
            // a grep.
            "backlot guide" => "backlot guide guide --grep permission",
            _ => prefix,
        };
        let (_, cmd) = args::parse(&shell_words(
            concrete.strip_prefix("backlot ").unwrap_or(concrete),
        ))
        .unwrap_or_else(|e| panic!("`{concrete}` does not parse: {e:?}"));
        match cmd {
            args::Cmd::Status { .. }
            | args::Cmd::Records { .. }
            | args::Cmd::Hollywood(_)
            | args::Cmd::Guide { .. }
            | args::Cmd::Progress { .. } => {}
            args::Cmd::Tell { role, .. } => assert_eq!(role, "producer"),
            args::Cmd::StudioNew { dry_run, .. } => {
                assert!(dry_run, "`{concrete}` is not the dry run")
            }
            args::Cmd::StudioCast(args::CastCmd::Set { dry_run, .. }) => {
                assert!(dry_run, "`{concrete}` is not the dry run")
            }
            _ => panic!("`{concrete}` is outside Hollywood's gate"),
        }
        // `tell` is allowed as `tell producer`; `studio new` and `studio
        // cast set` as their `--dry-run` form; `progress` as `backlot
        // progress`.
        let allowed_form = match prefix {
            "backlot tell" => "backlot tell producer",
            "backlot progress" => "backlot progress",
            "backlot studio new" => "backlot studio new --dry-run",
            "backlot studio cast set" => "backlot studio cast set --dry-run",
            // `guide` is pre-allowed by its prefix, either form.
            "backlot guide" => "backlot guide",
            _ => concrete,
        };
        for (runner, rules) in [
            ("claude", &claude),
            ("cursor", &cursor),
            ("cmd", &cmd_runner),
        ] {
            assert!(
                covered(rules, allowed_form),
                "{runner}'s pre-allow misses `{allowed_form}`"
            );
        }
        seen += 1;
    }
    assert!(seen > 0, "the reference names no runnable command");
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
/// when no requirement is recorded yet. A first `tell` from the user
/// is read as the brief instead — the wizard can start a studio with
/// it, so the producer must not ask over it. The terminal stays the
/// conversation.
#[test]
fn the_producer_records_every_question_as_a_tell() {
    let text = template(RoleKind::Producer);
    assert!(
        text.contains("`backlot tell user --text <question> --ask`"),
        "{text}"
    );
    assert!(
        text.contains("`backlot tell user --text <note> --note`"),
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
    // W6: a first `tell` from the user is the requirement brief —
    // recorded before any question, not asked over.
    assert!(
        text.contains("is their brief — read it and record it as"),
        "{text}"
    );
    assert!(text.contains("Only when there is no such"), "{text}");
    assert!(
        text.contains("Ask the user only when the decision is theirs"),
        "{text}"
    );
}

/// Every prompt that instructs `tell user` (producer, director, writer)
/// shows both `--ask` for questions and `--note` for information that
/// contains a question mark.
#[test]
fn tell_user_prompts_show_both_ask_and_note() {
    for (kind, name) in [
        (RoleKind::Producer, "producer"),
        (RoleKind::Director, "director"),
        (RoleKind::Writer, "writer"),
    ] {
        let text = template(kind);
        assert!(
            text.contains("`backlot tell user --text <question> --ask`"),
            "{name}.md misses --ask form:\n{text}"
        );
        assert!(
            text.contains("`backlot tell user --text <note> --note`"),
            "{name}.md misses --note form:\n{text}"
        );
    }
}

/// The director is told a candidate is a kind of actor, one candidate
/// can play several tasks at once, independent tasks run in parallel,
/// and the actor cap bounds how many play — with the studio's own cap
/// filled in. The producer's dispatch line asks for that parallelism.
#[test]
fn the_director_prompt_teaches_parallel_actors() {
    let dir = tmp();
    let mut state = state_with_task();
    state.studio = Some(studio_config(Vec::new()));
    state.studio.as_mut().unwrap().max_actors = 3;
    let text = fs::read_to_string(
        prepare(
            &dir,
            "acme-shop",
            &start(RoleKind::Director, "director", None),
            &state,
        )
        .unwrap(),
    )
    .unwrap();
    for want in [
        "of worker, not a person",
        "several tasks at once",
        "run in parallel",
        "actor cap (`3` actors at once here)",
        "actors <live>/<cap>",
        "waits for",
    ] {
        assert!(text.contains(want), "director.md misses {want:?}:\n{text}");
    }
    // The producer's dispatch asks the director to spread the work.
    let producer = template(RoleKind::Producer);
    assert!(
        producer.contains("run in parallel"),
        "producer.md does not ask for parallelism:\n{producer}"
    );
}

/// The producer reports the studio ready and never finishes it: the
/// person finishes from the lot.
#[test]
fn the_producer_never_finishes_the_studio() {
    let text = template(RoleKind::Producer);
    assert!(
        text.contains("never run `backlot studio finish`"),
        "producer.md still offers the finish: {text}"
    );
    assert!(
        text.contains("Finishing is the person's alone"),
        "producer.md does not leave the finish to the person: {text}"
    );
    assert!(
        !text.contains("On their word"),
        "producer.md still finishes on the person's word: {text}"
    );
}

/// `backlot tell user` (`--ask` and `--note`) is covered by every
/// studio role's pre-allowed command rules for every runner with a
/// pre-allow list.
#[test]
fn the_producers_tell_user_is_preallowed() {
    for role in ["producer", "director", "writer", "actor-t1-c1"] {
        let claude = crate::runners::claude::allowed_tools("/state/studios/acme-shop", role, &[]);
        assert!(
            claude.split(',').any(|rule| rule == "Bash(backlot *)"),
            "{role}: {claude}"
        );
        for (runner, rules) in [
            ("cursor", crate::runners::cursor::allow_rules(role, &[])),
            ("cmd", crate::runners::cmd::allow_rules(role, &[])),
        ] {
            assert!(
                rules.iter().any(|rule| rule == "Shell(backlot)"),
                "{runner} {role}: {rules:?}"
            );
        }
        assert!(
            crate::runners::cline::allow_commands(role, &[])
                .iter()
                .any(|cmd| cmd == "backlot"),
            "cline {role}"
        );
    }
    for line in [
        "backlot tell user --text \"Should we proceed?\" --ask",
        "backlot tell user --text \"FYI? fixed\" --note",
    ] {
        assert_eq!(
            crate::guard_rules::refused_command_line(line),
            None,
            "{line}"
        );
    }
}

/// R7b prevention: every role prompt instructs roles never to ask
/// on their own screen, using `backlot ask` / `backlot tell` instead.
#[test]
fn prompts_forbid_on_screen_questions() {
    for (kind, name) in [
        (RoleKind::Producer, "producer"),
        (RoleKind::Writer, "writer"),
        (RoleKind::Director, "director"),
        (RoleKind::Actor, "actor"),
    ] {
        let text = template(kind);
        assert!(
            text.contains("Never ask on your own screen"),
            "{name}.md misses on-screen question rule"
        );
        assert!(
            text.contains("`backlot ask --file <file>`"),
            "{name}.md misses backlot ask"
        );
        assert!(
            text.contains("`backlot tell <role> --file <file>`"),
            "{name}.md misses backlot tell"
        );
    }
    assert!(HOLLYWOOD.contains("Never ask on your own screen"));
    assert!(HOLLYWOOD.contains("`backlot tell producer --file <file>`"));
}

/// D5: every role prompt instructs roles at each step boundary to run
/// `backlot progress "<what you are doing now>"`, and every runner's base
/// rules permit `backlot progress` for both studio roles and Hollywood.
#[test]
fn prompts_and_runners_cover_backlot_progress() {
    for (kind, name) in [
        (RoleKind::Producer, "producer"),
        (RoleKind::Writer, "writer"),
        (RoleKind::Director, "director"),
        (RoleKind::Actor, "actor"),
    ] {
        let text = template(kind);
        assert!(
            text.contains("`backlot progress \"<what you are doing now>\"`"),
            "{name}.md misses backlot progress sentence"
        );
        assert!(
            text.contains("step boundary"),
            "{name}.md misses step boundary wording"
        );
    }
    assert!(HOLLYWOOD.contains("`backlot progress \"<what you are doing now>\"`"));
    assert!(HOLLYWOOD.contains("step boundary"));

    for role in ["producer", "director", "writer", "actor-t1-c1"] {
        let claude = crate::runners::claude::allowed_tools("/state/studios/acme-shop", role, &[]);
        assert!(claude.contains("Bash(backlot *)"), "{role}: {claude}");
        assert!(
            crate::runners::cursor::allow_rules(role, &[])
                .iter()
                .any(|r| r == "Shell(backlot)"),
            "cursor {role}"
        );
        assert!(
            crate::runners::cmd::allow_rules(role, &[])
                .iter()
                .any(|r| r == "Shell(backlot)"),
            "cmd {role}"
        );
        assert!(
            crate::runners::cline::allow_commands(role, &[])
                .iter()
                .any(|r| r == "backlot"),
            "cline {role}"
        );
    }
    // Hollywood explicitly enumerates commands instead of allowing all `backlot`.
    assert!(crate::runners::claude::hollywood_commands().contains(&"backlot progress"));
    let hw_claude = crate::runners::claude::allowed_tools("/state/hollywood", "hollywood", &[]);
    assert!(
        hw_claude.contains("Bash(backlot progress *)"),
        "{hw_claude}"
    );
    assert!(
        crate::runners::cursor::allow_rules("hollywood", &[])
            .iter()
            .any(|r| r == "Shell(backlot progress)")
    );
    assert!(
        crate::runners::cmd::allow_rules("hollywood", &[])
            .iter()
            .any(|r| r == "Shell(backlot progress:*)")
    );
    assert!(
        crate::runners::cline::allow_commands("hollywood", &[])
            .iter()
            .any(|r| r == "backlot progress")
    );
    assert_eq!(
        crate::guard_rules::refused_command_line("backlot progress \"working\""),
        None
    );
}

/// Field notes 1126 §4: every role prompt — the four studio roles and
/// Hollywood — says backlot's own docs come from `backlot guide` and
/// never from a disk search. The guide verb is pre-allowed for every
/// role on the runners that carry a command allow list.
#[test]
fn every_prompt_reads_its_docs_from_the_guide() {
    for (kind, name) in [
        (RoleKind::Producer, "producer"),
        (RoleKind::Writer, "writer"),
        (RoleKind::Director, "director"),
        (RoleKind::Actor, "actor"),
    ] {
        let text = template(kind);
        assert!(
            text.contains("backlot guide <topic>"),
            "{name}.md does not point at the guide"
        );
        assert!(
            text.contains("Never search the"),
            "{name}.md does not forbid a disk search"
        );
        assert!(text.contains("disk for them"), "{name}.md");
    }
    assert!(HOLLYWOOD.contains("backlot guide <topic>"), "hollywood.md");
    assert!(
        HOLLYWOOD.to_lowercase().contains("search the disk"),
        "hollywood.md does not forbid a disk search"
    );

    // `backlot guide` is pre-allowed for each role on each runner that
    // has an allow list — a prompt that teaches it must not prompt.
    // Studio roles carry the `backlot *` family; Hollywood names it.
    for role in ["producer", "director", "writer", "actor-t1-c1", "hollywood"] {
        let claude = crate::runners::claude::allowed_tools("/state/studios/acme-shop", role, &[]);
        assert!(
            claude.contains("Bash(backlot guide *)") || claude.contains("Bash(backlot *)"),
            "claude {role}: {claude}"
        );
        assert!(
            crate::runners::cursor::allow_rules(role, &[])
                .iter()
                .any(|r| r == "Shell(backlot guide)" || r == "Shell(backlot)"),
            "cursor {role}"
        );
        assert!(
            crate::runners::cmd::allow_rules(role, &[])
                .iter()
                .any(|r| r == "Shell(backlot guide:*)" || r == "Shell(backlot)"),
            "cmd {role}"
        );
        assert!(
            crate::runners::cline::allow_commands(role, &[])
                .iter()
                .any(|r| r == "backlot guide" || r == "backlot"),
            "cline {role}"
        );
    }
}

/// Field notes 1126 §5: the shared read-only set reaches **every**
/// role on **every** runner with an allow list — the producer's and
/// writer's included — so a role's `git log` never asks a person.
#[test]
fn every_runner_pre_allows_the_read_only_set_for_every_role() {
    assert!(
        crate::runners::READ_ONLY.contains(&"backlot msg show"),
        "READ_ONLY misses `backlot msg show`"
    );
    for role in ["producer", "director", "writer", "actor-t1-c1"] {
        let claude = crate::runners::claude::allowed_tools("/state/studios/acme-shop", role, &[]);
        let cursor = crate::runners::cursor::allow_rules(role, &[]);
        let cmd = crate::runners::cmd::allow_rules(role, &[]);
        let cline = crate::runners::cline::allow_commands(role, &[]);
        let devin = crate::runners::devin::allow_rules();
        for cmd_name in crate::runners::READ_ONLY {
            assert!(
                claude.contains(&format!("Bash({cmd_name} *)")),
                "claude {role} misses `{cmd_name}`"
            );
            assert!(
                cursor.iter().any(|r| r == &format!("Shell({cmd_name})")),
                "cursor {role} misses `{cmd_name}`"
            );
            assert!(
                cmd.iter().any(|r| r == &format!("Shell({cmd_name}:*)")),
                "cmd {role} misses `{cmd_name}`"
            );
            assert!(
                cline.iter().any(|r| r == cmd_name),
                "cline {role} misses `{cmd_name}`"
            );
            assert!(
                devin.iter().any(|r| r == &format!("Exec({cmd_name})")),
                "devin {role} misses `{cmd_name}`"
            );
        }
    }
}

#[test]
fn prompts_carry_binding_seq_rules_and_msg_show() {
    let dir = tmp();
    for gate in [
        backlot_model::event::DesignGate::Off,
        backlot_model::event::DesignGate::Required,
    ] {
        let state = state_with_gate(gate);

        let actor_file = prepare(
            &dir,
            "acme-shop",
            &start(RoleKind::Actor, "actor-t1-c1", Some("t1")),
            &state,
        )
        .unwrap();
        let actor_text = fs::read_to_string(&actor_file).unwrap();
        assert!(
            actor_text.contains("`backlot msg show <seq>`"),
            "actor misses msg show ({gate:?})"
        );
        assert!(
            actor_text.contains("#<seq>") && actor_text.contains("ask the director"),
            "actor misses seq opening / disagreement rule ({gate:?}):\n{actor_text}"
        );

        let director_file = prepare(
            &dir,
            "acme-shop",
            &start(RoleKind::Director, "director", None),
            &state,
        )
        .unwrap();
        let director_text = fs::read_to_string(&director_file).unwrap();
        assert!(
            director_text.contains("`backlot msg show <seq>`"),
            "director misses msg show ({gate:?})"
        );
        assert!(
            director_text.contains("Binding: #<seq>")
                && director_text.contains("instead of retelling")
                && director_text.contains("person's answers")
                && director_text.contains("producer's supplements")
                && director_text.contains("signed design")
                && director_text.contains("findings")
                && director_text.contains("only one line"),
            "director misses Binding seq rule ({gate:?}):\n{director_text}"
        );

        let producer_file = prepare(
            &dir,
            "acme-shop",
            &start(RoleKind::Producer, "producer", None),
            &state,
        )
        .unwrap();
        let producer_text = fs::read_to_string(&producer_file).unwrap();
        assert!(
            producer_text.contains("`backlot msg show <seq>`"),
            "producer misses msg show ({gate:?})"
        );
        assert!(
            producer_text.contains("Binding: #<seq>"),
            "producer misses Binding seq rule ({gate:?}):\n{producer_text}"
        );

        let writer_file = prepare(
            &dir,
            "acme-shop",
            &start(RoleKind::Writer, "writer", None),
            &state,
        )
        .unwrap();
        let writer_text = fs::read_to_string(&writer_file).unwrap();
        assert!(
            writer_text.contains("`backlot msg show <seq>`"),
            "writer misses msg show ({gate:?})"
        );
        assert!(
            writer_text.contains("Binding: #<seq>"),
            "writer misses Binding seq rule ({gate:?}):\n{writer_text}"
        );
    }
}

/// Nothing that writes is in the shared read-only set: no `git add`,
/// `git commit`, `git push`, `git merge`, and no bare `git branch`
/// family whose prefix would also reach `git branch -D`.
#[test]
fn the_read_only_set_holds_no_writes() {
    for bad in [
        "git add",
        "git commit",
        "git push",
        "git merge",
        "git branch",
        "git reset",
        "git checkout",
        "rm",
    ] {
        assert!(
            !crate::runners::READ_ONLY.contains(&bad),
            "the read-only set holds `{bad}`"
        );
    }
    assert!(crate::runners::READ_ONLY.contains(&"git branch --list"));
    assert!(crate::runners::READ_ONLY.contains(&"git branch --show-current"));
}

fn state_with_gate(design_gate: backlot_model::event::DesignGate) -> StudioState {
    let mut state = state_with_task();
    let mut cfg = studio_config(Vec::new());
    cfg.design_gate = design_gate;
    state.studio = Some(cfg);
    state
}

#[test]
fn actor_template_and_render_stay_byte_identical_across_design_gates() {
    let raw = template(RoleKind::Actor);
    assert_eq!(raw.len(), 3618, "actor.md size must stay byte-identical");
    assert!(raw.ends_with("asking to open a PR.\n{/writable}\n"));
    assert!(!raw.contains("design_gate"));
    assert!(!raw.contains("backlot design"));

    let dir = tmp();
    let st = start(RoleKind::Actor, "actor-t1-c1", Some("t1"));
    let off_state = state_with_gate(backlot_model::event::DesignGate::Off);
    let req_state = state_with_gate(backlot_model::event::DesignGate::Required);

    let off_file = prepare(&dir, "acme-shop", &st, &off_state).unwrap();
    let off_text = fs::read(&off_file).unwrap();
    let req_file = prepare(&dir, "acme-shop", &st, &req_state).unwrap();
    let req_text = fs::read(&req_file).unwrap();
    assert_eq!(off_text, req_text, "actor prompt must ignore design_gate");
}

#[test]
fn design_gate_required_renders_placeholders_sections_and_commands() {
    let dir = tmp();
    let state = state_with_gate(backlot_model::event::DesignGate::Required);
    let design_v2 = dir.join("design/v2.md").to_string_lossy().into_owned();

    for (kind, role) in [
        (RoleKind::Producer, "producer"),
        (RoleKind::Writer, "writer"),
        (RoleKind::Director, "director"),
    ] {
        let raw = template(kind);
        let section = raw
            .split("{#design_gate}")
            .nth(1)
            .and_then(|rest| rest.split("{/design_gate}").next())
            .unwrap_or_else(|| panic!("{role}.md must contain a {{ #design_gate }} block"));
        assert!(
            section.len() <= 2500,
            "{role}.md design section grew too large ({} bytes)",
            section.len()
        );

        let st = start(kind, role, None);
        let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains('{'), "{role} left a placeholder: {text}");
        assert!(
            text.contains(&design_v2),
            "{role} misses {design_v2}: {text}"
        );

        match kind {
            RoleKind::Producer => {
                assert!(!text.contains("wherdr"), "producer must not mention wherdr");
                for want in [
                    "## Opening design discussion (`design_gate = required`)",
                    "`backlot design open`",
                    "`backlot design open --suggest-skip --reason \"<why>\"`",
                    "`backlot design write --slot summary --file <file> --question <json>`",
                    "`backlot design show`",
                    "`backlot design note --from user --text \"...\"`",
                    "`backlot design next`",
                    "Never record the same words",
                    "never run `backlot design sign`",
                    "press `a` on the DECISION card in the Call Sheet or",
                    "`backlot tell user --text <note> --note`",
                ] {
                    assert!(text.contains(want), "producer misses `{want}`:\n{text}");
                }
            }
            RoleKind::Writer => {
                for want in [
                    "## Opening design proposal",
                    "`backlot design write --slot proposal --file <file>`",
                    "`backlot design supplement --file <file> --refs <seq>`",
                ] {
                    assert!(text.contains(want), "writer misses `{want}`:\n{text}");
                }
            }
            RoleKind::Director => {
                for want in [
                    "## Opening buildability review",
                    "`backlot design write --slot buildability --file <file>`",
                    "`backlot design supplement --file <file> --refs <seq>`",
                ] {
                    assert!(text.contains(want), "director misses `{want}`:\n{text}");
                }
            }
            RoleKind::Actor => unreachable!(),
        }
    }
}

#[test]
fn design_gate_off_omits_design_section_byte_identically() {
    let dir = tmp();
    let state = state_with_gate(backlot_model::event::DesignGate::Off);

    for (kind, role, baseline_len, baseline_tail) in [
        (
            RoleKind::Producer,
            "producer",
            5505usize,
            "merely to send them there.\n",
        ),
        (
            RoleKind::Writer,
            "writer",
            3744usize,
            "(`backlot tell user --text <note> --note`).\n",
        ),
        (
            RoleKind::Director,
            "director",
            8380usize,
            "is killed for you.\n",
        ),
    ] {
        let raw = template(kind);
        assert!(
            raw.contains("{#design_gate}"),
            "{role}.md missing {{#design_gate}}"
        );
        assert!(
            raw.contains("{/design_gate}"),
            "{role}.md missing {{/design_gate}}"
        );
        let base = raw.split("{#design_gate}").next().unwrap();
        assert_eq!(
            base.len(),
            baseline_len,
            "{role}.md baseline prefix length shifted"
        );
        assert!(
            base.ends_with(baseline_tail),
            "{role}.md baseline tail shifted"
        );

        let fields = BTreeMap::from([
            ("studio".into(), "acme-shop".into()),
            ("studio_dir".into(), dir.to_string_lossy().into_owned()),
            ("role".into(), role.into()),
            ("task".into(), String::new()),
            ("title".into(), String::new()),
            ("acceptance".into(), String::new()),
            (
                "req_file".into(),
                dir.join("req/v2.md").to_string_lossy().into_owned(),
            ),
            ("req_version".into(), "2".into()),
            ("work".into(), dir.to_string_lossy().into_owned()),
            ("branch".into(), "backlot/acme-shop/integration".into()),
            ("checks".into(), "cargo test".into()),
            ("actor_cap".into(), "3".into()),
            ("design_gate".into(), "off".into()),
            (
                "design_file".into(),
                dir.join("design/v2.md").to_string_lossy().into_owned(),
            ),
            ("repo_notes".into(), String::new()),
        ]);
        assert_eq!(
            render(raw, &fields),
            render(base, &fields),
            "{role}.md with design_gate=off must render byte-identical to baseline"
        );

        let st = start(kind, role, None);
        let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains('{'), "{role} left a brace: {text}");
        assert!(
            !text.contains("Opening design"),
            "{role} leaked design section"
        );
        assert!(!text.contains("backlot design"), "{role} leaked design cmd");
        assert!(text.ends_with(baseline_tail), "{role} tail changed: {text}");
    }
}

#[test]
fn design_gate_required_pre_requirement_avoids_empty_backticks_and_v0() {
    let dir = tmp();
    let mut state = state_with_gate(backlot_model::event::DesignGate::Required);
    state.requirement_version = 0;
    state.tasks.clear();
    let design_dir = dir.join("design").to_string_lossy().into_owned();
    for kind in [RoleKind::Producer, RoleKind::Director] {
        let st = start(kind, kind.name(), None);
        let file = prepare(&dir, "acme-shop", &st, &state).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains('{'), "{kind:?} left a placeholder: {text}");
        assert!(!text.contains("``"), "{kind:?} has empty backticks: {text}");
        let outside_paths = text.replace(&dir.to_string_lossy().into_owned(), "");
        assert!(!outside_paths.contains("v0"), "{kind:?} shows v0: {text}");
        assert!(
            text.contains(&design_dir),
            "{kind:?} misses the design folder: {text}"
        );
    }
}

#[test]
fn director_and_writer_prompts_teach_task_brief_template_canary_and_spine() {
    for (kind, role) in [
        (RoleKind::Director, "director"),
        (RoleKind::Writer, "writer"),
    ] {
        let raw = template(kind);
        assert!(
            raw.contains("## Task brief template"),
            "{role}.md misses task brief template heading"
        );
        for needle in [
            "Goal",
            "Scope",
            "Decided contracts",
            "Explicit No-Gos",
            "Open to actor",
            "Required verification",
            "--depends-on",
            "Architecture Spine (AD-1..k)",
        ] {
            assert!(raw.contains(needle), "{role}.md misses `{needle}`");
        }
        let template_block = raw
            .split("## Task brief template")
            .nth(1)
            .unwrap_or_default()
            .split("\n\n")
            .take(2)
            .collect::<Vec<_>>()
            .join("\n\n");
        assert!(
            template_block.lines().count() <= 25,
            "{role}.md task brief template exceeds 25 lines:\n{template_block}"
        );
    }

    let director = template(RoleKind::Director);
    assert!(
        director.contains("`backlot decide --id AD-<n> --file <file> --notify active-actors`"),
        "director.md misses `backlot decide`"
    );
    assert!(
        director.contains(
            "`backlot repo-note propose --id <slug> --text \"<rule>\" --why \"<evidence>\"`"
        ),
        "director.md misses `backlot repo-note propose`"
    );
}

#[test]
fn actor_prompt_includes_optional_siblings_discoveries_section() {
    let raw = template(RoleKind::Actor);
    assert!(
        raw.contains("## Decisions / Discoveries for siblings"),
        "actor.md misses optional sibling discoveries guidance"
    );
}

#[test]
fn hollywood_allow_list_contains_withdraw_and_cast_dry_run_for_every_runner() {
    for needed in [
        "backlot hollywood withdraw",
        "backlot studio cast set --dry-run",
    ] {
        assert!(
            crate::runners::claude::hollywood_commands().contains(&needed),
            "claude misses `{needed}`"
        );
        let hw_claude = crate::runners::claude::allowed_tools("/state/hollywood", "hollywood", &[]);
        assert!(
            hw_claude.contains(&format!("Bash({needed} *)")),
            "claude allowed_tools misses `{needed}`: {hw_claude}"
        );
        assert!(
            crate::runners::cmd::allow_rules("hollywood", &[])
                .iter()
                .any(|r| r == &format!("Shell({needed}:*)")),
            "cmd misses `{needed}`"
        );
        assert!(
            crate::runners::cursor::allow_rules("hollywood", &[])
                .iter()
                .any(|r| r == &format!("Shell({needed})")),
            "cursor misses `{needed}`"
        );
        assert!(
            crate::runners::cline::allow_commands("hollywood", &[])
                .iter()
                .any(|r| r == needed),
            "cline misses `{needed}`"
        );
    }
    assert!(
        HOLLYWOOD.contains("backlot hollywood withdraw"),
        "hollywood.md misses withdraw guidance"
    );
    assert!(
        crate::hwcli::generate().contains("backlot hollywood withdraw"),
        "hwcli reference misses withdraw"
    );
}
