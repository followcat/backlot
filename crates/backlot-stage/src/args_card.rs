//! Parser for `backlot card list|show|apply|cancel` and `--answers`.

use crate::args::*;
use crate::cli::Fail;

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

pub(crate) const U_CARD: &str = "usage:
  backlot card list
  backlot card show <id>
  backlot card apply <id>
  backlot card cancel <id> [--reason <r>]";

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

pub(crate) fn parse_card_id(v: &str) -> Option<u64> {
    v.strip_prefix('c').unwrap_or(v).parse::<u64>().ok()
}

pub(crate) fn parse_answers(raw: &str, out: &mut Vec<u64>, help: &'static str) -> Result<(), Fail> {
    if raw.is_empty() {
        return Err(usage("empty --answers, want <seq>[,<seq>]", help));
    }
    for part in raw.split(',') {
        let seq = part
            .parse::<u64>()
            .map_err(|_| usage(format!("bad --answers '{raw}', want <seq>[,<seq>]"), help))?;
        out.push(seq);
    }
    Ok(())
}
