//! Parsers for the overseer commands: `hollywood propose`,
//! `hollywood report`, and the `card` gate's `apply`/`cancel` —
//! plus their `Cmd` payload types and usage text.

use std::path::PathBuf;

use backlot_model::event::ActorIdentity;
use backlot_model::hollywood::CardAction;

use crate::args::*;
use crate::args_card::parse_answers;
use crate::args_studio::repo_path;
use crate::args_task;
use crate::cli::Fail;
use crate::config::HollywoodCfg;

#[path = "args_hswitch.rs"]
mod args_hswitch;
pub use crate::args_card::CardCmd;
pub(crate) use crate::args_card::card;
use args_hswitch::hswitch_args;

/// `backlot hollywood`'s subcommands.
pub enum HollywoodCmd {
    /// One card per action (D18) — `why` is Hollywood's one-line
    /// reason, stored on the card.
    Propose {
        action: CardAction,
        why: String,
        answers: Vec<u64>,
    },
    /// `hollywood report "<headline>" [--body <file>] [--answers <seqs>]`.
    Report {
        headline: String,
        body: Option<PathBuf>,
        answers: Vec<u64>,
    },
    /// `hollywood show <seq>` — full report body or message text.
    Show { seq: u64 },
    /// `hollywood withdraw <card> --reason <text>` — Hollywood takes
    /// back its own open card.
    Withdraw { id: u64, reason: String },
    /// `hollywood setup` (R22) — writes the `[hollywood]` table.
    Setup {
        cfg: HollywoodCfg,
        force_offline: bool,
    },
    /// `hollywood switch` — switch Hollywood's agent by make-before-break handoff.
    Switch {
        runner: String,
        model: String,
        effort: Option<String>,
        timeout_s: Option<u64>,
        force: bool,
        no_wait: bool,
        json: bool,
    },
    /// `hollywood switch --cancel`.
    SwitchCancel { json: bool },
    /// `hollywood switch --status`.
    SwitchStatus { json: bool },
}

pub(crate) const U_HOLLYWOOD: &str = "usage:
  backlot hollywood propose <action> [--studio <s>] [--task <t>] [--person <role>] \
[--level low|normal|high] [--runner <r>] [--model <m>] [--effort <e>] \
[--title <t>] [--repo <path>] [--id <slug>] [--text <text>] [--cast <f>] [--brief <f>] [--fresh] [--file <f>]
[--answers <seqs>] --why <line>
       actions: studio-new | close | reopen | reopen-studio | priority | pause | resume | abandon | recast | restart | cast | setup | repo-note
  backlot hollywood report \"<headline>\" [--body <f>] [--answers <seqs>]
  backlot hollywood withdraw <card> --reason <text>
  backlot hollywood show <seq>
  backlot hollywood setup --runner <r> --model <m> [--effort <e>] [--key <k>] [--default-cast <f>] [--force-offline]
  backlot hollywood switch --runner <r> --model <m> [--effort <e>] [--timeout <secs>] [--force] [--no-wait] [--json]
  backlot hollywood switch --cancel [--json]
  backlot hollywood switch --status [--json]
  a studio-new draft's --studio needs a name matching [a-z0-9_-]{1,16}";

/// `backlot hollywood propose|report|show|withdraw|setup|switch`.
pub(crate) fn hollywood(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let Some(sub) = it.next() else {
        return Err(usage("missing hollywood subcommand", U_HOLLYWOOD));
    };
    match sub.as_str() {
        "propose" => propose(common, it),
        "report" => hreport(common, it),
        "show" => hshow(common, it),
        "withdraw" => hwithdraw(common, it),
        "setup" => hsetup_args(common, it),
        "switch" => hswitch_args(common, it),
        "-h" | "--help" => Ok((std::mem::take(common), Cmd::Help(U_HOLLYWOOD))),
        _ => Err(usage(
            format!("unknown hollywood subcommand '{sub}'"),
            U_HOLLYWOOD,
        )),
    }
}

/// `hollywood propose <action>`: the action word first, then the flags
/// that action takes; `--why` is always required — a card without
/// its reason is one nobody can review.
pub(crate) fn propose(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let Some(word) = it.next() else {
        return Err(usage("missing card action", U_HOLLYWOOD));
    };
    if word == "-h" || word == "--help" {
        return Ok((std::mem::take(common), Cmd::Help(U_HOLLYWOOD)));
    }
    let mut person = None;
    let mut level = None;
    let mut title = None;
    let mut repo = None;
    let mut entry_id = None;
    let mut text = None;
    let mut cast = None;
    let mut brief = None;
    let mut runner = None;
    let mut model = None;
    let mut effort = None;
    let mut why = None;
    let mut answers = Vec::new();
    let mut no_git = false;
    let mut fresh = false;
    let mut cast_file = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_HOLLYWOOD)));
        }
        if common_flag(it, common, &arg, U_HOLLYWOOD)? {
            continue;
        }
        match arg.as_str() {
            "--person" => person = Some(it.value("--person", U_HOLLYWOOD)?),
            _ if eq(&arg, "--person").is_some() => person = eq(&arg, "--person").map(String::from),
            "--level" => {
                level = Some(args_task::parse_level(
                    &it.value("--level", U_HOLLYWOOD)?,
                    U_HOLLYWOOD,
                )?)
            }
            _ if eq(&arg, "--level").is_some() => {
                level = Some(args_task::parse_level(
                    eq(&arg, "--level").unwrap(),
                    U_HOLLYWOOD,
                )?)
            }
            "--title" => title = Some(it.value("--title", U_HOLLYWOOD)?),
            _ if eq(&arg, "--title").is_some() => title = eq(&arg, "--title").map(String::from),
            "--repo" => repo = Some(it.value("--repo", U_HOLLYWOOD)?),
            _ if eq(&arg, "--repo").is_some() => repo = eq(&arg, "--repo").map(String::from),
            "--id" => entry_id = Some(it.value("--id", U_HOLLYWOOD)?),
            _ if eq(&arg, "--id").is_some() => entry_id = eq(&arg, "--id").map(String::from),
            "--text" => text = Some(it.value("--text", U_HOLLYWOOD)?),
            _ if eq(&arg, "--text").is_some() => text = eq(&arg, "--text").map(String::from),
            "--cast" => cast = Some(PathBuf::from(it.value("--cast", U_HOLLYWOOD)?)),
            _ if eq(&arg, "--cast").is_some() => cast = eq(&arg, "--cast").map(PathBuf::from),
            "--brief" => brief = Some(PathBuf::from(it.value("--brief", U_HOLLYWOOD)?)),
            _ if eq(&arg, "--brief").is_some() => brief = eq(&arg, "--brief").map(PathBuf::from),
            "--runner" => runner = Some(it.value("--runner", U_HOLLYWOOD)?),
            _ if eq(&arg, "--runner").is_some() => runner = eq(&arg, "--runner").map(String::from),
            "--model" => model = Some(it.value("--model", U_HOLLYWOOD)?),
            _ if eq(&arg, "--model").is_some() => model = eq(&arg, "--model").map(String::from),
            "--effort" => effort = Some(it.value("--effort", U_HOLLYWOOD)?),
            _ if eq(&arg, "--effort").is_some() => effort = eq(&arg, "--effort").map(String::from),
            "--no-git" => no_git = true,
            "--fresh" => fresh = true,
            "--file" => cast_file = Some(it.value("--file", U_HOLLYWOOD)?),
            _ if eq(&arg, "--file").is_some() => cast_file = eq(&arg, "--file").map(String::from),
            "--why" => why = Some(it.value("--why", U_HOLLYWOOD)?),
            _ if eq(&arg, "--why").is_some() => why = eq(&arg, "--why").map(String::from),
            "--answers" => parse_answers(
                &it.value("--answers", U_HOLLYWOOD)?,
                &mut answers,
                U_HOLLYWOOD,
            )?,
            _ if eq(&arg, "--answers").is_some() => {
                parse_answers(eq(&arg, "--answers").unwrap(), &mut answers, U_HOLLYWOOD)?
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_HOLLYWOOD)),
        }
    }
    let studio = |flag| -> Result<String, Fail> {
        common
            .studio
            .clone()
            .ok_or_else(|| usage(format!("missing {flag}"), U_HOLLYWOOD))
    };
    let action = match word.as_str() {
        "studio-new" => CardAction::StudioNew {
            studio: studio("--studio")?,
            title: title.ok_or_else(|| usage("missing --title", U_HOLLYWOOD))?,
            repo: repo_path(&repo.ok_or_else(|| usage("missing --repo", U_HOLLYWOOD))?)?,
            cast: cast
                .ok_or_else(|| usage("missing --cast", U_HOLLYWOOD))?
                .to_string_lossy()
                .into_owned(),
            brief: brief
                .ok_or_else(|| usage("missing --brief", U_HOLLYWOOD))?
                .to_string_lossy()
                .into_owned(),
            no_git,
            snapshot: None,
        },
        "close" | "reopen" => {
            let person = person.ok_or_else(|| usage("missing --person", U_HOLLYWOOD))?;
            let studio = studio("--studio")?;
            if word == "close" {
                CardAction::Close { studio, person }
            } else {
                CardAction::Reopen { studio, person }
            }
        }
        // `reopen-studio` brings a finished studio back: the person's
        // own `studio reopen` is the command, and a card is Hollywood's
        // only way to ask for it.
        "reopen-studio" => {
            if person.is_some() {
                return Err(usage("reopen-studio takes no --person", U_HOLLYWOOD));
            }
            CardAction::ReopenStudio {
                studio: studio("--studio")?,
            }
        }
        "priority" => CardAction::Priority {
            studio: studio("--studio")?,
            task: task(common)?,
            level: level.ok_or_else(|| usage("missing --level", U_HOLLYWOOD))?,
        },
        "pause" | "resume" | "abandon" => {
            let studio = studio("--studio")?;
            let task = task(common)?;
            match word.as_str() {
                "pause" => CardAction::Pause { studio, task },
                "resume" => CardAction::Resume { studio, task },
                _ => CardAction::Abandon { studio, task },
            }
        }
        "restart" => CardAction::Restart {
            studio: studio("--studio")?,
            person: person.ok_or_else(|| usage("missing --person", U_HOLLYWOOD))?,
            fresh,
        },
        "cast" => CardAction::Cast {
            studio: studio("--studio")?,
            cast: cast_file
                .or_else(|| cast.map(|p| p.to_string_lossy().into_owned()))
                .ok_or_else(|| usage("missing --file", U_HOLLYWOOD))?,
            snapshot: None,
        },
        "recast" => {
            let studio = studio("--studio")?;
            let person = person.ok_or_else(|| usage("missing --person", U_HOLLYWOOD))?;
            CardAction::Recast {
                studio,
                person,
                actor: ActorIdentity {
                    runner: runner.ok_or_else(|| usage("missing --runner", U_HOLLYWOOD))?,
                    runner_version: None,
                    model: model.ok_or_else(|| usage("missing --model", U_HOLLYWOOD))?,
                    effort,
                    // Filled from the role's current identity when the
                    // card is resolved, before it is stored.
                    permission_profile: String::new(),
                },
            }
        }
        "setup" => {
            if common.studio.is_some() {
                return Err(usage("setup takes no --studio", U_HOLLYWOOD));
            }
            if person.is_some() {
                return Err(usage("setup takes no --person", U_HOLLYWOOD));
            }
            CardAction::Setup {
                runner: runner.ok_or_else(|| usage("missing --runner", U_HOLLYWOOD))?,
                model: model.ok_or_else(|| usage("missing --model", U_HOLLYWOOD))?,
                effort,
                permission_profile: None,
            }
        }
        "repo-note" => CardAction::RepoNote {
            studio: common
                .studio
                .clone()
                .unwrap_or_else(|| backlot_model::hollywood::HOLLYWOOD.to_string()),
            repo: repo_path(&repo.ok_or_else(|| usage("missing --repo", U_HOLLYWOOD))?)?,
            entry_id: entry_id.ok_or_else(|| usage("missing --id", U_HOLLYWOOD))?,
            text: text.ok_or_else(|| usage("missing --text", U_HOLLYWOOD))?,
            source_seq: None,
        },
        other => return Err(usage(format!("unknown card action '{other}'"), U_HOLLYWOOD)),
    };
    Ok((
        std::mem::take(common),
        Cmd::Hollywood(HollywoodCmd::Propose {
            action,
            why: why.ok_or_else(|| usage("missing --why", U_HOLLYWOOD))?,
            answers,
        }),
    ))
}

fn task(common: &Common) -> Result<String, Fail> {
    common
        .task
        .clone()
        .ok_or_else(|| usage("missing --task", U_HOLLYWOOD))
}

/// `hollywood report "<headline>" [--body <file>] [--answers <seqs>]`.
pub(crate) fn hreport(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut headline = None;
    let mut body = None;
    let mut answers = Vec::new();
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_HOLLYWOOD)));
        }
        if common_flag(it, common, &arg, U_HOLLYWOOD)? {
            continue;
        }
        match arg.as_str() {
            "--body" => body = Some(PathBuf::from(it.value("--body", U_HOLLYWOOD)?)),
            _ if eq(&arg, "--body").is_some() => body = eq(&arg, "--body").map(PathBuf::from),
            "--answers" => parse_answers(
                &it.value("--answers", U_HOLLYWOOD)?,
                &mut answers,
                U_HOLLYWOOD,
            )?,
            _ if eq(&arg, "--answers").is_some() => {
                parse_answers(eq(&arg, "--answers").unwrap(), &mut answers, U_HOLLYWOOD)?
            }
            _ if arg.starts_with('-') => {
                return Err(usage(format!("unexpected argument '{arg}'"), U_HOLLYWOOD));
            }
            _ if headline.is_none() => headline = Some(arg),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_HOLLYWOOD)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Hollywood(HollywoodCmd::Report {
            headline: headline.ok_or_else(|| usage("missing <headline>", U_HOLLYWOOD))?,
            body,
            answers,
        }),
    ))
}

/// `hollywood withdraw <card> --reason <text>`: the card is `c<seq>` or
/// a bare seq, like `card apply`'s.
fn hwithdraw(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut id = None;
    let mut reason = String::new();
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_HOLLYWOOD)));
        }
        if common_flag(it, common, &arg, U_HOLLYWOOD)? {
            continue;
        }
        match arg.as_str() {
            "--reason" => reason = it.value("--reason", U_HOLLYWOOD)?,
            _ if eq(&arg, "--reason").is_some() => {
                reason = eq(&arg, "--reason").map(String::from).unwrap_or_default()
            }
            _ if !arg.starts_with('-') && id.is_none() => {
                id = Some(crate::args_card::parse_card_id(&arg).ok_or_else(|| {
                    usage(
                        format!("bad card id '{arg}', want c<seq> or <seq>"),
                        U_HOLLYWOOD,
                    )
                })?)
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_HOLLYWOOD)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Hollywood(HollywoodCmd::Withdraw {
            id: id.ok_or_else(|| usage("missing <card>", U_HOLLYWOOD))?,
            reason,
        }),
    ))
}

/// `hollywood show <seq>`: `<seq>` or `#<seq>`.
fn hshow(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut seq = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_HOLLYWOOD)));
        }
        if common_flag(it, common, &arg, U_HOLLYWOOD)? {
            continue;
        }
        if !arg.starts_with('-') && seq.is_none() {
            let parsed = arg
                .strip_prefix('#')
                .unwrap_or(arg.as_str())
                .parse::<u64>()
                .map_err(|_| usage(format!("bad seq '{arg}', want <seq>"), U_HOLLYWOOD))?;
            seq = Some(parsed);
        } else {
            return Err(usage(format!("unexpected argument '{arg}'"), U_HOLLYWOOD));
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Hollywood(HollywoodCmd::Show {
            seq: seq.ok_or_else(|| usage("missing <seq>", U_HOLLYWOOD))?,
        }),
    ))
}

/// `hollywood setup --runner <r> --model <m> [--effort <e>] [--key
/// <k>] [--default-cast <f>]` — the write is `hsetup`'s; here the
/// flags only. `--default-cast` lets a reconfigure keep the table's
/// cast template (the TUI forwards it verbatim).
fn hsetup_args(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut runner = None;
    let mut model = None;
    let mut effort = None;
    let mut key = None;
    let mut default_cast = None;
    let mut force_offline = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_HOLLYWOOD)));
        }
        if common_flag(it, common, &arg, U_HOLLYWOOD)? {
            continue;
        }
        match arg.as_str() {
            "--runner" => runner = Some(it.value("--runner", U_HOLLYWOOD)?),
            _ if eq(&arg, "--runner").is_some() => runner = eq(&arg, "--runner").map(String::from),
            "--model" => model = Some(it.value("--model", U_HOLLYWOOD)?),
            _ if eq(&arg, "--model").is_some() => model = eq(&arg, "--model").map(String::from),
            "--effort" => effort = Some(it.value("--effort", U_HOLLYWOOD)?),
            _ if eq(&arg, "--effort").is_some() => effort = eq(&arg, "--effort").map(String::from),
            "--key" => key = Some(it.value("--key", U_HOLLYWOOD)?),
            _ if eq(&arg, "--key").is_some() => key = eq(&arg, "--key").map(String::from),
            "--default-cast" => default_cast = Some(it.value("--default-cast", U_HOLLYWOOD)?),
            _ if eq(&arg, "--default-cast").is_some() => {
                default_cast = eq(&arg, "--default-cast").map(String::from)
            }
            "--force-offline" => force_offline = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_HOLLYWOOD)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Hollywood(HollywoodCmd::Setup {
            cfg: HollywoodCfg {
                runner: runner.ok_or_else(|| usage("missing --runner", U_HOLLYWOOD))?,
                model: model.ok_or_else(|| usage("missing --model", U_HOLLYWOOD))?,
                effort,
                key,
                default_cast,
            },
            force_offline,
        }),
    ))
}
