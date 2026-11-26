//! Opening design command handlers (`backlot design ...`) shared by the
//! local CLI and `plugin.backlot.req` remote queue.

use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::design::{DesignPhase, DesignQuestion, DesignSlot, EntrySource, SignMode};
use backlot_model::req_results::{DesignSignResult, DesignStartRoundResult};
use backlot_model::{EventKind, NewEvent};

use crate::args::TextSource;
use crate::args_design::{DesignCmd, QuestionInput};
use crate::cli::{Caller, Env, Fail};
pub(crate) use crate::design_doc::design_show;
use crate::design_doc::load_questions;
use crate::render;
use crate::store::{self, Pending, PendingFile};
use crate::verbs::require_studio_dir;

pub(crate) fn run(
    base: &Path,
    _env: &Env,
    caller: &Caller,
    cmd: DesignCmd,
) -> Result<String, Fail> {
    match cmd {
        DesignCmd::Open {
            req,
            suggest_skip,
            reason,
        } => Ok(format_design_open(&design_open(
            base,
            caller,
            req,
            suggest_skip,
            reason,
        )?)),
        DesignCmd::Write {
            slot,
            file,
            refs,
            questions,
            questions_file,
        } => design_write(base, caller, slot, file, refs, questions, questions_file),
        DesignCmd::Supplement { file, refs } => design_supplement(base, caller, file, refs),
        DesignCmd::Note { from: _, source } => design_note(base, caller, source),
        DesignCmd::Next => Ok(format_design_open(&design_next(base, caller)?)),
        DesignCmd::Sign { skip, reason, note } => Ok(format_design_sign(&design_sign(
            base, caller, skip, reason, note,
        )?)),
        DesignCmd::Show { .. } => {
            unreachable!("`design show` is dispatched through hwood_read::read")
        }
    }
}

pub(crate) fn design_open(
    base: &Path,
    caller: &Caller,
    req: Option<u32>,
    suggest_skip: bool,
    reason: Option<String>,
) -> Result<DesignStartRoundResult, Fail> {
    open_round(base, caller, req, suggest_skip, reason, false)
}

pub(crate) fn design_start_round(
    base: &Path,
    caller: &Caller,
) -> Result<DesignStartRoundResult, Fail> {
    open_round(base, caller, None, false, None, true)
}

fn check_reason_flag(enabled: bool, reason: Option<&str>, flag: &str) -> Result<(), Fail> {
    if reason.is_some() && !enabled {
        return Err(Fail::usage(format!("--reason requires {flag}")));
    }
    if reason.is_some_and(|r| r.trim().is_empty()) {
        return Err(Fail::usage("--reason must not be empty"));
    }
    Ok(())
}

fn append_opened(
    dir: &Path,
    caller: &Caller,
    build: impl FnOnce(&backlot_model::state::StudioState) -> Result<(u32, u32), Fail>,
    suggest_skip: bool,
    reason: Option<String>,
) -> Result<DesignStartRoundResult, Fail> {
    let by = caller.by();
    let event = store::append(dir, move |state, _| {
        let (version, round) = build(state)?;
        Ok::<Pending, Fail>(Pending {
            event: NewEvent::new(
                by,
                EventKind::DesignOpened {
                    version,
                    round,
                    suggest_skip,
                    reason,
                    doc_file: format!("design/v{version}.md"),
                },
            ),
            files: Vec::new(),
        })
    })?;
    match event.kind {
        EventKind::DesignOpened {
            version,
            round,
            doc_file,
            ..
        } => Ok(DesignStartRoundResult {
            studio: caller.studio.clone(),
            seq: event.seq,
            version,
            round,
            doc_file,
        }),
        other => Err(Fail::refused(format!("unexpected event: {other:?}"))),
    }
}

fn open_round(
    base: &Path,
    caller: &Caller,
    req: Option<u32>,
    suggest_skip: bool,
    reason: Option<String>,
    allow_waiting_user: bool,
) -> Result<DesignStartRoundResult, Fail> {
    check_reason_flag(suggest_skip, reason.as_deref(), "--suggest-skip")?;
    let dir = store::studio_dir(base, &caller.studio);
    require_studio_dir(&dir, &caller.studio)?;
    append_opened(
        &dir,
        caller,
        move |state| {
            let version = req.unwrap_or(state.requirement_version);
            let round = if suggest_skip {
                0
            } else if allow_waiting_user && state.design_phase() == DesignPhase::WaitingUser {
                state.design.as_ref().map_or(1, |d| d.round + 1)
            } else {
                1
            };
            Ok((version, round))
        },
        suggest_skip,
        reason,
    )
}

pub(crate) fn design_next(base: &Path, caller: &Caller) -> Result<DesignStartRoundResult, Fail> {
    let dir = store::studio_dir(base, &caller.studio);
    require_studio_dir(&dir, &caller.studio)?;
    let role = caller.role.clone();
    append_opened(
        &dir,
        caller,
        move |state| {
            if !matches!(role.as_str(), "producer" | "user") {
                return Err(Fail::refused(format!(
                    "role '{role}' cannot do this; the user or the producer can"
                )));
            }
            let version = state.requirement_version;
            let Some(d) = state.design.as_ref().filter(|d| d.version == version) else {
                return Err(Fail::refused(format!(
                    "no active opening design for req v{version}; run 'backlot design open' first"
                )));
            };
            if matches!(d.phase, DesignPhase::Idle | DesignPhase::SuggestSkip) || d.round == 0 {
                return Err(Fail::refused(format!(
                    "opening design v{version} has no active round; run 'backlot design open' first"
                )));
            }
            Ok((version, d.round + 1))
        },
        false,
        None,
    )
}

pub(crate) fn design_write(
    base: &Path,
    caller: &Caller,
    slot: DesignSlot,
    file: PathBuf,
    refs: Vec<u64>,
    questions: Vec<QuestionInput>,
    questions_file: Option<PathBuf>,
) -> Result<String, Fail> {
    if !matches!(
        slot,
        DesignSlot::Proposal | DesignSlot::Buildability | DesignSlot::Summary
    ) {
        return Err(Fail::usage(
            "bad --slot, want proposal|buildability|summary",
        ));
    }
    let parsed_questions = load_questions(&questions, questions_file.as_deref())?;
    if !parsed_questions.is_empty() && slot != DesignSlot::Summary {
        return Err(Fail::refused("only summary entries may carry questions"));
    }
    append_entry(base, caller, slot, file, None, refs, parsed_questions)
}

pub(crate) fn design_supplement(
    base: &Path,
    caller: &Caller,
    file: PathBuf,
    refs: Vec<u64>,
) -> Result<String, Fail> {
    if refs.is_empty() {
        return Err(Fail::usage("missing --refs"));
    }
    append_entry(
        base,
        caller,
        DesignSlot::Supplement,
        file,
        None,
        refs,
        Vec::new(),
    )
}

pub(crate) fn design_note(
    base: &Path,
    caller: &Caller,
    source: TextSource,
) -> Result<String, Fail> {
    let TextSource::File(file) = source else {
        return Err(Fail::refused("internal: unresolved design note source"));
    };
    let entry_source = (caller.role == "producer").then_some(EntrySource::ProducerNote);
    append_entry(
        base,
        caller,
        DesignSlot::UserInput,
        file,
        entry_source,
        Vec::new(),
        Vec::new(),
    )
}

fn append_entry(
    base: &Path,
    caller: &Caller,
    slot: DesignSlot,
    file: PathBuf,
    source: Option<EntrySource>,
    refs: Vec<u64>,
    questions: Vec<DesignQuestion>,
) -> Result<String, Fail> {
    let body = fs::read_to_string(&file)
        .map_err(|e| Fail::refused(format!("cannot read --file {}: {e}", file.display())))?;
    if body.trim().is_empty() {
        return Err(Fail::refused("design entry file is empty"));
    }
    let dir = store::studio_dir(base, &caller.studio);
    require_studio_dir(&dir, &caller.studio)?;
    let by = caller.by();
    let slug = match slot {
        DesignSlot::Proposal => "proposal",
        DesignSlot::Buildability => "buildability",
        DesignSlot::Summary => "summary",
        DesignSlot::UserInput => "user_input",
        DesignSlot::Supplement => "supplement",
    };
    let event = store::append(&dir, move |state, seq| {
        let version = state.requirement_version;
        let round = state
            .design
            .as_ref()
            .filter(|d| d.version == version)
            .map_or(1, |d| d.round);
        let dest = format!("design/v{version}/r{round}-{seq}-{slug}.md");
        Ok::<Pending, Fail>(Pending {
            event: NewEvent::new(
                by,
                EventKind::DesignEntry {
                    version,
                    round,
                    slot,
                    file: dest.clone(),
                    source,
                    refs,
                    questions,
                },
            ),
            files: vec![PendingFile {
                src: file,
                dest,
                append: None,
            }],
        })
    })?;
    Ok(render::confirm(&event))
}

pub(crate) fn design_sign(
    base: &Path,
    caller: &Caller,
    skip: bool,
    reason: Option<String>,
    note: Option<String>,
) -> Result<DesignSignResult, Fail> {
    if caller.role != "user" {
        return Err(Fail::refused(format!(
            "role '{}' cannot do this; only the user can",
            caller.role
        )));
    }
    check_reason_flag(skip, reason.as_deref(), "--skip")?;
    let dir = store::studio_dir(base, &caller.studio);
    require_studio_dir(&dir, &caller.studio)?;
    let mode = if skip {
        SignMode::Skipped
    } else {
        SignMode::Approved
    };
    let recorded_note = note.or(reason);
    let by = caller.by();
    let event = store::append(&dir, move |state, _| {
        let version = state.requirement_version;
        let round = state
            .design
            .as_ref()
            .filter(|d| d.version == version)
            .map_or(0, |d| d.round);
        let doc_file = if skip {
            String::new()
        } else {
            format!("design/v{version}-final.md")
        };
        Ok::<Pending, Fail>(Pending {
            event: NewEvent::new(
                by,
                EventKind::DesignSigned {
                    version,
                    round,
                    mode,
                    doc_file,
                    note: recorded_note,
                },
            ),
            files: Vec::new(),
        })
    })?;
    match event.kind {
        EventKind::DesignSigned {
            version,
            round,
            mode,
            doc_file,
            ..
        } => Ok(DesignSignResult {
            studio: caller.studio.clone(),
            seq: event.seq,
            version,
            round,
            mode,
            doc_file,
        }),
        other => Err(Fail::refused(format!("unexpected event: {other:?}"))),
    }
}

pub(crate) fn format_design_open(res: &DesignStartRoundResult) -> String {
    if res.round == 0 {
        format!(
            "event {}: design.opened v{} (suggest-skip)",
            res.seq, res.version
        )
    } else {
        format!(
            "event {}: design.opened v{} r{}",
            res.seq, res.version, res.round
        )
    }
}

pub(crate) fn format_design_sign(res: &DesignSignResult) -> String {
    let mode = match res.mode {
        SignMode::Approved => "approved",
        SignMode::Skipped => "skipped",
    };
    format!(
        "event {}: design.signed v{} r{} {mode}",
        res.seq, res.version, res.round
    )
}
