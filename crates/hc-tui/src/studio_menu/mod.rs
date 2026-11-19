//! The studio context-menu model (S3 §1): the row kinds the dock,
//! the Call Sheet and `Ctrl+B m` open a menu for, each target's item
//! list in the spec's order and the action one press runs. Pure
//! data — the app fills the target facts from the same summary the
//! row drew from, so a menu always matches its row.

use backlot_model::state::TaskStatus;
use backlot_model::summary::{RoleSummary, StudioSummary, TaskSummary};

use crate::studio::StudioKey;

/// A role the menu addresses, copied from the summary — the model
/// owns its facts so a menu outlives the publish it was opened from.
#[derive(Debug, Clone)]
pub(crate) struct MenuRole {
    /// The role key `backlot` commands take (`producer`, `actor-alice`).
    pub role: String,
    /// The display name (`alice`) — the `Tell <name>` labels read it.
    pub name: String,
    /// `live` / `closed` / `lost`.
    pub state: String,
    /// A closed role with a saved session reopens.
    pub resumable: bool,
    /// The task the role works, when an actor is cast.
    pub task: Option<String>,
    /// Its live terminal, machine-qualified.
    pub terminal: Option<String>,
    /// An open attention's seq — `Clear stale attention #<seq>`.
    pub attention_seq: Option<u64>,
    /// A user-owned prompt (`y`/`d`); `false` is director-routed (D9).
    pub attention_user: bool,
}

impl From<&RoleSummary> for MenuRole {
    fn from(role: &RoleSummary) -> MenuRole {
        MenuRole {
            role: role.role.clone(),
            name: role.name.clone(),
            state: role.state.clone(),
            resumable: role.resumable,
            task: role.task.clone(),
            terminal: role.terminal.clone(),
            attention_seq: role.attention_seq,
            attention_user: role.attention_user,
        }
    }
}

/// A task row's facts — the pause/resume arms read `status`, the
/// priority items read `priority`.
#[derive(Debug, Clone)]
pub(crate) struct MenuTask {
    pub id: String,
    pub status: TaskStatus,
    pub priority: backlot_model::event::Priority,
}

impl From<&TaskSummary> for MenuTask {
    fn from(task: &TaskSummary) -> MenuTask {
        MenuTask {
            id: task.id.clone(),
            status: task.status,
            priority: task.priority,
        }
    }
}

/// Which spec row the menu belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuKind {
    /// A studio header, frame or strip — the studio items.
    Studio,
    /// A live person with no open prompt.
    Role,
    /// A person at a user-owned prompt (y/d).
    Prompt,
    /// A person at a director-routed prompt (D9).
    DirPrompt,
    /// A closed or lost person.
    Closed,
    /// A task row.
    Task,
    /// A Call Sheet card row.
    Card,
    /// A machine's stage row (Call Sheet `STAGE DOWN`).
    Stage,
}

/// What one press does — the app maps it to an action, a dialog or a
/// navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StudioAct {
    /// Enter's act — focus the row's live terminal (producer on a
    /// studio row).
    FocusTerminal,
    /// `p` — the peek screen.
    Peek,
    /// `v` on the studio — the Call Sheet / Task & Delivery Sheet.
    Sheet,
    /// `v` on a task — the Task & Delivery Sheet.
    TaskSheet,
    /// `y`/`d` on a user-owned prompt — `backlot approve` direct.
    Decide(bool),
    /// D9's override — a director-routed prompt the user answers
    /// after the confirm dialog.
    DirOverride(bool),
    /// `task <verb>` through the reason dialog (pause/resume/abandon)
    /// or directly (takeover's reason is optional — still dialogged).
    TaskVerb(&'static str),
    /// `priority --level` direct.
    Priority(backlot_model::event::Priority),
    /// `reopen --person` direct.
    Reopen,
    /// `close --person` through the confirm dialog.
    Close,
    /// `attention clear <seq>` through the confirm dialog.
    ClearAttention,
    /// `stage start` direct.
    StageStart,
    /// `stage stop` (G1) through the confirm dialog.
    StageStop,
    /// `studio finish` through the reason dialog (reason optional).
    Finish,
    /// `tell <role> --text` through the Tell dialog.
    Tell,
    /// `req revise --stdin` through the Revise dialog.
    Revise,
    /// `records --studio` — the actor-records read (view).
    Records,
    /// `card apply` direct.
    CardApply,
    /// `card show` — the details view.
    CardShow,
    /// `card cancel` through the reason dialog (reason optional).
    CardCancel,
}

/// One menu line.
#[derive(Debug, Clone)]
pub(crate) struct StudioItem {
    /// The row text — key hints live inside it (`Allow prompt (y)`).
    pub label: String,
    /// The single key that fires it while the menu is open.
    pub key: Option<char>,
    /// A dim reason (`no terminal`, `current`, `unreachable`) —
    /// `None` rows are live; `Some` rows draw dim and do nothing.
    pub dim: Option<String>,
    /// `ready_to_finish`'s finish item draws teal.
    pub teal: bool,
    /// What a press runs.
    pub act: StudioAct,
}

/// The menu's target — the row plus the facts its items need.
#[derive(Debug, Clone)]
pub(crate) struct StudioMenu {
    pub kind: MenuKind,
    /// The studio — a Stage row's menu belongs to no studio.
    pub studio: Option<StudioKey>,
    /// The role the row names (Role/Prompt/DirPrompt/Closed rows).
    pub role: Option<MenuRole>,
    /// The task the row names (Task rows, and an actor's own task).
    pub task: Option<MenuTask>,
    /// The card id (Card rows).
    pub card: Option<String>,
    /// The machine's stage is down — studio items swap start/stop.
    pub stage_down: bool,
    /// The machine is unreachable — every action dims.
    pub offline: bool,
    /// `ready_to_finish` — the Finish item draws teal.
    pub ready: bool,
    /// The requirement version — the Revise title counts `v<n> →
    /// v<n+1>`.
    pub req_version: u32,
    /// A Stage row's machine — studio rows derive theirs from
    /// `studio.machine` instead.
    pub machine: Option<String>,
}

impl StudioMenu {
    /// The machine stage acts run on — a Stage row's own, else the
    /// studio's.
    pub(crate) fn machine(&self) -> Option<&str> {
        self.machine
            .as_deref()
            .or_else(|| self.studio.as_ref().and_then(|k| k.machine.as_deref()))
    }

    /// A studio row's menu (header, block, strip, Call Sheet frame).
    pub(crate) fn studio(
        key: &StudioKey,
        summary: &StudioSummary,
        stage_down: bool,
        offline: bool,
    ) -> StudioMenu {
        StudioMenu {
            kind: MenuKind::Studio,
            studio: Some(key.clone()),
            role: None,
            task: None,
            card: None,
            stage_down,
            offline,
            ready: summary.ready_to_finish,
            req_version: summary.requirement_version,
            machine: None,
        }
    }

    /// A person row's menu — the kind follows the row's own state:
    /// a live role at a user prompt is `Prompt`, at a director
    /// prompt `DirPrompt`, otherwise `Role`; closed/lost is `Closed`.
    pub(crate) fn role(key: &StudioKey, role: &RoleSummary, offline: bool) -> StudioMenu {
        let role = MenuRole::from(role);
        let kind = match role.state.as_str() {
            "live" if role.attention_seq.is_some() && role.attention_user => MenuKind::Prompt,
            "live" if role.attention_seq.is_some() => MenuKind::DirPrompt,
            "live" => MenuKind::Role,
            _ => MenuKind::Closed,
        };
        StudioMenu {
            kind,
            studio: Some(key.clone()),
            role: Some(role),
            task: None,
            card: None,
            stage_down: false,
            offline,
            ready: false,
            req_version: 0,
            machine: None,
        }
    }

    /// A task row's menu.
    pub(crate) fn task(key: &StudioKey, task: &TaskSummary, offline: bool) -> StudioMenu {
        StudioMenu {
            kind: MenuKind::Task,
            studio: Some(key.clone()),
            role: None,
            task: Some(MenuTask::from(task)),
            card: None,
            stage_down: false,
            offline,
            ready: false,
            req_version: 0,
            machine: None,
        }
    }

    /// A Call Sheet card row's menu.
    pub(crate) fn card(id: &str) -> StudioMenu {
        StudioMenu {
            kind: MenuKind::Card,
            studio: None,
            role: None,
            task: None,
            card: Some(id.to_owned()),
            stage_down: false,
            offline: false,
            ready: false,
            req_version: 0,
            machine: None,
        }
    }

    /// A Call Sheet `STAGE DOWN` row's menu — `Start stage` alone.
    pub(crate) fn stage(offline: bool) -> StudioMenu {
        StudioMenu {
            kind: MenuKind::Stage,
            studio: None,
            role: None,
            task: None,
            card: None,
            stage_down: true,
            offline,
            ready: false,
            req_version: 0,
            machine: None,
        }
    }
}

mod items;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
