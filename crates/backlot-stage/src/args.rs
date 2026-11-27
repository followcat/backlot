//! Hand-written argument parsing for the studio commands, in the style
//! of `crates/hc-tui/src/cli.rs`. Common caller flags `--studio`,
//! `--as`, `--task` work on every command. The per-command parsers
//! live in `args_studio` and `args_task`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use backlot_model::event::{
    ApprovalDecision, Check, PlanKind, Priority, StudioMode, TaskStateChange, Verdict,
};

use crate::cli::Fail;
use crate::{args_design, args_hwood, args_studio, args_task};

#[derive(Default, Clone)]
pub struct Common {
    pub studio: Option<String>,
    pub role: Option<String>,
    pub task: Option<String>,
    /// `--session`/`--remote`, as the TUI takes them. Used only when a
    /// command must reach the core (no `BACKLOT_STATE_DIR`, or the
    /// stage commands).
    pub session: Option<String>,
    pub remote: Option<String>,
    pub server: Option<String>,
}

pub use crate::args_source::TextSource;

pub(crate) use crate::args_source::Source;
pub(crate) use crate::args_usage::*;

pub enum Cmd {
    Help(&'static str),
    StudioNew {
        /// `Some` always without `--interactive`; the wizard asks for
        /// what the flags left unset.
        title: Option<String>,
        repo: Option<String>,
        cast_file: Option<PathBuf>,
        /// `--dry-run`: every `studio new` check plus the draft's own
        /// (name free, repo is git, runners installed, caps seat the
        /// fixed roles) — nothing recorded, nothing created.
        dry_run: bool,
        /// `--interactive`: the wizard asks on the terminal for what
        /// is missing (U2).
        interactive: bool,
        /// `--mode standard|experimental` — overrides the cast
        /// file's `mode`; `None` leaves the TOML or the default.
        mode: Option<StudioMode>,
        /// `--init`: a plain directory becomes a repository with an
        /// empty initial commit (D4).
        init: bool,
        /// `--no-git`: a plain directory binds with no git at all —
        /// reduced isolation, every role shares it (D4).
        no_git: bool,
        /// `--ask <text>` / `--ask-file <f>`: the person's first
        /// message to the new producer, queued as a `tell` after
        /// `studio.created`. `None` starts the studio with no
        /// requirement, as before.
        ask: Option<TextSource>,
    },
    ReqRevise {
        source: TextSource,
        summary: String,
    },
    PlanRequest,
    PlanPublish {
        plan_kind: PlanKind,
        file: PathBuf,
    },
    Dispatch {
        file: PathBuf,
    },
    Report {
        node: String,
        file: PathBuf,
    },
    /// `report show --studio <s> --file <relpath>` (G3): streams a
    /// studio artifact under `reports/`, `checks/`, `attention/` or
    /// `requirements/` to stdout — read-only, no event appended.
    ReportShow {
        file: PathBuf,
    },
    /// `msg show <seq> [--json]`: reads one message/design/report/finding
    /// entry from the caller's own studio ledger — read-only.
    MsgShow {
        seq: u64,
        json: bool,
    },
    Ask {
        file: PathBuf,
    },
    AskAcknowledge {
        role: String,
        target: u64,
    },
    AskRun {
        argv: Vec<String>,
        why: Option<String>,
        cwd: Option<PathBuf>,
        command: Option<String>,
    },
    PermsRevoke {
        id: String,
    },
    Tell {
        role: String,
        source: TextSource,
        /// `--ask`: a `tell user` that is a question awaiting the person.
        ask: bool,
        /// `--note`: explicitly send a `tell user` with `?` as plain information.
        note: bool,
    },
    /// `decide --id AD-<n> --file <path> [--supersedes AD-<m>] [--notify <roles>|active-actors]`
    Decide {
        id: String,
        file: PathBuf,
        supersedes: Option<String>,
        notify: Option<String>,
    },
    /// `progress "<line>"` / `progress --file <f>`: role's optional step update.
    Progress {
        source: TextSource,
    },
    /// `tell hollywood "<text>"`: the user's line to the overseer.
    TellHollywood {
        text: String,
    },
    TaskNew {
        title: String,
        task_kind: String,
        acceptance_file: PathBuf,
        depends_on: Vec<String>,
    },
    TaskState {
        change: TaskStateChange,
        reason: String,
    },
    Cast {
        actor: String,
        reason: String,
        priority: Option<Priority>,
    },
    /// `cast --task <id> --suggest [--json]` (L3): the candidates'
    /// factual records for that task's kind — read-only, appends
    /// nothing (D8).
    CastSuggest {
        json: bool,
    },
    Priority {
        level: Priority,
    },
    Reopen {
        person: String,
    },
    Close {
        person: String,
    },
    /// `restart --person <role> [--fresh] [--reason <r>]`: stop the
    /// person's terminal and launch it again under the same identity,
    /// resuming the recorded session unless `--fresh`.
    Restart {
        person: String,
        fresh: bool,
        reason: String,
    },
    /// `recast --person <role> --runner <r> --model <m> [--effort <e>]
    /// [--permission-profile standard|worktree] [--reason <r>]` — the
    /// user's; the permission profile is kept from the current
    /// identity when the flag is absent.
    Recast {
        person: String,
        runner: String,
        model: String,
        effort: Option<String>,
        permission_profile: Option<String>,
        reason: String,
    },
    Approve {
        person: String,
        decision: ApprovalDecision,
    },
    AttentionClear {
        target: u64,
    },
    /// `studio cast`: the studio's cast after creation. `show [--json]`
    /// prints the effective cast as a cast file; `set --file <toml>
    /// [--dry-run] [--json]` records it.
    StudioCast(CastCmd),
    StudioFinish {
        reason: String,
        /// `--force`: finish a studio with no requirement and no
        /// tasks anyway (L5).
        force: bool,
    },
    /// `studio reopen --reason <r>`: the person brings a finished
    /// studio back. The name comes from `--studio`/`BACKLOT_STUDIO`
    /// like every studio command.
    StudioReopen {
        reason: String,
    },
    /// `studio prune`: remove a finished studio's worktrees and
    /// `backlot/<studio>/*` branches — never its ledger.
    StudioPrune {
        studio: Option<String>,
        all_finished: bool,
        dry_run: bool,
        force: bool,
        json: bool,
    },
    /// `studio delete`: finish (if live), close terminals, prune
    /// worktrees/branches safely, and archive the ledger.
    StudioDelete {
        studio: Option<String>,
        dry_run: bool,
        force: bool,
        json: bool,
    },
    /// `studio migrate [<studio> | --all] [--dry-run]`: rewrite the
    /// ledger at the current schema version — no event is appended.
    StudioMigrate {
        studio: Option<String>,
        all: bool,
        dry_run: bool,
    },
    Deliver {
        commits: String,
        report: PathBuf,
        checks: Vec<Check>,
        /// Runner-reported token/cost figures (P4a) — `None` when
        /// no `--tokens-*`/`--cost-usd` flag came in.
        usage: Option<backlot_model::event::Usage>,
    },
    Block {
        reason: String,
    },
    Review {
        delivery: u32,
        verdict: Verdict,
        conclusion: String,
        findings: Option<PathBuf>,
        upstream_changes: Option<PathBuf>,
    },
    Finding {
        delivery: u32,
        findings: PathBuf,
    },
    Correct {
        event: u64,
        reason: String,
        set: BTreeMap<String, String>,
    },
    Status {
        json: bool,
        /// `--all`: every studio, then Hollywood's cards.
        all: bool,
    },
    Records {
        json: bool,
        /// `--all`: every studio's log (the default without a pin).
        all: bool,
        /// `--kind <task_kind>`: only records of that task kind.
        kind: Option<String>,
    },
    StageStart,
    StageStop,
    StageRun,
    StageWatch {
        target: Option<String>,
        /// `--restart`: keep the stage alive, including reconnecting
        /// the core after an `hc` restart.
        restart: bool,
    },
    /// `backlot stage service [--print | --write] [--session <s>]` —
    /// the systemd `--user` supervisor for `stage watch --restart`.
    StageService {
        /// `--write` instead of the default `--print`.
        write: bool,
    },
    /// `backlot guide [<topic>] [--grep <word>]` — backlot's own docs,
    /// compiled into the binary, read-only (never a disk search).
    Guide {
        topic: Option<String>,
        grep: Option<String>,
    },
    /// `backlot runners [--json]` — the detector's table (U5) or the
    /// G4 document, read-only.
    Runners {
        json: bool,
    },
    /// `backlot repos [--json] [--from <dir>] [--all]` — the G4
    /// directory listing the New Studio dialog browses.
    Repos {
        json: bool,
        from: Option<String>,
        /// `--all`: include `.`-prefixed directories.
        all: bool,
    },
    /// `backlot studio list [--json] [--archived]` — every studio
    /// under every core's state directory (or the archived ones),
    /// read-only.
    StudioList {
        json: bool,
        /// `--archived`: list the ledgers under `archive/`.
        archived: bool,
    },
    /// `backlot studio restore <archived-name> [--as <new-name>]
    /// [--dry-run]` — bring an archived studio back, finished.
    StudioRestore {
        archived: String,
        new_name: Option<String>,
        dry_run: bool,
        json: bool,
    },
    /// `backlot hollywood propose|report` — Hollywood's own ledger.
    Hollywood(HollywoodCmd),
    /// `backlot card list|apply|cancel` — the card gate (D17a).
    Card(CardCmd),
    /// `backlot design open|write|supplement|note|next|sign|show` — opening design discussion.
    Design(DesignCmd),
    /// `backlot server run|start|stop|adopt|service` — single-server mode (B3).
    Server(ServerCmd),
}

/// Subcommands under `backlot studio cast`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CastCmd {
    /// `show [--json]`: the effective cast as a cast file, or the same
    /// settings structured.
    Show { json: bool },
    /// `set --file <toml> [--dry-run] [--json]`: validate (and, unless
    /// `--dry-run`, record) the file as the studio's cast.
    Set {
        file: PathBuf,
        dry_run: bool,
        json: bool,
    },
}

/// Subcommands under `backlot server`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerCmd {
    Run,
    Start,
    Stop,
    Adopt {
        dry_run: bool,
        stop_stages: bool,
        core: Option<String>,
    },
    Service {
        write: bool,
    },
}

pub use crate::args_design::{DesignCmd, QuestionInput};
pub use crate::args_hwood::{CardCmd, HollywoodCmd};

/// Tail line of every command's usage text.
pub const COMMON: &str = "  common: --studio <name> --as <role> --task <id> --session <s> --remote <t> --server <host>[/<session>]";

pub(crate) fn usage(msg: impl Into<String>, u: &'static str) -> Fail {
    Fail::usage(format!("{}\n{}\n{}", msg.into(), u, COMMON))
}

pub(crate) struct It<'a> {
    it: std::iter::Peekable<std::slice::Iter<'a, String>>,
}

impl<'a> It<'a> {
    pub(crate) fn next(&mut self) -> Option<String> {
        self.it.next().cloned()
    }
    /// Value for a flag in `--x value` position.
    pub(crate) fn value(&mut self, flag: &str, u: &'static str) -> Result<String, Fail> {
        self.next()
            .ok_or_else(|| usage(format!("{flag} needs a value"), u))
    }
    /// The next word without consuming it — subcommand dispatch.
    pub(crate) fn peek(&mut self) -> Option<&str> {
        self.it.peek().map(|s| s.as_str())
    }
}

/// `--name=value` split.
pub(crate) fn eq<'a>(arg: &'a str, name: &str) -> Option<&'a str> {
    arg.strip_prefix(name)?.strip_prefix('=')
}

/// Consume a common caller flag; `true` when one was eaten.
pub(crate) fn common_flag(
    it: &mut It,
    common: &mut Common,
    arg: &str,
    u: &'static str,
) -> Result<bool, Fail> {
    macro_rules! flag {
        ($field:ident, $flag:literal) => {
            if arg == $flag {
                common.$field = Some(it.value($flag, u)?);
                return Ok(true);
            }
            if let Some(v) = eq(arg, $flag) {
                common.$field = Some(v.to_string());
                return Ok(true);
            }
        };
    }
    flag!(studio, "--studio");
    flag!(role, "--as");
    flag!(task, "--task");
    flag!(session, "--session");
    flag!(remote, "--remote");
    flag!(server, "--server");
    Ok(false)
}

pub(crate) fn parse_check(v: &str, u: &'static str) -> Result<Check, Fail> {
    let (name, result) = v
        .split_once('=')
        .ok_or_else(|| usage(format!("bad --check '{v}', want name=pass|fail"), u))?;
    match result {
        "pass" => Ok(Check {
            name: name.to_string(),
            passed: true,
        }),
        "fail" => Ok(Check {
            name: name.to_string(),
            passed: false,
        }),
        _ => Err(usage(format!("bad --check '{v}', want name=pass|fail"), u)),
    }
}

pub(crate) fn parse_set(v: &str, u: &'static str) -> Result<(String, String), Fail> {
    let (k, val) = v
        .split_once('=')
        .ok_or_else(|| usage(format!("bad --set '{v}', want key=value"), u))?;
    if k.is_empty() {
        return Err(usage(format!("bad --set '{v}', empty key"), u));
    }
    Ok((k.to_string(), val.to_string()))
}

/// A caller flag `common_flag` recognizes — the leading loop skips
/// only these, so an unknown flag still reaches the command parser's
/// own error.
fn is_common_flag(arg: &str) -> bool {
    [
        "--studio",
        "--as",
        "--task",
        "--session",
        "--remote",
        "--server",
    ]
    .iter()
    .any(|flag| arg == *flag || eq(arg, flag).is_some())
}

/// Per-command arg loops: `--x v` and `--x=v`, common flags anywhere,
/// `-h`/`--help` prints the command's usage.
pub fn parse(args: &[String]) -> Result<(Common, Cmd), Fail> {
    let mut common = Common::default();
    let mut it = It {
        it: args.iter().peekable(),
    };
    // Common flags may lead the command: `backlot --remote host-b
    // studio list` reads the same as `backlot studio list --remote
    // host-b`. A line with no command word behind them never reaches
    // here — `maybe_run` leaves it to the TUI's own parser.
    while let Some(arg) = it.peek().map(str::to_owned) {
        if !is_common_flag(&arg) {
            break;
        }
        it.next();
        common_flag(&mut it, &mut common, &arg, U_STATUS)?;
    }
    let Some(word) = it.next() else {
        return Err(usage("missing studio command", U_STATUS));
    };
    match word.as_str() {
        "studio" => {
            let Some(sub) = it.next() else {
                return Err(usage("missing studio subcommand", U_STUDIO));
            };
            match sub.as_str() {
                "new" => args_studio::studio_new(&mut common, &mut it),
                "cast" => args_studio::studio_cast(&mut common, &mut it),
                "finish" => args_studio::studio_finish(&mut common, &mut it),
                "reopen" => args_studio::studio_reopen(&mut common, &mut it),
                "prune" => args_studio::studio_prune(&mut common, &mut it),
                "delete" => args_studio::studio_delete(&mut common, &mut it),
                "migrate" => args_studio::studio_migrate(&mut common, &mut it),
                "list" => args_studio::studio_list(&mut common, &mut it),
                "restore" => args_studio::studio_restore(&mut common, &mut it),
                "-h" | "--help" => Ok((common, Cmd::Help(U_STUDIO))),
                _ => Err(usage(
                    format!("unknown studio subcommand '{sub}'"),
                    U_STUDIO,
                )),
            }
        }
        "req" => {
            let Some(sub) = it.next() else {
                return Err(usage("missing req subcommand", U_REQ));
            };
            match sub.as_str() {
                "revise" => args_studio::req_revise(&mut common, &mut it),
                "-h" | "--help" => Ok((common, Cmd::Help(U_REQ))),
                _ => Err(usage(format!("unknown req subcommand '{sub}'"), U_REQ)),
            }
        }
        "plan" => {
            let Some(sub) = it.next() else {
                return Err(usage("missing plan subcommand", U_PLAN));
            };
            match sub.as_str() {
                "request" => {
                    while let Some(arg) = it.next() {
                        if arg == "-h" || arg == "--help" {
                            return Ok((common, Cmd::Help(U_PLAN)));
                        }
                        if common_flag(&mut it, &mut common, &arg, U_PLAN)? {
                            continue;
                        }
                        return Err(usage(format!("unexpected argument '{arg}'"), U_PLAN));
                    }
                    Ok((common, Cmd::PlanRequest))
                }
                "publish" => args_studio::plan_publish(&mut common, &mut it),
                "-h" | "--help" => Ok((common, Cmd::Help(U_PLAN))),
                _ => Err(usage(format!("unknown plan subcommand '{sub}'"), U_PLAN)),
            }
        }
        "task" => {
            let Some(sub) = it.next() else {
                return Err(usage("missing task subcommand", U_TASK));
            };
            match sub.as_str() {
                "new" => args_task::task_new(&mut common, &mut it),
                "pause" => args_task::task_state(&mut common, &mut it, TaskStateChange::Paused),
                "resume" => args_task::task_state(&mut common, &mut it, TaskStateChange::Resumed),
                "abandon" => {
                    args_task::task_state(&mut common, &mut it, TaskStateChange::Abandoned)
                }
                "takeover" => {
                    args_task::task_state(&mut common, &mut it, TaskStateChange::TakenOver)
                }
                "-h" | "--help" => Ok((common, Cmd::Help(U_TASK))),
                _ => Err(usage(format!("unknown task subcommand '{sub}'"), U_TASK)),
            }
        }
        "dispatch" => args_studio::one_file(&mut common, &mut it, U_DISPATCH, |file| {
            Cmd::Dispatch { file }
        }),
        "report" => args_studio::report(&mut common, &mut it),
        "ask" => args_studio::ask(&mut common, &mut it),
        "tell" => args_studio::tell(&mut common, &mut it),
        "decide" => args_studio::decide(&mut common, &mut it),
        "msg" => args_studio::msg(&mut common, &mut it),
        "progress" => args_studio::progress(&mut common, &mut it),
        "cast" => args_task::cast(&mut common, &mut it),
        "priority" => args_task::priority(&mut common, &mut it),
        "reopen" => args_task::person(&mut common, &mut it, U_REOPEN, |person| Cmd::Reopen {
            person,
        }),
        "close" => args_task::person(&mut common, &mut it, U_CLOSE, |person| Cmd::Close {
            person,
        }),
        "restart" => args_task::restart(&mut common, &mut it),
        "recast" => args_task::recast(&mut common, &mut it),
        "approve" => args_task::approve(&mut common, &mut it),
        "attention" => {
            let Some(sub) = it.next() else {
                return Err(usage("missing attention subcommand", U_ATTENTION));
            };
            match sub.as_str() {
                "clear" => args_task::attention_clear(&mut common, &mut it),
                "-h" | "--help" => Ok((common, Cmd::Help(U_ATTENTION))),
                _ => Err(usage(
                    format!("unknown attention subcommand '{sub}'"),
                    U_ATTENTION,
                )),
            }
        }
        "deliver" => args_task::deliver(&mut common, &mut it),
        "block" => args_task::block(&mut common, &mut it),
        "review" => args_task::review(&mut common, &mut it),
        "finding" => args_task::finding(&mut common, &mut it),
        "correct" => args_task::correct(&mut common, &mut it),
        "status" => args_task::flags_only(&mut common, &mut it, U_STATUS, |json, all| {
            Cmd::Status { json, all }
        }),
        "records" => args_task::records(&mut common, &mut it),
        "hollywood" => args_hwood::hollywood(&mut common, &mut it),
        "card" => args_hwood::card(&mut common, &mut it),
        "design" => args_design::design(&mut common, &mut it),
        "perms" => match it.next().as_deref() {
            Some("revoke") => {
                let id = it
                    .next()
                    .ok_or_else(|| usage("missing <id>", "usage: backlot perms revoke <id>"))?;
                Ok((common, Cmd::PermsRevoke { id }))
            }
            Some("-h" | "--help") => Ok((common, Cmd::Help("usage: backlot perms revoke <id>"))),
            Some(s) => Err(usage(
                format!("unknown perms subcommand '{s}'"),
                "usage: backlot perms revoke <id>",
            )),
            None => Err(usage(
                "missing perms subcommand",
                "usage: backlot perms revoke <id>",
            )),
        },
        "stage" => {
            let Some(sub) = it.next() else {
                return Err(usage("missing stage subcommand", U_STAGE));
            };
            match sub.as_str() {
                "start" => args_studio::common_only(&mut common, &mut it, U_STAGE, Cmd::StageStart),
                "stop" => args_studio::common_only(&mut common, &mut it, U_STAGE, Cmd::StageStop),
                "run" => args_studio::common_only(&mut common, &mut it, U_STAGE, Cmd::StageRun),
                "watch" => args_studio::stage_watch(&mut common, &mut it),
                "service" => args_studio::stage_service(&mut common, &mut it),
                "-h" | "--help" => Ok((common, Cmd::Help(U_STAGE))),
                _ => Err(usage(format!("unknown stage subcommand '{sub}'"), U_STAGE)),
            }
        }
        "server" => {
            let Some(sub) = it.next() else {
                return Err(usage("missing server subcommand", U_SERVER));
            };
            match sub.as_str() {
                "run" => args_studio::common_only(
                    &mut common,
                    &mut it,
                    U_SERVER,
                    Cmd::Server(ServerCmd::Run),
                ),
                "start" => args_studio::common_only(
                    &mut common,
                    &mut it,
                    U_SERVER,
                    Cmd::Server(ServerCmd::Start),
                ),
                "stop" => args_studio::common_only(
                    &mut common,
                    &mut it,
                    U_SERVER,
                    Cmd::Server(ServerCmd::Stop),
                ),
                "adopt" => args_studio::server_adopt(&mut common, &mut it),
                "service" => args_studio::server_service(&mut common, &mut it),
                "-h" | "--help" => Ok((common, Cmd::Help(U_SERVER))),
                _ => Err(usage(
                    format!("unknown server subcommand '{sub}'"),
                    U_SERVER,
                )),
            }
        }
        "guide" => args_studio::guide(&mut common, &mut it),
        "runners" => args_studio::runners(&mut common, &mut it),
        "repos" => args_studio::repos(&mut common, &mut it),
        other => Err(usage(format!("unknown command '{other}'"), U_STATUS)),
    }
}
