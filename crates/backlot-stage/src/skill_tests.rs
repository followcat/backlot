//! `backlot skill`: the agent table, install/uninstall/status against a
//! temp `HOME` per agent, the stale refresh on a version change, and the
//! two truth tests — every `backlot` example in the SKILL.md parses with
//! the real parser, and its person-only block is `crate::person_only`'s
//! list.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::args::{self, Cmd};
use crate::cli::Env;
use crate::skill;
use crate::test_support::TestDir;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A temp home with an empty `PATH`; `dirs` are created, `bins` are
/// executable files placed on `PATH`.
fn home_env(dir: &TestDir, dirs: &[&str], bins: &[&str]) -> Env {
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    for d in dirs {
        fs::create_dir_all(home.join(d)).unwrap();
    }
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    for b in bins {
        let path = bin.join(b);
        fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let mut vars = BTreeMap::new();
    vars.insert("HOME".into(), home.to_string_lossy().into_owned());
    vars.insert(
        "XDG_STATE_HOME".into(),
        dir.join("state").to_string_lossy().into_owned(),
    );
    vars.insert("PATH".into(), bin.to_string_lossy().into_owned());
    Env { vars }
}

fn manifest_path(env: &Env) -> PathBuf {
    crate::store::root_dir(&env.vars).join("skills.json")
}

fn skill_file(env: &Env, dir: &str) -> PathBuf {
    PathBuf::from(env.get("HOME").unwrap())
        .join(dir.trim_start_matches("~/"))
        .join("backlot")
        .join("SKILL.md")
}

fn installed(env: &Env, dir: &str) -> String {
    fs::read_to_string(skill_file(env, dir)).unwrap_or_default()
}

/// The table the brief fixed — one row per agent.
#[test]
fn the_agent_table_is_the_agreed_one() {
    let table: Vec<(&str, &str)> = crate::skill_agents::AGENTS
        .iter()
        .map(|a| (a.name, a.skills_dir))
        .collect();
    assert_eq!(
        table,
        vec![
            ("claude-code", "~/.claude/skills"),
            ("codex", "~/.agents/skills"),
            ("cline", "~/.agents/skills"),
            ("pi", "~/.agents/skills"),
            ("cursor", "~/.cursor/skills"),
            ("devin", "~/.config/devin/skills"),
            ("command-code", "~/.commandcode/skills"),
            ("opencode", "~/.config/opencode/skills"),
            ("antigravity", "~/.gemini/config/skills"),
            ("gemini-cli", "~/.gemini/skills"),
        ]
    );
}

/// The generated copy is the compiled file plus the marker, and only a
/// marked file counts as backlot's.
#[test]
fn the_written_file_carries_the_marker() {
    let text = skill::generated("1.2.3");
    assert!(text.starts_with("---\nname: backlot\n"));
    assert!(text.contains("metadata:\n  generated-by: backlot 1.2.3\n"));
    assert!(text.contains("<!-- person-only:start -->"));
    // The frontmatter still closes before the body.
    let (front, body) = text
        .strip_prefix("---\n")
        .unwrap()
        .split_once("\n---\n")
        .unwrap();
    assert!(front.contains("description:"), "{front}");
    assert!(
        body.trim_start().starts_with("# backlot"),
        "{}",
        &body[..20]
    );
}

/// A detected agent — config dir or binary on `PATH` — gets the file;
/// an undetected one does not.
#[test]
fn install_writes_detected_agents_only() {
    let dir = TestDir::new("backlot-skill-install");
    let env = home_env(&dir, &[".claude", ".agents"], &["opencode"]);
    let out = skill::run(
        crate::args::SkillCmd::Install {
            agents: Vec::new(),
            dry_run: false,
            force: false,
        },
        &env,
    )
    .unwrap();
    assert!(installed(&env, "~/.claude/skills").contains("name: backlot"));
    assert!(installed(&env, "~/.agents/skills").contains("name: backlot"));
    // `.agents` detects codex, cline and pi too; one file serves all.
    assert!(out.contains("(claude-code)"), "{out}");
    assert!(out.contains("(codex, cline, pi)"), "{out}");
    assert!(out.contains("(opencode)"), "{out}");
    // cursor was neither detected nor named.
    assert!(!skill_file(&env, "~/.cursor/skills").exists());
    // The manifest lists the written paths.
    let manifest = fs::read_to_string(manifest_path(&env)).unwrap();
    assert!(manifest.contains("skills/backlot/SKILL.md"), "{manifest}");
}

/// Shared directories get exactly one file, whatever the agent list.
#[test]
fn agents_sharing_a_directory_share_one_file() {
    let dir = TestDir::new("backlot-skill-shared");
    let env = home_env(&dir, &[], &[]);
    skill::run(
        crate::args::SkillCmd::Install {
            agents: vec!["codex".into(), "cline".into(), "pi".into()],
            dry_run: false,
            force: false,
        },
        &env,
    )
    .unwrap();
    let mut found = Vec::new();
    for agent in crate::skill_agents::AGENTS {
        let f = crate::skill_agents::skill_file(agent, &env);
        if f.exists() && !found.contains(&f) {
            found.push(f);
        }
    }
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].ends_with(".agents/skills/backlot/SKILL.md"));
}

/// A foreign `backlot/SKILL.md` is never overwritten without --force.
#[test]
fn a_foreign_file_is_left_alone_unless_forced() {
    let dir = TestDir::new("backlot-skill-foreign");
    let env = home_env(&dir, &[".claude"], &[]);
    let file = skill_file(&env, "~/.claude/skills");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "the person's own skill\n").unwrap();

    let out = skill::run(
        crate::args::SkillCmd::Install {
            agents: vec!["claude-code".into()],
            dry_run: false,
            force: false,
        },
        &env,
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "the person's own skill\n"
    );
    assert!(
        out.contains("skipped") && out.contains("use --force"),
        "{out}"
    );

    skill::run(
        crate::args::SkillCmd::Install {
            agents: vec!["claude-code".into()],
            dry_run: false,
            force: true,
        },
        &env,
    )
    .unwrap();
    assert!(fs::read_to_string(&file).unwrap().contains("name: backlot"));
}

/// A dry run prints the plan and writes nothing.
#[test]
fn a_dry_run_writes_nothing() {
    let dir = TestDir::new("backlot-skill-dry");
    let env = home_env(&dir, &[".claude"], &[]);
    let out = skill::run(
        crate::args::SkillCmd::Install {
            agents: Vec::new(),
            dry_run: true,
            force: false,
        },
        &env,
    )
    .unwrap();
    assert!(out.contains("would write"), "{out}");
    assert!(out.contains("would be:"), "{out}");
    assert!(!skill_file(&env, "~/.claude/skills").exists());
    assert!(!manifest_path(&env).exists());
}

/// A second install is a no-op that says so.
#[test]
fn a_reinstall_is_idempotent() {
    let dir = TestDir::new("backlot-skill-again");
    let env = home_env(&dir, &[".claude"], &[]);
    let install = || {
        skill::run(
            crate::args::SkillCmd::Install {
                agents: Vec::new(),
                dry_run: false,
                force: false,
            },
            &env,
        )
        .unwrap()
    };
    install();
    let before = installed(&env, "~/.claude/skills");
    let out = install();
    assert!(out.contains("current"), "{out}");
    assert_eq!(installed(&env, "~/.claude/skills"), before);
}

/// `status` tells current, stale and foreign copies apart.
#[test]
fn status_names_each_copy() {
    let dir = TestDir::new("backlot-skill-status");
    let env = home_env(&dir, &[".claude", ".cursor"], &[]);
    // Current.
    fs::create_dir_all(skill_file(&env, "~/.claude/skills").parent().unwrap()).unwrap();
    fs::write(
        skill_file(&env, "~/.claude/skills"),
        skill::generated(VERSION),
    )
    .unwrap();
    // Stale.
    fs::create_dir_all(skill_file(&env, "~/.cursor/skills").parent().unwrap()).unwrap();
    fs::write(
        skill_file(&env, "~/.cursor/skills"),
        skill::generated("0.0.1"),
    )
    .unwrap();
    // Foreign.
    let devin = skill_file(&env, "~/.config/devin/skills");
    fs::create_dir_all(devin.parent().unwrap()).unwrap();
    fs::write(&devin, "mine\n").unwrap();

    let out = skill::run(crate::args::SkillCmd::Status { json: false }, &env).unwrap();
    assert!(
        out.contains("current ~/.claude/skills/backlot/SKILL.md"),
        "{out}"
    );
    assert!(
        out.contains("stale   ~/.cursor/skills/backlot/SKILL.md"),
        "{out}"
    );
    assert!(
        out.contains("foreign ~/.config/devin/skills/backlot/SKILL.md"),
        "{out}"
    );
    assert!(out.contains("refresh stale copies"), "{out}");
}

/// A new version rewrites the stale copy the manifest lists — and only
/// that: a missing path is not recreated.
#[test]
fn a_new_version_refreshes_the_manifest_copies() {
    let dir = TestDir::new("backlot-skill-refresh");
    let env = home_env(&dir, &[".claude"], &[]);
    let file = skill_file(&env, "~/.claude/skills");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, skill::generated("0.0.1")).unwrap();
    let gone = dir.join("gone/skills/backlot/SKILL.md");
    let manifest = serde_json::json!({
        "version": "0.0.1",
        "paths": [&file, &gone],
    });
    fs::create_dir_all(manifest_path(&env).parent().unwrap()).unwrap();
    fs::write(manifest_path(&env), manifest.to_string()).unwrap();

    let rewritten = skill::refresh(&env);
    assert_eq!(rewritten, vec![file.clone()]);
    assert!(
        installed(&env, "~/.claude/skills").contains(&format!("generated-by: backlot {VERSION}"))
    );
    assert!(!gone.exists(), "refresh must never create a place");
    let kept: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(manifest_path(&env)).unwrap()).unwrap();
    assert_eq!(kept["paths"].as_array().unwrap().len(), 1, "{kept}");
}

/// Uninstall removes backlot's copies and leaves a foreign one.
#[test]
fn uninstall_removes_only_our_copies() {
    let dir = TestDir::new("backlot-skill-uninstall");
    let env = home_env(&dir, &[".claude", ".cursor"], &[]);
    skill::run(
        crate::args::SkillCmd::Install {
            agents: vec!["claude-code".into(), "cursor".into()],
            dry_run: false,
            force: false,
        },
        &env,
    )
    .unwrap();
    let foreign = skill_file(&env, "~/.cursor/skills");
    fs::write(&foreign, "mine\n").unwrap();

    let out = skill::run(crate::args::SkillCmd::Uninstall, &env).unwrap();
    assert!(
        out.contains("removed ~/.claude/skills/backlot/SKILL.md"),
        "{out}"
    );
    assert!(
        out.contains("left ~/.cursor/skills/backlot/SKILL.md"),
        "{out}"
    );
    assert!(!skill_file(&env, "~/.claude/skills").exists());
    assert_eq!(fs::read_to_string(&foreign).unwrap(), "mine\n");
}

/// `status` covers all five states against one temp HOME, and
/// `--json` is exactly the published row model.
#[test]
fn status_covers_every_state_and_json() {
    let dir = TestDir::new("backlot-skill-states");
    let env = home_env(
        &dir,
        &[
            ".claude",
            ".cursor",
            ".config/devin",
            ".commandcode",
            ".agents",
        ],
        &[],
    );
    fs::create_dir_all(skill_file(&env, "~/.claude/skills").parent().unwrap()).unwrap();
    fs::write(
        skill_file(&env, "~/.claude/skills"),
        skill::generated(VERSION),
    )
    .unwrap();
    fs::create_dir_all(skill_file(&env, "~/.cursor/skills").parent().unwrap()).unwrap();
    fs::write(
        skill_file(&env, "~/.cursor/skills"),
        skill::generated("0.0.1"),
    )
    .unwrap();
    fs::create_dir_all(skill_file(&env, "~/.config/devin/skills").parent().unwrap()).unwrap();
    fs::write(skill_file(&env, "~/.config/devin/skills"), "mine\n").unwrap();
    // `.commandcode` is detected but has no copy: missing. `.agents`
    // detects the shared codex/cline/pi rows too, so give them a copy
    // and leave opencode absent by non-detection.
    fs::create_dir_all(skill_file(&env, "~/.agents/skills").parent().unwrap()).unwrap();
    fs::write(
        skill_file(&env, "~/.agents/skills"),
        skill::generated(VERSION),
    )
    .unwrap();

    let out = skill::run(crate::args::SkillCmd::Status { json: false }, &env).unwrap();
    for word in ["current", "stale", "foreign", "missing", "absent"] {
        assert!(out.contains(word), "{word} missing from {out}");
    }
    let json = skill::run(crate::args::SkillCmd::Status { json: true }, &env).unwrap();
    let report: backlot_model::summary::AgentSkillSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(report.version, VERSION);
    assert_eq!(report.rows.len(), crate::skill_agents::AGENTS.len());
    let state = |name: &str| {
        report
            .rows
            .iter()
            .find(|row| row.agent == name)
            .unwrap()
            .state
    };
    use backlot_model::summary::SkillState;
    assert_eq!(state("claude-code"), SkillState::Current);
    assert_eq!(state("cursor"), SkillState::Stale);
    assert_eq!(state("devin"), SkillState::Foreign);
    assert_eq!(state("command-code"), SkillState::Missing);
    assert_eq!(state("opencode"), SkillState::Absent);
}

/// The shared-machine request path can uninstall exactly the rows it
/// names; foreign copies still stay put.
#[test]
fn targeted_uninstall_removes_only_named_rows() {
    let dir = TestDir::new("backlot-skill-targeted-uninstall");
    let env = home_env(&dir, &[".claude", ".cursor"], &[]);
    skill::run(
        crate::args::SkillCmd::Install {
            agents: vec!["claude-code".into(), "cursor".into()],
            dry_run: false,
            force: false,
        },
        &env,
    )
    .unwrap();
    let out = skill::uninstall_agents(&["claude-code".into()], &env).unwrap();
    assert!(
        out.contains("removed ~/.claude/skills/backlot/SKILL.md"),
        "{out}"
    );
    assert!(!skill_file(&env, "~/.claude/skills").exists());
    assert!(skill_file(&env, "~/.cursor/skills").exists());
}

/// `show` prints the compiled skill.
#[test]
fn show_prints_the_skill() {
    let dir = TestDir::new("backlot-skill-show");
    let env = home_env(&dir, &[], &[]);
    let out = skill::run(crate::args::SkillCmd::Show, &env).unwrap();
    assert_eq!(out, skill::SKILL);
    assert!(out.starts_with("---\nname: backlot\n"));
}

/// An unknown `--agent` name is a usage error that lists the table.
#[test]
fn an_unknown_agent_is_refused() {
    let dir = TestDir::new("backlot-skill-unknown");
    let env = home_env(&dir, &[], &[]);
    let err = skill::run(
        crate::args::SkillCmd::Install {
            agents: vec!["nope".into()],
            dry_run: false,
            force: false,
        },
        &env,
    )
    .unwrap_err();
    assert!(
        format!("{err:?}").contains("unknown agent 'nope'"),
        "{err:?}"
    );
    assert!(format!("{err:?}").contains("claude-code"), "{err:?}");
}

/// `--agent a,b` parses to both names; `--dry-run` and `--force` land.
#[test]
fn the_install_flags_parse() {
    let (common, cmd) = args::parse(&[
        "--via".into(),
        "claude-code".into(),
        "skill".into(),
        "install".into(),
        "--agent".into(),
        "codex,cline".into(),
        "--dry-run".into(),
        "--force".into(),
    ])
    .unwrap();
    assert_eq!(common.via.as_deref(), Some("claude-code"));
    assert_eq!(
        cmd.skill(),
        Some(&crate::args::SkillCmd::Install {
            agents: vec!["codex".into(), "cline".into()],
            dry_run: true,
            force: true,
        })
    );
}

/// Every `backlot ...` example in the SKILL.md — fenced or inline, in
/// the person-only block too — parses with the real argument parser.
#[test]
fn every_skill_command_parses() {
    let commands = skill_commands(skill::SKILL);
    assert!(
        commands.len() >= 20,
        "only {} examples found",
        commands.len()
    );
    for line in &commands {
        let words: Vec<String> = crate::docs_tests::shell_words(line)
            .into_iter()
            .skip(1)
            .map(|w| crate::docs_tests::substitute(&w))
            .collect();
        for w in &words {
            assert!(!w.contains('<'), "`{line}` keeps a placeholder");
        }
        assert!(args::parse(&words).is_ok(), "`{line}` does not parse");
    }
}

/// The person-only block is the crate's list, verbatim.
#[test]
fn the_person_only_block_is_the_crates_list() {
    let block = between_markers(skill::SKILL).expect("SKILL.md has no person-only block");
    let expected: Vec<String> = crate::person_only::ENTRIES
        .iter()
        .map(|e| format!("- {}", e.bullet))
        .collect();
    let found: Vec<String> = block
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    assert_eq!(
        found, expected,
        "the skill's person-only block drifted from person_only::ENTRIES"
    );
}

/// The skill never teaches an outside agent to forge a role: every
/// write example either carries `--as user` or no `--as` at all, and a
/// read carries none.
#[test]
fn every_example_acts_as_the_person() {
    let mut writes = 0;
    for line in skill_commands(skill::SKILL) {
        let words: Vec<String> = crate::docs_tests::shell_words(&line)
            .into_iter()
            .skip(1)
            .map(|w| crate::docs_tests::substitute(&w))
            .collect();
        let seat = words.iter().position(|w| w == "--as");
        if let Some(i) = seat {
            assert_eq!(
                words.get(i + 1).map(String::as_str),
                Some("user"),
                "`{line}` acts as someone other than the person"
            );
        }
        if is_read(&command_path(&words)) {
            assert!(seat.is_none(), "`{line}` is a read and takes no --as");
        } else {
            writes += 1;
        }
    }
    assert!(writes > 0, "no write example found");
}

/// The skill goes where the guide topic and the install path say it is.
#[test]
fn the_guide_topic_is_the_same_file() {
    let out = crate::guide::run(Some("agents"), None).unwrap();
    assert_eq!(out, skill::SKILL);
}

/// The command words after any leading common flags.
fn command_path(words: &[String]) -> Vec<&str> {
    let mut i = 0;
    while i < words.len() {
        let w = words[i].as_str();
        if matches!(
            w,
            "--studio" | "--as" | "--task" | "--session" | "--remote" | "--server" | "--via"
        ) {
            i += 2;
            continue;
        }
        if w.starts_with("--") {
            i += 1;
            continue;
        }
        break;
    }
    words[i.min(words.len())..]
        .iter()
        .map(String::as_str)
        .collect()
}

/// A read-only command: it takes no `--as` and moves nothing.
fn is_read(path: &[&str]) -> bool {
    match path.first().copied() {
        Some("status" | "records" | "guide" | "runners" | "repos" | "metrics" | "digest") => true,
        Some("studio") => path.get(1).copied() == Some("list"),
        Some("msg") => path.get(1).copied() == Some("show"),
        Some("repo-note") => path.get(1).copied() == Some("show"),
        Some("card") => path.get(1).copied() == Some("list"),
        Some("design") => path.get(1).copied() == Some("show"),
        Some("report") => path.get(1).copied() == Some("show"),
        Some("skill") => matches!(path.get(1).copied(), Some("status" | "show")),
        _ => false,
    }
}

fn between_markers(text: &str) -> Option<String> {
    let start = text.find("<!-- person-only:start -->")?;
    let rest = &text[start + "<!-- person-only:start -->".len()..];
    let end = rest.find("<!-- person-only:end -->")?;
    Some(rest[..end].to_string())
}

/// Every `backlot ` command in fenced blocks and inline code spans.
fn skill_commands(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            if let Some(cmd) = trimmed.strip_prefix("backlot ") {
                out.push(format!("backlot {cmd}"));
            }
            continue;
        }
        // Inline spans on a prose line.
        let mut span = String::new();
        let mut inside = false;
        for c in line.chars() {
            if c == '`' {
                if inside && span.starts_with("backlot ") {
                    out.push(span.clone());
                }
                span.clear();
                inside = !inside;
            } else if inside {
                span.push(c);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
impl Cmd {
    /// The `SkillCmd` inside a `Cmd::Skill`, for readable assertions.
    fn skill(&self) -> Option<&crate::args::SkillCmd> {
        match self {
            Cmd::Skill(cmd) => Some(cmd),
            _ => None,
        }
    }
}

/// `--via` records `agent:<name>` on the user's event, `msg show`
/// prints `user (via claude-code)`, and the summary carries the agent.
#[test]
fn via_is_recorded_and_shown() {
    use crate::cli_testkit::{CAST_TOML, fixture, ok, scratch};

    let (s, env) = scratch();
    let cast = fixture(&s, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    ok(
        &env,
        "tell producer --text hello --as user --via claude-code",
    );
    let dir = crate::store::studio_dir(&s.dir, "acme-shop");
    let (events, _) = crate::store::load_events(&dir).unwrap();
    let tell = events
        .iter()
        .find(|e| matches!(e.kind, backlot_model::EventKind::MessageQueued { .. }))
        .expect("tell was recorded");
    assert_eq!(tell.by.role, "user");
    assert_eq!(tell.by.via.as_deref(), Some("agent:claude-code"));
    assert_eq!(tell.by.label(), "user (via claude-code)");

    let shown = ok(&env, &format!("msg show {}", tell.seq));
    assert!(
        shown.contains("user (via claude-code) -> producer"),
        "{shown}"
    );

    let state = backlot_model::state::replay(&events);
    let exchange = backlot_model::summary::studio_exchange(&state, &BTreeMap::new());
    let turn = exchange.iter().find(|t| t.seq == tell.seq).unwrap();
    assert_eq!(turn.via.as_deref(), Some("claude-code"));
}

/// A name that could not be told apart from a role is a usage error,
/// and a role's own event never carries the person's agent.
#[test]
fn a_bad_via_is_refused_and_a_role_keeps_none() {
    use crate::cli_testkit::{CAST_TOML, fixture, ok, scratch, usage};

    let (s, env) = scratch();
    let cast = fixture(&s, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let err = usage(&env, "tell producer --text hello --as user --via Claude");
    assert!(err.contains("bad via 'Claude'"), "{err}");

    ok(
        &env,
        "tell producer --text hello --as director --via claude-code",
    );
    let dir = crate::store::studio_dir(&s.dir, "acme-shop");
    let (events, _) = crate::store::load_events(&dir).unwrap();
    let tell = events
        .iter()
        .find(|e| e.by.role == "director")
        .expect("director's tell was recorded");
    assert_eq!(tell.by.via, None);
}
