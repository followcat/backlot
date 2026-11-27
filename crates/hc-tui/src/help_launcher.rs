//! The `Ctrl+B ?` launcher's row model (S5 §1): two row kinds —
//! runnable actions that carry a `StudioMenu`/`StudioAct` pair and
//! fire through `run_studio_act`, and pure reference rows whose
//! Enter only focuses the surface they describe. The compact target
//! picker shares the model: each of its rows names a concrete
//! target and the press that follows.

use crate::studio_menu::{StudioAct, StudioMenu};

/// What an Enter on the row does — the launcher closes either way;
/// the action's surface (dialog, menu, terminal) owns the keys next.
#[derive(Debug, Clone)]
pub(crate) enum Launch {
    /// A studio act on a built target menu — `run_studio_act`
    /// resolves it to `run_action`, a native dialog or a navigation.
    Act {
        menu: Box<StudioMenu>,
        act: StudioAct,
    },
    /// Focus a named surface — the Call Sheet, its `tell` line, the
    /// dock or the focused pane's context menu.
    Surface(Surface),
    /// Open the compact target picker — its rows name concrete
    /// targets; Enter on one acts like Enter on that target's row.
    Pick(Pick),
    /// A studio key's own navigation — Enter runs `studio_nav`.
    Nav(crate::panel::StudioNav),
    /// Open a target's own context menu — the same list a right-click
    /// opens (`Machine menu · <machine>`).
    Menu(Box<StudioMenu>),
    /// Herdr's own keybind rows and the legend — Enter just closes.
    Close,
}

/// The surfaces a reference row focuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Surface {
    /// The Call Sheet expanded — keys go to the bar.
    CallSheet,
    /// The Call Sheet expanded with `› tell hollywood` editing.
    CallSheetTell,
    /// The studio dock — the same press the toggle chord gives.
    Dock,
    /// Focus the lot on its last selectable row (`Alt+L`).
    FocusLot,
    /// The focused pane's studio menu (`Ctrl+B m`).
    PaneMenu,
}

/// Which target list a picker row gathers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pick {
    /// Every machine — `new_studio_open` on the choice.
    Machine,
    /// Every studio — the row's own `StudioAct` (Revise).
    Studio(StudioAct),
    /// Every visible task — the Task & Delivery Sheet on the choice.
    Task,
    /// Every (task, applicable verb) pair — the reason dialog.
    TaskVerb,
    /// Every closed-or-lost person — `reopen --person`.
    PersonReopen,
}

/// One launcher row — `glyph` leads runnable waiting rows (`×`, `◆`,
/// `✓`, `◌`); `hint` is the right-aligned key column (`alt+y -> y`,
/// `run ↵`); `launch` is what Enter does.
#[derive(Debug, Clone)]
pub(crate) struct HelpRow {
    pub label: String,
    pub hint: String,
    /// The leading glyph — `""` for plain keybind-style rows.
    pub glyph: &'static str,
    pub launch: Launch,
}

/// A group header plus its rows — the launcher's answer to
/// `KeybindHelpGroup`.
#[derive(Debug, Clone)]
pub(crate) struct HelpGroup {
    pub title: String,
    pub rows: Vec<HelpRow>,
}

/// What a picker row's Enter does with its `StudioMenu` target.
#[derive(Debug, Clone)]
pub(crate) enum PickerAct {
    /// `run_studio_act` — actions, dialogs, focuses all flow through
    /// the S3 activation arm.
    Act(StudioAct),
    /// The Task & Delivery Sheet on the row's studio/task.
    Sheet,
    /// The New Studio dialog seeded with the row's machine.
    NewStudio,
}

/// One picker row — a concrete target's label plus its target menu.
#[derive(Debug, Clone)]
pub(crate) struct PickerRow {
    pub label: String,
    pub dim: Option<String>,
    pub menu: StudioMenu,
    pub act: PickerAct,
}

/// The compact picker — a floating list inside the help overlay.
#[derive(Debug, Clone)]
pub(crate) struct HelpPicker {
    /// The launcher row it came from — drawn as the picker's title.
    pub title: String,
    pub rows: Vec<PickerRow>,
    pub selected: usize,
    pub scroll: usize,
    /// True when opened directly from the Cast without the launcher behind it.
    pub standalone: bool,
}
