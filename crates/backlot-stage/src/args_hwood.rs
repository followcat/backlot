//! Parsers for the overseer commands: `hollywood propose`,
//! `hollywood report`, and the `card` gate's `apply`/`cancel` —
//! plus their `Cmd` payload types and usage text.

use std::path::PathBuf;

use backlot_model::hollywood::CardAction;

use crate::args::*;
use crate::args_studio::repo_path;
use crate::args_task;
use crate::cli::Fail;
use crate::config::HollywoodCfg;

/// `backlot hollywood`'s subcommands.
pub enum HollywoodCmd {
    /// One card per action (D18) — `why` is Hollywood's one-line
    /// reason, stored on the card.
    Propose { action: CardAction, why: String },
    /// `hollywood report "<headline>" [--body <file>]`.
    Report {
        headline: String,
        body: Option<PathBuf>,
    },
    /// `hollywood setup` (R22) — writes the `[hollywood]` table.
    Setup { cfg: HollywoodCfg },
}

/// `backlot card`'s subcommands.
pub enum CardCmd {
    List,
    /// `card show <id>` — one card's full read, a `studio-new`'s
    /// draft included.
    Show {
        id: u64,
    },
    Apply {
        id: u64,
    },
    Cancel {
        id: u64,
        reason: String,
    },
}

pub(crate) const U_HOLLYWOOD: &str = "usage:
  backlot hollywood propose <action> [--studio <s>] [--task <t>] [--person <role>] \
[--level low|normal|high] [--title <t>] [--repo <path>] [--cast <f>] [--brief <f>] --why <line>
       actions: studio-new | close | reopen | priority | pause | resume | abandon
  backlot hollywood report \"<headline>\" [--body <f>]
  backlot hollywood setup --runner <r> --model <m> [--effort <e>] [--key <k>] [--default-cast <f>]";
pub(crate) const U_CARD: &str = "usage:
  backlot card list
  backlot card show <id>
  backlot card apply <id>
  backlot card cancel <id> [--reason <r>]";

/// `backlot hollywood propose|report`.
pub(crate) fn hollywood(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let Some(sub) = it.next() else {
        return Err(usage("missing hollywood subcommand", U_HOLLYWOOD));
    };
    match sub.as_str() {
        "propose" => propose(common, it),
        "report" => hreport(common, it),
        "setup" => hsetup_args(common, it),
        "-h" | "--help" => Ok((std::mem::take(common), Cmd::Help(U_HOLLYWOOD))),
        _ => Err(usage(
            format!("unknown hollywood subcommand '{sub}'"),
            U_HOLLYWOOD,
        )),
    }
}

/// `backlot card list|apply|cancel` — the card gate (D17a).
pub(crate) fn card(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let Some(sub) = it.next() else {
        return Err(usage("missing card subcommand", U_CARD));
    };
    match sub.as_str() {
        "list" => crate::args_studio::common_only(common, it, U_CARD, Cmd::Card(CardCmd::List)),
        "show" => card_id(common, it, CardKind::Show),
        "apply" => card_id(common, it, CardKind::Apply),
        "cancel" => card_id(common, it, CardKind::Cancel),
        "-h" | "--help" => Ok((std::mem::take(common), Cmd::Help(U_CARD))),
        _ => Err(usage(format!("unknown card subcommand '{sub}'"), U_CARD)),
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
    let mut cast = None;
    let mut brief = None;
    let mut why = None;
    let mut no_git = false;
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
            "--cast" => cast = Some(PathBuf::from(it.value("--cast", U_HOLLYWOOD)?)),
            _ if eq(&arg, "--cast").is_some() => cast = eq(&arg, "--cast").map(PathBuf::from),
            "--brief" => brief = Some(PathBuf::from(it.value("--brief", U_HOLLYWOOD)?)),
            _ if eq(&arg, "--brief").is_some() => brief = eq(&arg, "--brief").map(PathBuf::from),
            "--no-git" => no_git = true,
            "--why" => why = Some(it.value("--why", U_HOLLYWOOD)?),
            _ if eq(&arg, "--why").is_some() => why = eq(&arg, "--why").map(String::from),
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
        other => return Err(usage(format!("unknown card action '{other}'"), U_HOLLYWOOD)),
    };
    Ok((
        std::mem::take(common),
        Cmd::Hollywood(HollywoodCmd::Propose {
            action,
            why: why.ok_or_else(|| usage("missing --why", U_HOLLYWOOD))?,
        }),
    ))
}

fn task(common: &Common) -> Result<String, Fail> {
    common
        .task
        .clone()
        .ok_or_else(|| usage("missing --task", U_HOLLYWOOD))
}

/// `hollywood report "<headline>" [--body <file>]`.
pub(crate) fn hreport(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut headline = None;
    let mut body = None;
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
        }),
    ))
}

/// Which `card <id>` verb is being parsed — `cancel` alone takes
/// `--reason`.
pub(crate) enum CardKind {
    Show,
    Apply,
    Cancel,
}

/// `card show|apply <id>` / `card cancel <id> [--reason <r>]`: the id
/// is `c<seq>` or a bare seq.
pub(crate) fn card_id(
    common: &mut Common,
    it: &mut It,
    kind: CardKind,
) -> Result<(Common, Cmd), Fail> {
    let cancel = matches!(kind, CardKind::Cancel);
    let mut id = None;
    let mut reason = String::new();
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_CARD)));
        }
        if common_flag(it, common, &arg, U_CARD)? {
            continue;
        }
        match arg.as_str() {
            "--reason" if cancel => reason = it.value("--reason", U_CARD)?,
            _ if cancel && eq(&arg, "--reason").is_some() => {
                reason = eq(&arg, "--reason").map(String::from).unwrap_or_default()
            }
            _ if !arg.starts_with('-') && id.is_none() => {
                id = Some(parse_card_id(&arg).ok_or_else(|| {
                    usage(format!("bad card id '{arg}', want c<seq> or <seq>"), U_CARD)
                })?)
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_CARD)),
        }
    }
    let id = id.ok_or_else(|| usage("missing <id>", U_CARD))?;
    let cmd = match kind {
        CardKind::Show => CardCmd::Show { id },
        CardKind::Apply => CardCmd::Apply { id },
        CardKind::Cancel => CardCmd::Cancel { id, reason },
    };
    Ok((std::mem::take(common), Cmd::Card(cmd)))
}

fn parse_card_id(v: &str) -> Option<u64> {
    v.strip_prefix('c').unwrap_or(v).parse::<u64>().ok()
}
