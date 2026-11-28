//! Remembered grants land in each runner's measured native form.

use std::collections::BTreeMap;
use std::path::PathBuf;

use backlot_model::event::{PermitKind, PermitRecord, PermitScope};

use super::{Launch, Prep, Session, apply_permits};

fn permit(kind: PermitKind, rule: &str) -> PermitRecord {
    PermitRecord {
        id: "p2".into(),
        scope: PermitScope::Role,
        role: Some("actor-alice".into()),
        kind,
        rule: rule.into(),
        outward: false,
        from: 1,
        runner: "fake".into(),
        seq: 2,
        ts_ms: 1,
    }
}

fn launch(prep: Prep) -> Launch {
    Launch {
        argv: vec!["agent".into()],
        env: BTreeMap::new(),
        session: Session::PostLaunch,
        hooks: None,
        prep: Some(prep),
    }
}

#[test]
fn apply_permits_writes_native_shell_and_edit_forms() {
    let permits = vec![
        permit(PermitKind::Shell, "git status"),
        permit(PermitKind::Edit, "/home/user/repo/src/a.rs"),
    ];

    let mut cursor = launch(Prep::Cursor(super::cursor::CursorPrep {
        root: PathBuf::from("/tmp/cursor"),
        project: "p".into(),
        allow: Vec::new(),
        global_mcp: PathBuf::from("/tmp/mcp.json"),
        readonly: false,
        hooks: PathBuf::from("/tmp/work/.cursor/hooks.json"),
        signals: PathBuf::from("/tmp/signals.jsonl"),
    }));
    apply_permits(&mut cursor, "cursor", &permits);
    let Prep::Cursor(p) = cursor.prep.unwrap() else {
        panic!()
    };
    assert!(p.allow.contains(&"Shell(git status)".to_string()));
    assert!(
        p.allow
            .contains(&"Write(/home/user/repo/src/a.rs)".to_string())
    );

    let mut cmd = launch(Prep::Cmd(super::cmd::CmdPrep {
        settings: PathBuf::from("/tmp/settings.json"),
        allow: Vec::new(),
        additional_dirs: Vec::new(),
        global_mcp: PathBuf::from("/tmp/mcp.json"),
        signals: PathBuf::from("/tmp/signals.jsonl"),
        readonly: false,
    }));
    apply_permits(&mut cmd, "cmd", &permits);
    let Prep::Cmd(p) = cmd.prep.unwrap() else {
        panic!()
    };
    assert!(p.allow.contains(&"Shell(git status:*)".to_string()));

    let mut agy = launch(Prep::Agy(super::agy::AgyPrep {
        settings: PathBuf::from("/tmp/a.json"),
        hooks: PathBuf::from("/tmp/hooks.json"),
        signals: PathBuf::from("/tmp/signals.jsonl"),
        trusted_workspace: None,
        allow: Vec::new(),
        readonly: false,
    }));
    apply_permits(&mut agy, "agy", &permits);
    let Prep::Agy(p) = agy.prep.unwrap() else {
        panic!()
    };
    assert_eq!(p.allow, vec!["git status"]);

    let mut devin = launch(Prep::Devin(super::devin::DevinPrep {
        config: PathBuf::from("/tmp/config.json"),
        signals: PathBuf::from("/tmp/signals.jsonl"),
        allow: Vec::new(),
        readonly: false,
    }));
    apply_permits(&mut devin, "devin", &permits);
    let Prep::Devin(p) = devin.prep.unwrap() else {
        panic!()
    };
    assert!(p.allow.contains(&"Exec(git status)".to_string()));
}

#[test]
fn apply_permits_rewrites_claude_allowed_tools() {
    let permits = vec![
        permit(PermitKind::Shell, "git diff"),
        permit(PermitKind::Edit, "/home/user/repo/src/a.rs"),
    ];
    let mut claude = Launch {
        argv: vec!["claude".into(), "--allowedTools".into(), "Bash(ls)".into()],
        env: BTreeMap::new(),
        session: Session::PostLaunch,
        hooks: None,
        prep: None,
    };
    apply_permits(&mut claude, "claude", &permits);
    let tools = &claude.argv[2];
    assert!(tools.contains("Bash(git diff)"), "{tools}");
    assert!(tools.contains("Bash(git diff *)"), "{tools}");
    assert!(tools.contains("Edit(/home/user/repo/src/a.rs)"), "{tools}");
}

#[test]
fn exec_capable_rule_families_stay_ledger_only_and_devin_rules_are_cleaned() {
    for rule in ["find", "fd", "rg", "sort", "tar tzf", "sed -n"] {
        let permits = vec![permit(PermitKind::Shell, rule)];
        for runner in ["devin", "cmd", "cursor", "agy"] {
            let prep = match runner {
                "devin" => Prep::Devin(super::devin::DevinPrep {
                    config: PathBuf::from("/tmp/config.json"),
                    signals: PathBuf::from("/tmp/signals.jsonl"),
                    allow: Vec::new(),
                    readonly: false,
                }),
                "cmd" => Prep::Cmd(super::cmd::CmdPrep {
                    settings: PathBuf::from("/tmp/settings.json"),
                    allow: Vec::new(),
                    additional_dirs: Vec::new(),
                    global_mcp: PathBuf::from("/tmp/mcp.json"),
                    signals: PathBuf::from("/tmp/signals.jsonl"),
                    readonly: false,
                }),
                "cursor" => Prep::Cursor(super::cursor::CursorPrep {
                    root: PathBuf::from("/tmp/cursor"),
                    project: "p".into(),
                    allow: Vec::new(),
                    global_mcp: PathBuf::from("/tmp/mcp.json"),
                    readonly: false,
                    hooks: PathBuf::from("/tmp/work/.cursor/hooks.json"),
                    signals: PathBuf::from("/tmp/signals.jsonl"),
                }),
                _ => Prep::Agy(super::agy::AgyPrep {
                    settings: PathBuf::from("/tmp/a.json"),
                    hooks: PathBuf::from("/tmp/hooks.json"),
                    signals: PathBuf::from("/tmp/signals.jsonl"),
                    trusted_workspace: None,
                    allow: Vec::new(),
                    readonly: false,
                }),
            };
            let mut launch = launch(prep);
            apply_permits(&mut launch, runner, &permits);
            match launch.prep.unwrap() {
                Prep::Devin(p) => assert!(p.allow.is_empty(), "{runner} wrote {rule}"),
                Prep::Cmd(p) => assert!(p.allow.is_empty(), "{runner} wrote {rule}"),
                Prep::Cursor(p) => assert!(p.allow.is_empty(), "{runner} wrote {rule}"),
                Prep::Agy(p) => assert!(p.allow.is_empty(), "{runner} wrote {rule}"),
                _ => unreachable!(),
            }
        }
    }

    let permits = vec![permit(PermitKind::Shell, "cargo test *")];
    let mut devin = launch(Prep::Devin(super::devin::DevinPrep {
        config: PathBuf::from("/tmp/config.json"),
        signals: PathBuf::from("/tmp/signals.jsonl"),
        allow: Vec::new(),
        readonly: false,
    }));
    apply_permits(&mut devin, "devin", &permits);
    let Prep::Devin(p) = devin.prep.unwrap() else {
        panic!()
    };
    assert!(
        p.allow.is_empty(),
        "metacharacters reached Devin: {:?}",
        p.allow
    );
}
