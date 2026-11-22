//! The S3 ActionTracker (§3, A7.1): every `backlot` call the TUI
//! starts — dock, Call Sheet, context menus, the native dialogs —
//! goes through this one lifecycle. `start` marks the target row
//! with the spinner and dim label at once and ignores a second
//! press on the same row; the reply answers a 3.5 s success toast
//! or an 8 s failure card carrying the first `stderr` line and the
//! `copy cmd` action. The owning machine is named on every line.

use std::time::Duration;

/// The success toast's hold.
pub(crate) const DONE_HOLD: Duration = Duration::from_millis(3500);
/// The failure card's hold — `Esc`, a click or `C` dismisses sooner.
pub(crate) const FAIL_HOLD: Duration = Duration::from_secs(8);

/// One `backlot` invocation. `mark` is the target row's identity —
/// dedup and the dim mark both key on it.
#[derive(Debug, Clone)]
pub(crate) struct Action {
    /// Pending text: `pausing t1`, `applying c3`.
    pub label: String,
    /// Success toast text: `paused task t1`, `applied card c3`.
    pub done: String,
    /// Failure title text: `pause task t1`, `apply card c3`.
    pub name: String,
    /// The row identity — `task:Local:acme-shop:t1`, `card:c3`.
    pub mark: String,
    /// `backlot <argv>`; a local run's endpoint flags ride inside.
    pub argv: Vec<String>,
    /// The studio's machine — `None` is this core.
    pub machine: Option<String>,
    /// A piped body (`req revise --stdin`), when the action has one.
    pub stdin: Option<String>,
    /// `card show`: `Ok(stdout)` opens the read-only view instead of
    /// a toast.
    pub view: bool,
}

/// A started action with its id and the row's pending text.
#[derive(Debug, Clone)]
pub(crate) struct Pending {
    pub id: u64,
    pub action: Action,
    /// `<label>` or `<label> on <machine>` — the row's dim text.
    pub text: String,
}

/// The pending set and the id sequence.
#[derive(Default)]
pub(crate) struct ActionTracker {
    seq: u64,
    pub pending: Vec<Pending>,
}

impl ActionTracker {
    /// `mark` already runs — the tests' shape for asking.
    #[cfg(test)]
    pub(crate) fn is_pending(&self, mark: &str) -> bool {
        self.pending.iter().any(|p| p.action.mark == mark)
    }

    /// Seat `action` and answer its id — `None` while a same-mark
    /// action is in flight.
    pub(crate) fn start(&mut self, action: Action) -> Option<u64> {
        // A duplicate press — the same command on the same target —
        // is ignored while it runs; a different action on the row
        // (apply, then cancel) still goes through.
        if self
            .pending
            .iter()
            .any(|p| p.action.mark == action.mark && p.action.argv == action.argv)
        {
            return None;
        }
        self.seq += 1;
        let id = self.seq;
        let text = match &action.machine {
            Some(machine) => format!("{} on {machine}", action.label),
            None => action.label.clone(),
        };
        self.pending.push(Pending { id, action, text });
        Some(id)
    }

    /// The reply's action, off the pending list.
    pub(crate) fn finish(&mut self, id: u64) -> Option<Pending> {
        let pos = self.pending.iter().position(|p| p.id == id)?;
        Some(self.pending.remove(pos))
    }

    /// Every pending mark joined — the Call Sheet's
    /// `pending.contains(target_label)` row mark reads this, and a
    /// dock row's mark is one of them.
    pub(crate) fn marks(&self) -> String {
        self.pending
            .iter()
            .map(|p| p.action.mark.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `mark`'s pending text for the dim row label.
    pub(crate) fn pending_label(&self, mark: &str) -> Option<&str> {
        self.pending
            .iter()
            .find(|p| p.action.mark == mark)
            .map(|p| p.text.as_str())
    }
}

/// `host-b: paused task t1` — local drops the prefix (§3.2).
pub(crate) fn done_title(p: &Pending) -> String {
    match &p.action.machine {
        Some(machine) => format!("{machine}: {}", p.action.done),
        None => p.action.done.clone(),
    }
}

/// `host-b failed: pause task t1` / `failed: pause task t1` (§3.3).
pub(crate) fn fail_title(p: &Pending) -> String {
    match &p.action.machine {
        Some(machine) => format!("{machine} failed: {}", p.action.name),
        None => format!("failed: {}", p.action.name),
    }
}

/// The exact first non-empty line of the failure's `stderr` (§3.3).
pub(crate) fn fail_body(err: &str) -> String {
    err.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("command failed")
        .to_owned()
}

/// D17a: an argv word carrying a control byte — NUL, newline, ESC —
/// must never reach a spawned command or the ledger. The studio and
/// card commands build argv from summary-document strings, so this
/// is the exec boundary's check: `run_action` refuses to spawn when
/// a word (or the machine name) has one, and the copy line's
/// `sh_quote` is what makes the same hostile word safe to display.
/// Identifiers that spell a flag (`--force`, `-x`) stay exactly one
/// argv word — the backlot parser refuses them as unexpected
/// arguments — so bytes are the check, not position.
pub(crate) fn unsafe_word(w: &str) -> bool {
    w.chars().any(char::is_control)
}

/// POSIX single-quote escaping for one shell word.
fn sh_quote(word: &str) -> String {
    if word
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-._/:@+=".contains(c))
    {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// The exact command line `copy cmd` writes — `ssh <machine> --
/// backlot …` for a remote studio, `backlot …` locally (§3.3).
pub(crate) fn copy_line(p: &Pending) -> String {
    let argv = p
        .action
        .argv
        .iter()
        .map(|a| sh_quote(a))
        .collect::<Vec<_>>()
        .join(" ");
    match &p.action.machine {
        Some(machine) => format!("ssh {machine} -- backlot {argv}"),
        None => format!("backlot {argv}"),
    }
}

/// The dock/Call Sheet mark for one backlot object — the dedup key
/// and the substring the Call Sheet's `pending` mark matches.
pub(crate) mod mark {
    use crate::panel::Target;
    use crate::studio::StudioKey;

    /// `machine` names remote machines `Local` for this core — the
    /// Call Sheet's stage rows compare against it.
    fn machine(machine: Option<&str>) -> &str {
        machine.unwrap_or("Local")
    }

    pub(crate) fn studio(key: &StudioKey) -> String {
        format!("studio:{}:{}", machine(key.machine.as_deref()), key.id)
    }
    pub(crate) fn role(key: &StudioKey, role: &str) -> String {
        format!("role:{}:{}:{role}", machine(key.machine.as_deref()), key.id)
    }
    pub(crate) fn task(key: &StudioKey, task: &str) -> String {
        format!("task:{}:{}:{task}", machine(key.machine.as_deref()), key.id)
    }
    pub(crate) fn card(id: &str) -> String {
        format!("card:{id}")
    }
    pub(crate) fn stage(m: Option<&str>) -> String {
        format!("stage:{}", machine(m))
    }
    /// The input line's own mark — `tell hollywood`.
    pub(crate) fn hollywood() -> String {
        "hollywood".to_owned()
    }
    /// The setup dialog's write — `hollywood setup` marks apart from
    /// `tell`'s `hollywood` so the two never dedup each other.
    pub(crate) fn hollywood_setup() -> String {
        "hollywood:setup".to_owned()
    }
    /// One Hollywood report/message read — `hollywood show <seq>`.
    pub(crate) fn hollywood_entry(seq: u64) -> String {
        format!("hollywood:{seq}")
    }

    /// A dock `Target`'s mark — `None` targets mark nothing.
    pub(crate) fn of(target: &Target) -> Option<String> {
        match target {
            Target::Studio(key)
            | Target::Block(key)
            | Target::Strip(key)
            | Target::Fold(key)
            | Target::Input(key) => Some(studio(key)),
            Target::Person(key, r) => Some(role(key, r)),
            Target::Task(key, t) => Some(task(key, t)),
            Target::Stage(m) => Some(stage(m.as_deref())),
            Target::Setup | Target::None => None,
        }
    }
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
