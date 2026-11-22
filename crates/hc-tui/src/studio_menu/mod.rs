//! The studio context-menu model (S3 §1): the row kinds the dock,
//! the Call Sheet and `Ctrl+B m` open a menu for, each target's item
//! list in the spec's order and the action one press runs. Pure
//! data — the app fills the target facts from the same summary the
//! row drew from, so a menu always matches its row.

use backlot_model::state::TaskStatus;
use backlot_model::summary::{RoleSummary, StudioSummary, TaskSummary};

use crate::studio::StudioKey;

/// The producer's `MenuRole` for a finished studio's menu — the
/// document's own row when it lists one, so a producer reopened after
/// the finish reads live and the menu can offer `Focus producer`
/// instead of a reopen it does not need (A12). The fixed seat still
/// synthesises its row when the document lists none (a finished
/// studio lists people as closed). `resumable` stays on either way:
/// a fixed seat reopens without a saved session, the same rule a
/// closed-person row uses, and a live seat never consults it.
fn producer_role(summary: &StudioSummary) -> Option<MenuRole> {
    if let Some(role) = summary.roles.iter().find(|r| r.role == "producer") {
        let mut role = MenuRole::from(role);
        role.resumable = true;
        return Some(role);
    }
    Some(MenuRole {
        role: "producer".into(),
        name: "producer".into(),
        state: "closed".into(),
        resumable: true,
        task: None,
        terminal: None,
        attention_seq: None,
        attention_user: false,
    })
}

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

impl MenuRole {
    /// `studio::live_terminal`'s rule on the menu's own copy: live
    /// with a terminal to land on.
    fn live_terminal(&self) -> Option<&str> {
        match self.state.as_str() {
            "live" => self.terminal.as_deref(),
            _ => None,
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
    /// The lot pill's quick menu (S5 §2.3) — lot-wide surfaces,
    /// not one studio's.
    Lot,
    /// An endpoint sidebar machine row (U6c) — install/upgrade,
    /// the stage and `New studio here`, plus the endpoint ops.
    Machine,
    /// A Call Sheet `Headline` or `Talk(seq)` row — read entry or
    /// focus the raw `bl-hollywood` terminal.
    Hollywood(Option<u64>),
    /// A Studio Dock `Talk(studio, seq)` row — read message/report
    /// body or act on its studio.
    StudioTalk(u64),
}

/// What the machine menu knows about `backlot` there — the
/// installed version, a confirmed absence, or "not yet probed".
#[derive(Debug, Clone)]
pub(crate) enum MenuBacklot {
    /// `backlot --version` (or the machine's document) answered.
    Installed(String),
    /// The locate probe says there is no `backlot` on the machine.
    Missing,
    /// Probing or not yet asked — the install item dims.
    Unknown,
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
    /// The lot menu's `Open Call Sheet` — the expanded band.
    CallSheet,
    /// The lot menu's `Toggle Studio Dock`.
    ToggleDock,
    /// The lot menu's `New studio…` — S4's dialog.
    NewStudio,
    /// The lot menu's `Hollywood settings…` — Settings' Studio
    /// section, where the `[hollywood]` rows live.
    HollywoodSettings,
    /// `backlot stage stop && stage start` on the machine — the
    /// upgrade follow-up and the menu's restart item (U6c).
    StageRestart,
    /// Install or upgrade `backlot` on the machine through the
    /// official installer — a popup on remote, the update flow
    /// locally; managed binaries refuse first (W2).
    MachineInstall,
    /// `machine.connect` on an unreachable endpoint.
    MachineConnect,
    /// `machine.rename` — the same dialog the herdr menu opens.
    MachineRename,
    /// `machine.rm` — the same confirm the herdr menu opens.
    MachineRemove,
    /// `machine.add` — the Local row's own op.
    MachineAdd,
    /// `hollywood show <seq>` — open the report or message in `CardView`.
    HollywoodShow,
    /// Focus the raw `bl-hollywood` terminal.
    FocusHollywood,
    /// `report show --file <rel> --studio <s>` — open a studio
    /// exchange entry in `CardView`.
    ReportShow,
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
    /// `studio.finished` landed: the studio ended and every role was
    /// released. `ready_to_finish` reads `state: "finished"` too, so
    /// this is `finished && !ready_to_finish` — the studio menu swaps
    /// its dead Finish and Focus rows for the producer's reopen.
    pub finished: bool,
    /// `ready_to_finish` — the Finish item draws teal.
    pub ready: bool,
    /// The requirement version — the Revise title counts `v<n> →
    /// v<n+1>`.
    pub req_version: u32,
    /// Current requirement's studio-relative path (`req/v<n>.md`), if
    /// published in the summary.
    pub req_file: Option<String>,
    /// A Stage row's machine — studio rows derive theirs from
    /// `studio.machine` instead.
    pub machine: Option<String>,
    /// A Machine row's `backlot` fact (U6c) — `None` on every other
    /// kind.
    pub backlot: Option<MenuBacklot>,
    /// The newest known release (`0.2.4`, no `v`) — the
    /// `Upgrade backlot to v<latest>` label reads it.
    pub latest: Option<String>,
    /// Studio-relative file path on a `StudioTalk` row (`messages/…`
    /// or `reports/…`).
    pub talk_file: Option<String>,
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
    /// A finished studio carries its producer's `MenuRole`, so the
    /// reopen row runs the same people-reopen path a closed-person
    /// row does — the sidebar and the band have no person rows of
    /// their own, so this menu is that person's only entry.
    pub(crate) fn studio(
        key: &StudioKey,
        summary: &StudioSummary,
        stage_down: bool,
        offline: bool,
    ) -> StudioMenu {
        // `ready_to_finish` also reads `state: "finished"`; only a
        // studio past it has really ended.
        let finished = summary.state == "finished" && !summary.ready_to_finish;
        StudioMenu {
            kind: MenuKind::Studio,
            studio: Some(key.clone()),
            role: if finished {
                producer_role(summary)
            } else {
                None
            },
            task: None,
            card: None,
            stage_down,
            offline,
            finished,
            ready: summary.ready_to_finish,
            req_version: summary.requirement_version,
            req_file: summary.req_file.clone(),
            machine: None,
            backlot: None,
            latest: None,
            talk_file: None,
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
            finished: false,
            ready: false,
            req_version: 0,
            req_file: None,
            machine: None,
            backlot: None,
            latest: None,
            talk_file: None,
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
            finished: false,
            ready: false,
            req_version: 0,
            req_file: None,
            machine: None,
            backlot: None,
            latest: None,
            talk_file: None,
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
            finished: false,
            ready: false,
            req_version: 0,
            req_file: None,
            machine: None,
            backlot: None,
            latest: None,
            talk_file: None,
        }
    }

    /// The lot pill's menu (S5 §2.3): the whole lot's quick actions —
    /// no studio, role, task or card facts.
    pub(crate) fn lot() -> StudioMenu {
        StudioMenu {
            kind: MenuKind::Lot,
            studio: None,
            role: None,
            task: None,
            card: None,
            stage_down: false,
            offline: false,
            finished: false,
            ready: false,
            req_version: 0,
            req_file: None,
            machine: None,
            backlot: None,
            latest: None,
            talk_file: None,
        }
    }

    /// An endpoint sidebar machine row's menu (U6c) — `None` is the
    /// Local endpoint. `backlot`, `stage_down` and `latest` decide
    /// which of install/upgrade/stage items the row offers.
    pub(crate) fn machine_row(
        machine: Option<String>,
        backlot: MenuBacklot,
        stage_down: bool,
        offline: bool,
        latest: Option<String>,
    ) -> StudioMenu {
        StudioMenu {
            kind: MenuKind::Machine,
            studio: None,
            role: None,
            task: None,
            card: None,
            stage_down,
            offline,
            finished: false,
            ready: false,
            req_version: 0,
            req_file: None,
            machine,
            backlot: Some(backlot),
            latest,
            talk_file: None,
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
            finished: false,
            ready: false,
            req_version: 0,
            req_file: None,
            machine: None,
            backlot: None,
            latest: None,
            talk_file: None,
        }
    }

    /// A Call Sheet `Headline` or `Talk(seq)` row's menu — reads the
    /// entry or focuses `bl-hollywood`.
    pub(crate) fn hollywood(seq: Option<u64>) -> StudioMenu {
        StudioMenu {
            kind: MenuKind::Hollywood(seq),
            ..StudioMenu::lot()
        }
    }

    /// A Studio Dock `Talk(studio, seq)` row's menu — reads its
    /// `messages/…` or `reports/…` file when present, plus quick
    /// studio actions.
    pub(crate) fn studio_talk(
        key: &StudioKey,
        seq: u64,
        talk_file: Option<String>,
        offline: bool,
    ) -> StudioMenu {
        StudioMenu {
            kind: MenuKind::StudioTalk(seq),
            studio: Some(key.clone()),
            offline,
            talk_file,
            ..StudioMenu::lot()
        }
    }
}

mod items;

#[cfg(test)]
#[path = "finished_tests.rs"]
mod finished_tests;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
