//! The S3 native dialogs' model (§2, A6.2): what each kind asks,
//! what its Enter/y answers, and the multi-line editor the Tell and
//! Revise bodies share. The app owns opening, keys, mouse and the
//! submit — this file keeps the data and the small pure rules so the
//! render and the tests read them without an `App`.

use crate::chrome::text_editor::TextEditor;
use crate::studio::StudioKey;

mod lifecycle;
pub(crate) use lifecycle::{RecastDialog, RecastStep, RestartDialog, RestoreDialog, RestoreStep};

/// A reason-taking command (§2.1) — the dialog's Enter builds the
/// action its `for` names.
#[derive(Debug, Clone)]
pub(crate) enum ReasonFor {
    /// `task <verb> --task <id> --reason <r>` — pause/resume/abandon
    /// require the reason; takeover's stays optional in the parser.
    TaskVerb {
        key: StudioKey,
        task: String,
        verb: &'static str,
    },
    /// `studio finish [--reason <r>]` — optional.
    Finish { key: StudioKey },
    /// `card cancel <id> [--reason <r>]` — optional.
    CardCancel { machine: Option<String>, id: String },
}

/// A confirmed command (§2.2).
#[derive(Debug, Clone)]
pub(crate) enum ConfirmFor {
    /// `close --person <role>` — "the role's terminal is removed…".
    Close { key: StudioKey, role: String },
    /// `stage stop` (G1) — "every studio on this machine stops
    /// advancing…". The stage serves the whole machine, so the row
    /// lives on the machine menu.
    StageStop { machine: Option<String> },
    /// `attention clear <seq>` — a stale prompt the role already
    /// left behind.
    ClearAttention {
        key: StudioKey,
        role: String,
        seq: u64,
    },
    /// The D9 override — a director-routed prompt the user answers
    /// after confirming (`backlot approve --as user`).
    DirOverride {
        key: StudioKey,
        role: String,
        allow: bool,
    },
    /// `studio delete <id> [--force] --json --as user` — finishes if
    /// live, closes terminals, prunes safely, and archives the ledger.
    /// `token` is the plan's confirm token when the dialog reviewed
    /// one over a server.
    DeleteStudio {
        key: StudioKey,
        force: bool,
        kept: Vec<String>,
        token: Option<String>,
    },
    /// `studio finish --force` on a studio with no requirement and no
    /// tasks — the L5 guard, said in plain words first.
    FinishEmpty { key: StudioKey },
    /// `studio prune <studio> --as user` after its `--dry-run` plan.
    Prune {
        key: StudioKey,
        token: Option<String>,
    },
}

/// What the multi-line dialogs' focus rings walk through — Tab
/// advances, Shift-Tab backs up, Enter on Send submits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogField {
    Body,
    Summary,
    Send,
}

/// `Tell <role> · <studio>[ · <machine>]` (§2.3).
pub(crate) struct TellDialog {
    /// The dialog title line — `Tell alice · acme-shop · host-b`.
    pub(crate) title: String,
    pub(crate) key: StudioKey,
    /// The role key `backlot tell` takes.
    pub(crate) role: String,
    pub(crate) body: BodyEditor,
    pub(crate) field: DialogField,
    /// A refused submit's inline error (`empty message`).
    pub(crate) error: Option<String>,
}

/// `Revise requirement (v<n> → v<n+1>) · <studio>[ · <machine>]` (§2.4).
pub(crate) struct ReviseDialog {
    pub(crate) title: String,
    pub(crate) key: StudioKey,
    /// `None` while the prefill fetch is in flight; `Some` once the
    /// body editor holds the current text (empty on a failed fetch —
    /// the error field explains).
    pub(crate) body: Option<BodyEditor>,
    pub(crate) summary: TextEditor,
    pub(crate) field: DialogField,
    /// `^E` is local-only — a remote studio's body stays in the
    /// dialog's own editor.
    pub(crate) local: bool,
    /// A failed prefill or a refused submit explains itself here.
    pub(crate) error: Option<String>,
    /// True when the base requirement read failed (`from > 0`).
    pub(crate) prefill_failed: bool,
    /// Armed by a first submit attempt after `prefill_failed`, so only
    /// a second explicit submit overwrites without the base text.
    pub(crate) confirm_overwrite: bool,
    /// Bumps per `$EDITOR` write so a closed editor's late file is
    /// never re-read.
    pub(crate) editor_gen: u64,
}

/// `Pause task t1 · acme-shop · Local` (§2.1).
pub(crate) struct ReasonDialog {
    pub(crate) title: String,
    pub(crate) reason: TextEditor,
    /// The error an empty required submit shows.
    pub(crate) required: bool,
    pub(crate) error: Option<String>,
    pub(crate) for_: ReasonFor,
}

/// `[Enter / y] Confirm · [Esc] Cancel` (§2.2).
pub(crate) struct ConfirmDialog {
    pub(crate) title: String,
    /// The explanation the spec sets per kind.
    pub(crate) body: String,
    /// When true, the Confirm key/button is disabled while a dry-run loads.
    pub(crate) loading: bool,
    /// A refused confirm — a stale server plan — shown in the dialog;
    /// `r`/Enter reviews the plan again.
    pub(crate) error: Option<String>,
    pub(crate) for_: ConfirmFor,
}

/// Every dialog kind.
pub(crate) enum StudioDialog {
    Reason(ReasonDialog),
    Confirm(ConfirmDialog),
    Tell(TellDialog),
    Revise(ReviseDialog),
    Restart(RestartDialog),
    Recast(RecastDialog),
    Restore(RestoreDialog),
}

impl StudioDialog {
    /// `Reason <verb> task t1 · acme-shop · Local` titles and the
    /// shared `reason: ` line.
    pub(crate) fn reason(title: String, required: bool, for_: ReasonFor) -> StudioDialog {
        StudioDialog::Reason(ReasonDialog {
            title,
            reason: TextEditor::new("", false),
            required,
            error: None,
            for_,
        })
    }

    /// The confirm card — its body is the spec's wording per kind.
    pub(crate) fn confirm(title: String, body: String, for_: ConfirmFor) -> StudioDialog {
        StudioDialog::Confirm(ConfirmDialog {
            title,
            body,
            loading: false,
            error: None,
            for_,
        })
    }

    /// The machine suffix every §2 title shares: `· host-b`, or `·
    /// Local` when the studio sits on this core.
    pub(crate) fn place(key: &StudioKey) -> String {
        format!("{} · {}", key.id, key.machine.as_deref().unwrap_or("Local"))
    }

    /// The D9 override confirm (§2.2): a director-routed prompt the
    /// user answers themself, worded as the spec sets it.
    pub(crate) fn dir_override(key: &StudioKey, role: &str, allow: bool) -> StudioDialog {
        StudioDialog::Confirm(ConfirmDialog {
            title: format!("Answer {}'s prompt? · {}", role, Self::place(key)),
            body: format!(
                "This prompt is routed to the director. Answering \
                 {}s it as the user — the director's own wait clears \
                 with it.",
                if allow { "allow" } else { "deny" }
            ),
            loading: false,
            error: None,
            for_: ConfirmFor::DirOverride {
                key: key.clone(),
                role: role.to_owned(),
                allow,
            },
        })
    }

    /// First confirmation for `studio delete`, opened immediately while
    /// `--dry-run --json` inspects the studio.
    pub(crate) fn delete_studio_loading(key: &StudioKey) -> StudioDialog {
        StudioDialog::Confirm(ConfirmDialog {
            title: format!("Delete studio · {}", Self::place(key)),
            body: "Inspecting studio…".to_owned(),
            loading: true,
            error: None,
            for_: ConfirmFor::DeleteStudio {
                key: key.clone(),
                force: false,
                kept: Vec::new(),
                token: None,
            },
        })
    }

    /// Second confirmation after `studio delete` kept dirty worktrees or
    /// unmerged branches.
    pub(crate) fn delete_studio_force(key: &StudioKey, kept: &[String]) -> StudioDialog {
        StudioDialog::Confirm(ConfirmDialog {
            title: format!("Force delete studio? · {}", Self::place(key)),
            body: format!(
                "Kept: {}. Pass --force to discard dirty worktrees and unmerged \
                 branches and archive the ledger? Your repository is not touched.",
                kept.join("; ")
            ),
            loading: false,
            error: None,
            for_: ConfirmFor::DeleteStudio {
                key: key.clone(),
                force: true,
                kept: Vec::new(),
                token: None,
            },
        })
    }

    /// A server plan went stale under the confirm: the dialog stays,
    /// says why, and Enter/`r` reviews the plan again.
    pub(crate) fn confirm_stale(
        title: String,
        body: String,
        error: String,
        for_: ConfirmFor,
    ) -> StudioDialog {
        StudioDialog::Confirm(ConfirmDialog {
            title,
            body,
            loading: false,
            error: Some(error),
            for_,
        })
    }
}

mod body_editor;
pub(crate) use body_editor::BodyEditor;

#[cfg(test)]
#[path = "studio_dialog_tests.rs"]
mod tests;
