//! Hand-written argument parsing for the studio commands, in the style
//! of `crates/hc-tui/src/cli.rs`. Common caller flags `--studio`,
//! `--as`, `--task` work on every command. The per-command parsers
//! live in `args_studio` and `args_task`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use backlot_model::event::{ApprovalDecision, Check, PlanKind, Priority, TaskStateChange, Verdict};

use crate::cli::Fail;
use crate::{args_studio, args_task};

#[derive(Default)]
pub struct Common {
    pub studio: Option<String>,
    pub role: Option<String>,
    pub task: Option<String>,
    /// `--session`/`--remote`, as the TUI takes them. Used only when a
    /// command must reach the core (no `BACKLOT_STATE_DIR`, or the
    /// stage commands).
    pub session: Option<String>,
    pub remote: Option<String>,
}

pub enum Cmd {
    Help(&'static str),
    StudioNew {
        title: String,
        repo: String,
        cast_file: PathBuf,
    },
    ReqRevise {
        file: PathBuf,
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
    Ask {
        file: PathBuf,
    },
    Tell {
        role: String,
        file: PathBuf,
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
    Priority {
        level: Priority,
    },
    Reopen {
        person: String,
    },
    Close {
        person: String,
    },
    Approve {
        person: String,
        decision: ApprovalDecision,
    },
    AttentionClear {
        target: u64,
    },
    StudioFinish {
        reason: String,
    },
    Deliver {
        commits: String,
        report: PathBuf,
        checks: Vec<Check>,
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
    },
    Records {
        json: bool,
    },
    StageStart,
    StageRun,
    StageWatch {
        target: Option<String>,
    },
}

/// Tail line of every command's usage text.
pub const COMMON: &str =
    "  common: --studio <name> --as <role> --task <id> --session <s> --remote <t>";

pub(crate) const U_STUDIO: &str = "usage: backlot studio new --title <t> --repo <path> --cast <file.toml>\n       backlot studio finish [--reason <r>]";
pub(crate) const U_REQ: &str = "usage: backlot req revise --file <f> --summary <s>";
pub(crate) const U_PLAN: &str =
    "usage: backlot plan request | publish --kind plan|acceptance --file <f>";
pub(crate) const U_DISPATCH: &str = "usage: backlot dispatch --file <f>";
pub(crate) const U_REPORT: &str = "usage: backlot report --node <n> --file <f>";
pub(crate) const U_ASK: &str = "usage: backlot ask --file <f>";
pub(crate) const U_TELL: &str = "usage: backlot tell <role> --file <f>";
pub(crate) const U_TASK: &str = "usage: backlot task new --title <t> --kind <k> --acceptance-file <f> [--depends-on <id>...]\n       backlot task pause|resume|abandon|takeover --task <id> --reason <r>";
pub(crate) const U_CAST: &str =
    "usage: backlot cast --task <id> --actor <name> [--reason <r>] [--priority low|normal|high]";
pub(crate) const U_PRIORITY: &str = "usage: backlot priority --task <id> --level low|normal|high";
pub(crate) const U_REOPEN: &str = "usage: backlot reopen --person <role key>";
pub(crate) const U_CLOSE: &str = "usage: backlot close --person <role key>";
pub(crate) const U_APPROVE: &str =
    "usage: backlot approve --person <role key> --decision allow|deny";
pub(crate) const U_ATTENTION: &str = "usage: backlot attention clear <seq>";
pub(crate) const U_DELIVER: &str =
    "usage: backlot deliver --commits A..B --report <f> [--check name=pass|fail...]";
pub(crate) const U_BLOCK: &str = "usage: backlot block --reason <r>";
pub(crate) const U_REVIEW: &str = "usage: backlot review --task <id> --delivery <n> --verdict \
accepted|partially-accepted|changes-requested|blocked --conclusion <text> \
[--findings <f.json>] [--upstream-changes <f.json>]";
pub(crate) const U_FINDING: &str =
    "usage: backlot finding --task <id> --delivery <n> --findings <file.json>";
pub(crate) const U_CORRECT: &str =
    "usage: backlot correct --event <seq> --reason <r> --set <key=value> [--set ...]";
pub(crate) const U_STATUS: &str = "usage: backlot status [--json]";
pub(crate) const U_RECORDS: &str = "usage: backlot records [--json]";
pub(crate) const U_STAGE: &str = "usage: backlot stage start [--session <s>] [--remote <t>]\n       backlot stage run\n       backlot stage watch [--target <hc-terminal>]";

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

/// Per-command arg loops: `--x v` and `--x=v`, common flags anywhere,
/// `-h`/`--help` prints the command's usage.
pub fn parse(args: &[String]) -> Result<(Common, Cmd), Fail> {
    let mut common = Common::default();
    let mut it = It {
        it: args.iter().peekable(),
    };
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
                "finish" => args_studio::studio_finish(&mut common, &mut it),
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
        "ask" => args_studio::one_file(&mut common, &mut it, U_ASK, |file| Cmd::Ask { file }),
        "tell" => args_studio::tell(&mut common, &mut it),
        "cast" => args_task::cast(&mut common, &mut it),
        "priority" => args_task::priority(&mut common, &mut it),
        "reopen" => args_task::person(&mut common, &mut it, U_REOPEN, |person| Cmd::Reopen {
            person,
        }),
        "close" => args_task::person(&mut common, &mut it, U_CLOSE, |person| Cmd::Close {
            person,
        }),
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
        "status" => {
            args_task::flags_only(&mut common, &mut it, U_STATUS, |json| Cmd::Status { json })
        }
        "records" => args_task::flags_only(&mut common, &mut it, U_RECORDS, |json| Cmd::Records {
            json,
        }),
        "stage" => {
            let Some(sub) = it.next() else {
                return Err(usage("missing stage subcommand", U_STAGE));
            };
            match sub.as_str() {
                "start" => args_studio::common_only(&mut common, &mut it, U_STAGE, Cmd::StageStart),
                "run" => args_studio::common_only(&mut common, &mut it, U_STAGE, Cmd::StageRun),
                "watch" => args_studio::stage_watch(&mut common, &mut it),
                "-h" | "--help" => Ok((common, Cmd::Help(U_STAGE))),
                _ => Err(usage(format!("unknown stage subcommand '{sub}'"), U_STAGE)),
            }
        }
        other => Err(usage(format!("unknown command '{other}'"), U_STATUS)),
    }
}
