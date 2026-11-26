//! `Edit cast…` — the studio's effective cast in `$VISUAL`/`$EDITOR`,
//! checked with `studio cast set --dry-run`, then recorded on confirm.
//!
//! The flow is the same shape on a local core and on a server: the cast
//! file is fetched as text (`studio cast show`), edited in a temp copy,
//! and set with the file's **content** — the queue path carries content,
//! never a client path, so the TUI and the CLI agree. A refusal (a
//! runner the machine has not got, a cap that leaves a seat empty) shows
//! in the dialog with its reason; a file that says what the studio
//! already plays says "nothing changes".

use crate::studio::StudioKey;

/// Where the cast editor stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CastEditStep {
    /// Fetching the effective cast (`studio cast show`).
    Loading,
    /// The fetched cast is open in the person's editor; the dialog waits.
    Editing,
    /// Checking the edited file with `studio cast set --dry-run`.
    Checking,
    /// The diff a `--dry-run` made, shown for confirmation.
    Diff,
}

/// `Edit cast · acme-shop · Local`.
#[derive(Debug, Clone)]
pub(crate) struct CastEditDialog {
    pub(crate) title: String,
    pub(crate) key: StudioKey,
    pub(crate) step: CastEditStep,
    /// The edited cast file's text, once fetched and loaded into the
    /// editor — what `set --file -` is given as content.
    pub(crate) cast: String,
    /// The dry-run output, shown verbatim in the confirm step.
    pub(crate) diff: String,
    /// A refusal or read failure, shown in the dialog with its reason.
    pub(crate) error: Option<String>,
    /// True once the dialog holds a diff worth applying.
    pub(crate) ready: bool,
    /// True while a fetch (show or dry run) is in flight — Enter waits.
    pub(crate) busy: bool,
    /// Bumps per run so a late fetch reply for a closed dialog is
    /// dropped (the editor round trip and the dry-run both key on it).
    pub(crate) epoch: u64,
}

impl CastEditDialog {
    pub(crate) fn new(key: &StudioKey) -> CastEditDialog {
        CastEditDialog {
            title: format!(
                "Edit cast · {}",
                crate::studio_dialog::StudioDialog::place(key)
            ),
            key: key.clone(),
            step: CastEditStep::Loading,
            cast: String::new(),
            diff: String::new(),
            error: None,
            ready: false,
            busy: true,
            epoch: 0,
        }
    }

    /// The cast editor opened on `text`: the dialog now waits for the
    /// editor to close.
    pub(crate) fn editing(&mut self, text: String) {
        self.step = CastEditStep::Editing;
        self.cast = text;
        self.error = None;
        self.ready = false;
        self.busy = false;
    }

    /// The dry-run is in flight: Enter waits.
    pub(crate) fn checking(&mut self) {
        self.step = CastEditStep::Checking;
        self.busy = true;
        self.error = None;
    }

    /// The dry-run landed: a refusal stays in the dialog, a diff readies
    /// the confirm. An empty diff is "nothing changes".
    pub(crate) fn dry_run(&mut self, reply: Result<String, String>) {
        self.busy = false;
        self.step = CastEditStep::Diff;
        match reply {
            Ok(text) => {
                self.ready = !text.trim().is_empty();
                self.diff = text;
                self.error = None;
            }
            Err(err) => {
                self.diff.clear();
                self.ready = false;
                self.error = Some(err);
            }
        }
    }

    /// A failed or refused read at the open: the dialog says why and
    /// offers nothing to apply.
    pub(crate) fn failed(&mut self, error: String) {
        self.busy = false;
        self.step = CastEditStep::Diff;
        self.diff.clear();
        self.ready = false;
        self.error = Some(error);
    }
}
