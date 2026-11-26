//! Parsers for the 7 `backlot design` subcommands: `open`, `write`,
//! `supplement`, `note`, `next`, `sign`, and `show`.

use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::design::{DesignQuestion, DesignSlot};

use crate::args::*;
use crate::args_source::Source;
use crate::cli::Fail;

const MAX_QUESTIONS: usize = 3;

/// `--question` value passed to `backlot design write`: inline JSON or `@path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionInput {
    Inline(String),
    File(PathBuf),
}

/// Subcommands under `backlot design`.
#[derive(Debug)]
pub enum DesignCmd {
    Open {
        req: Option<u32>,
        suggest_skip: bool,
        reason: Option<String>,
    },
    Write {
        slot: DesignSlot,
        file: PathBuf,
        refs: Vec<u64>,
        questions: Vec<QuestionInput>,
        questions_file: Option<PathBuf>,
    },
    Supplement {
        file: PathBuf,
        refs: Vec<u64>,
    },
    Note {
        from: String,
        source: TextSource,
    },
    Next,
    Sign {
        skip: bool,
        reason: Option<String>,
        note: Option<String>,
    },
    Show {
        version: Option<u32>,
        round: Option<u32>,
        json: bool,
    },
}

/// `backlot design open|write|supplement|note|next|sign|show`.
pub(crate) fn design(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let Some(sub) = it.next() else {
        return Err(usage("missing design subcommand", U_DESIGN));
    };
    match sub.as_str() {
        "open" => open(common, it),
        "write" => write(common, it),
        "supplement" => supplement(common, it),
        "note" => note(common, it),
        "next" => {
            crate::args_studio::common_only(common, it, U_DESIGN, Cmd::Design(DesignCmd::Next))
        }
        "sign" => sign(common, it),
        "show" => show(common, it),
        "-h" | "--help" => Ok((std::mem::take(common), Cmd::Help(U_DESIGN))),
        _ => Err(usage(
            format!("unknown design subcommand '{sub}'"),
            U_DESIGN,
        )),
    }
}

fn parse_u32_pos(v: &str, flag: &str) -> Result<u32, Fail> {
    let n = v
        .parse::<u32>()
        .map_err(|_| usage(format!("bad {flag} '{v}'"), U_DESIGN))?;
    if n == 0 {
        return Err(usage(format!("{flag} must be at least 1"), U_DESIGN));
    }
    Ok(n)
}

fn parse_refs(raw: &str, out: &mut Vec<u64>) -> Result<(), Fail> {
    if raw.trim().is_empty() {
        return Err(usage("empty --refs, want <seq>[,<seq>...]", U_DESIGN));
    }
    for part in raw.split(',') {
        let token = part.trim().strip_prefix('#').unwrap_or(part.trim());
        let seq = token.parse::<u64>().map_err(|_| {
            usage(
                format!("bad --refs '{raw}', want <seq>[,<seq>...]"),
                U_DESIGN,
            )
        })?;
        out.push(seq);
    }
    Ok(())
}

fn parse_slot(v: &str) -> Result<DesignSlot, Fail> {
    match v {
        "proposal" => Ok(DesignSlot::Proposal),
        "buildability" => Ok(DesignSlot::Buildability),
        "summary" => Ok(DesignSlot::Summary),
        _ => Err(usage(
            format!("bad --slot '{v}', want proposal|buildability|summary"),
            U_DESIGN,
        )),
    }
}

fn parse_question_input(raw: String) -> Result<QuestionInput, Fail> {
    if let Some(path) = raw.strip_prefix('@') {
        if path.trim().is_empty() {
            return Err(usage("bad --question '@', missing file path", U_DESIGN));
        }
        Ok(QuestionInput::File(PathBuf::from(path)))
    } else {
        Ok(QuestionInput::Inline(raw))
    }
}

fn open(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut req = None;
    let mut suggest_skip = false;
    let mut reason: Option<String> = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_DESIGN)));
        }
        if common_flag(it, common, &arg, U_DESIGN)? {
            continue;
        }
        match arg.as_str() {
            "--req" => req = Some(parse_u32_pos(&it.value("--req", U_DESIGN)?, "--req")?),
            _ if eq(&arg, "--req").is_some() => {
                req = Some(parse_u32_pos(eq(&arg, "--req").unwrap(), "--req")?)
            }
            "--suggest-skip" => suggest_skip = true,
            "--reason" => reason = Some(it.value("--reason", U_DESIGN)?),
            _ if eq(&arg, "--reason").is_some() => reason = eq(&arg, "--reason").map(String::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_DESIGN)),
        }
    }
    if reason.is_some() && !suggest_skip {
        return Err(usage("--reason requires --suggest-skip", U_DESIGN));
    }
    if let Some(r) = &reason
        && r.trim().is_empty()
    {
        return Err(usage("--reason must not be empty", U_DESIGN));
    }
    Ok((
        std::mem::take(common),
        Cmd::Design(DesignCmd::Open {
            req,
            suggest_skip,
            reason,
        }),
    ))
}

fn write(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut slot = None;
    let mut file = None;
    let mut refs = Vec::new();
    let mut questions = Vec::new();
    let mut questions_file = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_DESIGN)));
        }
        if common_flag(it, common, &arg, U_DESIGN)? {
            continue;
        }
        match arg.as_str() {
            "--slot" => slot = Some(parse_slot(&it.value("--slot", U_DESIGN)?)?),
            _ if eq(&arg, "--slot").is_some() => {
                slot = Some(parse_slot(eq(&arg, "--slot").unwrap())?)
            }
            "--file" => file = Some(PathBuf::from(it.value("--file", U_DESIGN)?)),
            _ if eq(&arg, "--file").is_some() => file = eq(&arg, "--file").map(PathBuf::from),
            "--refs" => parse_refs(&it.value("--refs", U_DESIGN)?, &mut refs)?,
            _ if eq(&arg, "--refs").is_some() => {
                parse_refs(eq(&arg, "--refs").unwrap(), &mut refs)?
            }
            "--question" => {
                questions.push(parse_question_input(it.value("--question", U_DESIGN)?)?)
            }
            _ if eq(&arg, "--question").is_some() => questions.push(parse_question_input(
                eq(&arg, "--question").unwrap().into(),
            )?),
            "--questions-file" => {
                questions_file = Some(PathBuf::from(it.value("--questions-file", U_DESIGN)?))
            }
            _ if eq(&arg, "--questions-file").is_some() => {
                questions_file = eq(&arg, "--questions-file").map(PathBuf::from)
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_DESIGN)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Design(DesignCmd::Write {
            slot: slot.ok_or_else(|| usage("missing --slot", U_DESIGN))?,
            file: file.ok_or_else(|| usage("missing --file", U_DESIGN))?,
            refs,
            questions,
            questions_file,
        }),
    ))
}

fn supplement(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut file = None;
    let mut refs = Vec::new();
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_DESIGN)));
        }
        if common_flag(it, common, &arg, U_DESIGN)? {
            continue;
        }
        match arg.as_str() {
            "--file" => file = Some(PathBuf::from(it.value("--file", U_DESIGN)?)),
            _ if eq(&arg, "--file").is_some() => file = eq(&arg, "--file").map(PathBuf::from),
            "--refs" => parse_refs(&it.value("--refs", U_DESIGN)?, &mut refs)?,
            _ if eq(&arg, "--refs").is_some() => {
                parse_refs(eq(&arg, "--refs").unwrap(), &mut refs)?
            }
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_DESIGN)),
        }
    }
    if refs.is_empty() {
        return Err(usage("missing --refs", U_DESIGN));
    }
    Ok((
        std::mem::take(common),
        Cmd::Design(DesignCmd::Supplement {
            file: file.ok_or_else(|| usage("missing --file", U_DESIGN))?,
            refs,
        }),
    ))
}

fn note(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut from = None;
    let mut src = Source::default();
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_DESIGN)));
        }
        if common_flag(it, common, &arg, U_DESIGN)? {
            continue;
        }
        if src.flag(it, &arg, U_DESIGN)? {
            continue;
        }
        match arg.as_str() {
            "--from" => from = Some(it.value("--from", U_DESIGN)?),
            _ if eq(&arg, "--from").is_some() => from = eq(&arg, "--from").map(String::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_DESIGN)),
        }
    }
    let from = from.ok_or_else(|| usage("missing --from", U_DESIGN))?;
    if from != "user" {
        return Err(usage(format!("bad --from '{from}', want user"), U_DESIGN));
    }
    Ok((
        std::mem::take(common),
        Cmd::Design(DesignCmd::Note {
            from,
            source: src.one(U_DESIGN)?,
        }),
    ))
}

fn sign(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut skip = false;
    let mut reason: Option<String> = None;
    let mut note: Option<String> = None;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_DESIGN)));
        }
        if common_flag(it, common, &arg, U_DESIGN)? {
            continue;
        }
        match arg.as_str() {
            "--skip" => skip = true,
            "--reason" => reason = Some(it.value("--reason", U_DESIGN)?),
            _ if eq(&arg, "--reason").is_some() => reason = eq(&arg, "--reason").map(String::from),
            "--note" => note = Some(it.value("--note", U_DESIGN)?),
            _ if eq(&arg, "--note").is_some() => note = eq(&arg, "--note").map(String::from),
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_DESIGN)),
        }
    }
    if reason.is_some() && !skip {
        return Err(usage("--reason requires --skip", U_DESIGN));
    }
    if let Some(r) = &reason
        && r.trim().is_empty()
    {
        return Err(usage("--reason must not be empty", U_DESIGN));
    }
    Ok((
        std::mem::take(common),
        Cmd::Design(DesignCmd::Sign { skip, reason, note }),
    ))
}

fn show(common: &mut Common, it: &mut It) -> Result<(Common, Cmd), Fail> {
    let mut version = None;
    let mut round = None;
    let mut json = false;
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok((std::mem::take(common), Cmd::Help(U_DESIGN)));
        }
        if common_flag(it, common, &arg, U_DESIGN)? {
            continue;
        }
        match arg.as_str() {
            "--version" => {
                version = Some(parse_u32_pos(
                    &it.value("--version", U_DESIGN)?,
                    "--version",
                )?)
            }
            _ if eq(&arg, "--version").is_some() => {
                version = Some(parse_u32_pos(eq(&arg, "--version").unwrap(), "--version")?)
            }
            "--round" => round = Some(parse_u32_pos(&it.value("--round", U_DESIGN)?, "--round")?),
            _ if eq(&arg, "--round").is_some() => {
                round = Some(parse_u32_pos(eq(&arg, "--round").unwrap(), "--round")?)
            }
            "--json" => json = true,
            _ => return Err(usage(format!("unexpected argument '{arg}'"), U_DESIGN)),
        }
    }
    Ok((
        std::mem::take(common),
        Cmd::Design(DesignCmd::Show {
            version,
            round,
            json,
        }),
    ))
}

fn parse_question_json(raw: &str, label: &str) -> Result<Vec<DesignQuestion>, Fail> {
    let val: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| Fail::usage(format!("bad {label} JSON: {e}")))?;
    if val.is_array() {
        serde_json::from_value(val).map_err(|e| Fail::usage(format!("bad {label} array: {e}")))
    } else {
        let q: DesignQuestion =
            serde_json::from_value(val).map_err(|e| Fail::usage(format!("bad {label}: {e}")))?;
        Ok(vec![q])
    }
}

pub(crate) fn load_questions(
    inputs: &[QuestionInput],
    questions_file: Option<&Path>,
) -> Result<Vec<DesignQuestion>, Fail> {
    let mut out = Vec::new();
    for item in inputs {
        match item {
            QuestionInput::Inline(raw) => out.extend(parse_question_json(raw, "--question")?),
            QuestionInput::File(path) => {
                let raw = fs::read_to_string(path).map_err(|e| {
                    Fail::refused(format!("cannot read --question {}: {e}", path.display()))
                })?;
                out.extend(parse_question_json(&raw, "--question")?);
            }
        }
    }
    if let Some(path) = questions_file {
        let raw = fs::read_to_string(path).map_err(|e| {
            Fail::refused(format!(
                "cannot read --questions-file {}: {e}",
                path.display()
            ))
        })?;
        out.extend(parse_question_json(&raw, "--questions-file")?);
    }
    if out.len() > MAX_QUESTIONS {
        return Err(Fail::refused(format!(
            "design summary accepts at most {MAX_QUESTIONS} questions, got {}",
            out.len()
        )));
    }
    Ok(out)
}
