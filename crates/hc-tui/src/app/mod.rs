//! The interactive client: owns inventory, shared layout, pane connections
//! and the draw scheduler. One owner for all
//! UI/VT state; the loop awaits input, protocol notifications and
//! bounded actor results, and redraws only when dirty under the ~60 Hz
//! cap — there is no idle poll timer.
//!
//! M3a implements structure, layout/tab operations and bindings plus lock.
//! Custom commands, notifications and remaining actions belong to M4.

mod agent_ops;
mod agent_presentation;
mod closing;
mod context_menu;
mod copy_keys;
pub(crate) mod copy_mode;
mod copy_move;
mod copy_render;
mod creation;
mod dialogs;
mod draw;
mod endpoint_navigation;
mod float;
mod float_draw;
mod float_reconcile;
mod help;
mod host;
mod image_paste;
mod init;
pub(crate) mod input;
mod input_dispatch;
mod key_leases;
mod key_routing;
mod layout_actions;
pub(crate) mod layout_mouse;
mod mouse;
mod multipane;
mod navigation;
mod navigator;
mod notices;
pub(crate) mod notifications;
mod notifications_focus;
mod pane;
mod pane_geometry;
mod pane_mouse;
mod pane_selection;
mod panel;
pub(crate) mod popup;
mod popup_selection;
mod presentation;
mod reload;
mod routing;
mod runtime;
mod sched;
mod seen;
mod selection_copy;
mod settings;
mod shared;
mod synthesize;
mod tab_status;
mod window_title;
mod word_selection;

pub use input::UiEvent;
pub use pane::{OpenAttach, PaneEv, Role, Spec};

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use hc_proto::watch::{View, Watcher};
use ratatui::Terminal;
use ratatui::backend::Backend;
use tokio::sync::mpsc;

use crate::chrome::ClientShellConfig;
use crate::chrome::navigate::WorkspaceNavigationTarget;
use crate::chrome::render::ShellHitMap;
use crate::chrome::snapshot::ClientEndpointId;

/// A boxed future for the injected attach opener.
pub type BoxFut<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// What the app is doing with input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Keys go to the focused terminal.
    Normal,
    /// Navigate mode — the workspace picker the reference enters on
    /// `WorkspacePicker`; the pane previews the highlight via `observe`.
    Navigate,
    /// Keyboard copy mode — the reference's `ClientShellMode::Copy`.
    /// Keys drive the copy cursor, selection and search; the pane's
    /// program sees none of them.
    Copy,
    Resize,
}

/// Instrumentation counters a test can hold across `run`.
#[derive(Debug, Default)]
pub struct Probe {
    /// Redraws performed.
    pub draws: std::sync::atomic::AtomicU64,
    /// Attach attempts made.
    pub attaches: std::sync::atomic::AtomicU64,
    /// Times the select loop woke for any reason.
    pub wakeups: std::sync::atomic::AtomicU64,
    /// Times a deadline sleep was armed.
    pub timers: std::sync::atomic::AtomicU64,
}

/// Where host-mode control writes go (keyboard push/pop, mouse modes,
/// notifications): the outer terminal in production, a test recorder
/// otherwise. A plain boxed `FnMut` keeps the app free of
/// crossterm/global-IO calls.
pub type HostControlSink = Box<dyn FnMut(&[u8]) -> std::io::Result<()>>;

/// The mutable chrome state `ShellRenderState` borrows each frame —
/// scroll offsets, collapse sets, reveal flags and the sidebar split.
#[derive(Default)]
pub(crate) struct ShellState {
    pub collapsed_endpoints: std::collections::HashSet<ClientEndpointId>,
    pub collapsed_groups: std::collections::HashSet<String>,
    pub remote_collapsed_groups:
        std::collections::HashMap<ClientEndpointId, std::collections::HashSet<String>>,
    pub workspace_scroll: usize,
    pub agent_scroll: usize,
    pub tab_scroll: usize,
    pub reveal_focused_workspace: bool,
    pub reveal_focused_tab: bool,
    pub sidebar_collapsed: bool,
    /// Expanded sidebar's spaces:agents split, `0.0..=1.0`.
    pub sidebar_section_split: f32,
    pub reveal_navigation_workspace: bool,
    /// Mobile switcher viewport scroll (Navigate mode under
    /// `mobile_width_threshold` — the switcher is the mobile picker).
    pub mobile_switcher_scroll: usize,
    /// Scrolls the switcher to the pending workspace highlight on move.
    pub reveal_mobile_workspace: bool,
    pub machine_diagnostics: crate::chrome::render::MachineDiagnostics,
}

/// A forwarded press and the wire it went out on. `route` binds the
/// attach generation — a reincarnated terminal is a different route,
/// so a release can never land on a replacement wire for the same
/// address. Repeats route only while the lease's wire is still the
/// active attach — releases for a pane the client no longer views
/// still pass, but its repeats do not.
type PaneLease = key_leases::ForwardedInputLease<u64>;
type KeyLeases = key_leases::InputLeaseTable<u8, key_routing::Context, u64>;

/// The whole application.
pub struct App {
    notice: Option<notices::Notice>,
    /// The one-shot `hc server start` report — `Some` until it lands.
    core_start: Option<crate::core_start::Report>,
    update_check: Option<crate::update_check::Report>,
    background_failure: Option<String>,
    next_tab_status: Option<Instant>,
    last_window_title: Option<String>,
    take_control_hit: ratatui::layout::Rect,
    watcher: Watcher,
    pane: pane::Pane,
    pane_rx: mpsc::Receiver<PaneEv>,
    pane_tx: mpsc::Sender<PaneEv>,
    open_attach: OpenAttach,
    observers: std::collections::BTreeMap<String, pane::Pane>,
    size_leases: std::collections::BTreeMap<String, pane::Pane>,
    observer_mru: std::collections::VecDeque<String>,
    layout_seen: Option<(String, u64)>,
    operations: Option<crate::operations::Store>,
    pending_actions: usize,
    layout_confirmed: Option<(String, hc_proto::watch::Document)>,
    prefix: input::Prefix,
    mode: Mode,
    keyboard_locked: bool,
    previous_pane: Option<String>,
    last_focus: Option<String>,
    dialog: Option<dialogs::Dialog>,
    dialog_hits: crate::chrome::dialogs::DialogRender,
    /// Open right-click context menu — the reference's
    /// `ClientShellOverlay::ContextMenu`.
    menu: Option<crate::chrome::menu::ContextMenu>,
    /// Panes whose bare right-click goes to the program instead of the
    /// context menu — the per-pane `right_click_passthrough` toggle.
    right_click_panes: std::collections::HashSet<String>,
    help: bool,
    mobile_switcher_suspended: bool,
    help_state: crate::chrome::help::HelpState,
    help_hits: crate::chrome::help::HelpRender,
    help_scroll_grab: Option<u16>,
    /// The "Go to" overlay — the reference's
    /// `ClientShellOverlay::Navigator`.
    navigator: Option<crate::chrome::navigator::NavigatorState>,
    /// Last frame's navigator hit rectangles.
    navigator_hits: crate::chrome::navigator_render::NavigatorRender,
    /// The Settings overlay — the reference's
    /// `ClientShellOverlay::Settings`.
    settings: Option<crate::chrome::settings::SettingsOverlay>,
    /// Last frame's settings hit rectangles (popup, tabs, choices,
    /// buttons).
    settings_hits: crate::chrome::settings_render::SettingsRender,
    /// The floating popup terminal — the reference's `popup_pane`.
    popup: Option<popup::PopupState>,
    /// Geometry for a `terminal.run` still in flight for a popup.
    popup_pending: Option<popup::PopupPending>,
    /// Pressed keys intercepted on behalf of a persistent float — their
    /// releases must not reach the program either.
    popup_key_suppress: std::collections::HashSet<hc_vt::Key>,
    /// Hidden persistent floats, keyed by owner tab + binding — each
    /// entry's program runs on behind a `size_leases` attach.
    floats: std::collections::BTreeMap<float::FloatKey, float::FloatEntry>,
    /// The `pane` command's zoomed overlay — the reference's
    /// `overlay_panes` entry for a spawned command pane.
    command_overlay: Option<popup::CommandOverlay>,
    /// The worker-peek overlay — herdr-app's `terminal-peek`.
    peek: Option<popup::PeekState>,
    /// The studio panel's client-local state — `Closed` by default, so
    /// a stock frame is cell-identical to herdr (no width, no keys).
    panel: crate::panel::Panel,
    /// The `[panel]` config and the resolved toggle chord tail.
    panel_config: crate::panel::PanelConfig,
    panel_key_combo: Option<crate::config::KeyCombo>,
    /// Last frame's panel hit rectangles (`PanelRender`).
    panel_hits: crate::panel::PanelRender,
    /// One-shot spin deadline — armed only while a working mark is
    /// drawn, so an idle or closed panel wakes nothing (sched.rs).
    panel_anim_deadline: Option<Instant>,
    /// Injectable `backlot …` runner — tests record argv, production
    /// spawns `current_exe()` off the event loop.
    panel_runner: crate::panel::PanelRunner,
    panel_action_tx: mpsc::Sender<(String, Result<(), String>)>,
    panel_rx: mpsc::Receiver<(String, Result<(), String>)>,
    /// Endpoint flags (`--session`/`--remote`) for spawned actions.
    panel_endpoint_args: Vec<String>,
    sched: sched::Scheduler,
    /// The focused terminal address (Normal-mode spec).
    focused: Option<String>,
    /// The terminal to restore on navigate cancel.
    before_navigate: Option<String>,
    /// The resolved prefix + keybind config driving the prefix machine.
    live: crate::config::LiveKeybindConfig,
    /// The immutable reference chrome config: palette, layout bounds,
    /// sidebar composition, pane chrome and keybinds — resolved once.
    shell: ClientShellConfig,
    /// The projected snapshot the chrome renders; recomputed on
    /// `on_view` once the focus settles so `focused_*` ids resolve.
    projection: crate::project::Projection,
    /// The pending workspace highlight Navigate moves — highlight-only
    /// until Enter; the pane keeps its active attach meanwhile.
    navigate_target: Option<WorkspaceNavigationTarget>,
    /// Sidebar/tab/agent scroll + collapse state the chrome mutates
    /// through `ShellRenderState` borrows each frame.
    pub(crate) shell_state: ShellState,
    /// Live sidebar width — initialized from config, dragged by the
    /// divider; the reference's `sidebar_width` runtime field.
    sidebar_width: u16,
    sidebar_width_manual: bool,
    agent_panel_sort_manual: bool,
    /// Active chrome drag grab (`ChromeDrag`).
    chrome_drag: Option<mouse::ChromeDrag>,
    pane_mouse_gesture: Option<pane_mouse::Gesture>,
    /// Pane text selection — the reference's `state.selection`. Anchors
    /// live on absolute screen rows, so scrolling and new output don't
    /// move the selected text.
    selection: Option<crate::selection::Selection>,
    /// Keyboard copy mode — the reference's `copy_mode`. The mode is
    /// armed per pane: it survives losing focus and re-arms `Mode::Copy`
    /// when its pane is focused again.
    pub(crate) copy_mode: Option<copy_mode::CopyModeState>,
    /// Selection inside the floating popup terminal — the reference's
    /// `popup_selection`, resolved against the popup's visible frame.
    popup_selection: Option<crate::selection::Selection>,
    /// Mouse button captured by a reporting popup terminal — the
    /// reference's shared `pane_mouse_gesture` for `hit.popup` targets.
    popup_gesture: Option<popup::PopupGesture>,
    /// Double-click word-drag gesture — the reference's
    /// `word_selection_gesture`, resolved against the pane's local VT.
    word_selection: Option<word_selection::WordSelection>,
    /// Edge autoscroll while a drag sits past the inner rect's top or
    /// bottom — the reference's `ClientSelectionAutoscroll`.
    selection_autoscroll: Option<pane_selection::SelectionAutoscroll>,
    /// Next 30 ms autoscroll tick.
    selection_autoscroll_deadline: Option<Instant>,
    /// A bare double-click's word highlight clears after 500 ms.
    selection_highlight_clear_deadline: Option<Instant>,
    /// The `copied to clipboard` card — the reference's `copy_feedback`
    /// with its two-second deadline.
    copy_feedback: Option<notices::CopyFeedback>,
    copy_feedback_deadline: Option<Instant>,
    /// Last pane click — the reference's `ClientPaneClick` double-click
    /// bookkeeping for word selection.
    last_pane_click: Option<pane_selection::PaneClick>,
    /// Pressed-but-unreleased workspace/tab rows.
    workspace_press: Option<mouse::WorkspacePress>,
    tab_press: Option<mouse::TabPress>,
    tab_drag_index: Option<usize>,
    tab_dragged: bool,
    interaction_core: Option<String>,
    /// For the divider's double-click reset.
    last_divider_click: Option<Instant>,
    /// Image-paste bridge context (`endpoint_accepts_local_images`):
    /// the endpoint is `--remote`, or this client itself runs over SSH.
    image_paste_remote: bool,
    image_paste_over_ssh: bool,
    /// Kitty releases of presses the image bridge consumed: the release
    /// arm consults this before the popup's unconditional release leg.
    image_paste_suppress: std::collections::HashSet<hc_vt::Key>,
    /// The clipboard-image read — a boxed field so tests never touch a
    /// real clipboard and can close over stub-tool environments.
    image_paste_reader: Box<dyn Fn() -> crate::clipboard::ClipboardRead>,
    /// Last frame's clickable rects.
    pub(crate) hit_map: ShellHitMap,
    /// The host was told to report every key as an escape code —
    /// the reference's `keyboard_report_all_active`, upgraded while
    /// the pane negotiates report-all or a binding mode is active so
    /// IME/text commits stay distinguishable from typed keys.
    host_reports_all_keys: bool,
    /// The host reports key releases (the startup keyboard push
    /// succeeded) — `host_reports_key_releases`.
    host_reports_key_releases: bool,
    /// The host's cooked VERASE is `^H` — a raw 0x08 is its Backspace
    /// key (`host_erase_is_ctrl_h`, #3244).
    host_erase_is_ctrl_h: bool,
    /// Capability-probe state — the reference's
    /// `HostKeyboardProbeResponses`: flags are only accepted until
    /// primary DA proves the query pipeline works.
    probe_keyboard_flags: Option<u16>,
    probe_device_attributes: bool,
    /// DECRPM `?1016$p` answer — the reference's `sgr_pixel_mouse`:
    /// `Some(true)` only when the host reports the mode recognized and
    /// reset; `None` when it never answered (keep pixel mouse off).
    /// `?1016h` itself stays off until inner-negotiated cell geometry
    /// exists to translate pixel coordinates (the FACTS surface).
    host_sgr_pixel_mouse: Option<bool>,
    host_pixel_mouse_active: bool,
    host_mouse_capture_active: bool,
    host_mouse_pixels: Option<crate::mouse_geometry::HostPixels>,
    host_appearance: Option<crate::chrome::palette::HostAppearance>,
    host_appearance_explicit: bool,
    /// Host-mode control writes (keyboard pop/push). Injected by cli —
    /// headless tests get the no-op default so they never touch the
    /// real terminal's modes.
    host_control: HostControlSink,
    /// Pane-bound forwarded presses — the reference's `InputLeaseTable`
    /// scoped to one local source and one attach. A press routed to
    /// the terminal keeps a lease: repeats keep routing while it
    /// lives, releases close it, and a detach/target move synthesizes
    /// the release the program would otherwise never see.
    /// Forwarded pane presses, bound to the terminal they went out on:
    /// a held key's release returns to that wire even after the pane
    /// switches away and parks the attach for drain.
    pane_leases: KeyLeases,
    next_dispatch_id: u64,
    /// Last watcher view.
    view: View,
    /// View version cursor for `changed_since`.
    seen: u64,
    /// The area the pane was last allocated (for spec sizing).
    area: (u16, u16),
    /// The cursor the last frame asked for — resolved into the
    /// host-cursor state the backend emits inside the frame.
    pub(crate) want_cursor: draw::CursorWish,
    last_visible_cursor: Option<(u16, u16)>,
    /// Set by Detach or when the input source ends.
    quitting: bool,
    /// Probed host-terminal facts (colors, cell pitch) — late replies
    /// update it and re-seed panes.
    host_facts: crate::host::HostFacts,
    /// Panes the focused workspace's active tab marked seen — the
    /// reference's `mark_active_tab_seen`/`apply_pane_state_change`:
    /// an idle agent reports `Done` only while unseen; focusing its
    /// workspace marks every active-tab pane seen at once.
    presentation: presentation::Presentation,
    /// Synthesized notifications — the reference client pipeline
    /// (`pending`/`visible`/`queued`) fed from pane status words, since
    /// hc's core emits no notification events.
    notifications: notifications::Notifications,
    /// The first reconcile seeds word baselines silently (herdr-app's
    /// `listed_once`): only post-baseline transitions alert.
    notifications_seeded: bool,
    /// The rendered toast's rect — a click focuses its target.
    notification_hit: ratatui::layout::Rect,
    /// OSC 9/99 backend + tmux wrap, detected once from the env.
    notify_backend: Option<crate::terminal_notify::TerminalNotificationBackend>,
    in_tmux: bool,
    /// Per-pane turn markers for `herdr.seen` — the reference's
    /// `looked`. Only currently-visible panes keep an entry.
    seen_looked: std::collections::HashMap<String, u64>,
    /// Seen writes in flight — the same (pane, turn) never double-sends.
    seen_inflight: std::collections::HashSet<(String, u64)>,
    /// The dedicated label-write worker: fire-and-forget `terminal.label`
    /// RPCs with failures reported back for retry, kept off the layout
    /// store so a background write can't surface an action notice.
    seen_tx: mpsc::Sender<seen::SeenWrite>,
    seen_res_tx: mpsc::Sender<seen::SeenResult>,
    seen_rx: mpsc::Receiver<seen::SeenResult>,
    seen_req_rx: Option<mpsc::Receiver<seen::SeenWrite>>,
    outer_focused: Option<bool>,
    probe: Arc<Probe>,
}

impl App {
    /// An input drain failure is reported after terminal restoration on exit.
    pub fn failure(&self) -> Option<&str> {
        self.pane
            .close_failure()
            .or(self.background_failure.as_deref())
    }

    /// Recompute the projection for the current focus — finding 12:
    /// `focused_*` snapshot ids must track the focus without waiting
    /// for the next watcher publish.
    fn refresh_projection(&mut self) {
        if self.focused != self.last_focus {
            self.previous_pane = self.last_focus.clone();
            self.last_focus = self.focused.clone();
        }
        let old_tab = self.projection.snapshot.focused_tab_id.clone();
        let old_order = self
            .projection
            .snapshot
            .tabs
            .iter()
            .map(|t| t.tab_id.clone())
            .collect::<Vec<_>>();
        self.projection = crate::project::project(&self.view, self.focused.as_deref());
        if old_tab != self.projection.snapshot.focused_tab_id
            || old_order
                != self
                    .projection
                    .snapshot
                    .tabs
                    .iter()
                    .map(|t| t.tab_id.clone())
                    .collect::<Vec<_>>()
        {
            self.shell_state.reveal_focused_tab = true;
        }

        self.presentation.project(&self.view, &mut self.projection);
        self.update_tab_status(Instant::now());
        self.update_window_title();
        // The reference's `apply_active_snapshot` copy arms: pane loss
        // unarms the mode, focus re-arms/disarms it.
        self.sync_copy_focus();
        // Focus settled: park the old owner's float, restore the new
        // one's — before `reconcile_popups` sees the publish.
        self.sync_float_focus();
    }

    /// Request a surface draw; Source acknowledges only displayed output.
    fn mark_active_seen(&mut self) {
        // A focus request alone cannot acknowledge unseen output. Source only
        // acknowledges a matching surface after it has actually been drawn.
        self.sched.mark_dirty(Instant::now());
    }

    /// A watcher publish: refresh the projection + pane target policy.
    fn on_view(&mut self) {
        self.view = self.watcher.view();
        self.seen = self.view.version;
        self.retain_confirmed_layout();
        self.reconcile_published_view();
    }

    /// Everything `on_view` does after the view lands — split out so
    /// tests can publish a synthetic `self.view` through the same path.
    fn reconcile_published_view(&mut self) {
        self.fence_chrome_core();
        self.follow_shared_focus();
        // The dead pane's context survives the reprojection for the
        // fallback below — a transient inventory view can precede the
        // layout write that names the real successor.
        let previous_tab = self.projection.snapshot.focused_tab_id.clone();
        let previous_workspace = self.projection.snapshot.focused_workspace_id.clone();
        self.refresh_projection();
        // If the focused terminal fell out of the inventory entirely,
        // prefer a sibling in the same tab, then the same workspace —
        // never a global first pane that could ack another space's work.
        if self.mode == Mode::Normal
            && self.focused.as_ref().is_none_or(|f| {
                self.projection
                    .snapshot
                    .panes
                    .iter()
                    .all(|p| &p.pane_id != f)
            })
        {
            let panes = &self.projection.snapshot.panes;
            let fallback = panes
                .iter()
                .find(|p| previous_tab.as_ref() == Some(&p.tab_id))
                .or_else(|| {
                    panes
                        .iter()
                        .find(|p| previous_workspace.as_ref() == Some(&p.workspace_id))
                })
                .or_else(|| panes.first())
                .map(|p| p.pane_id.clone());
            if fallback != self.focused {
                self.focused = fallback;
                self.refresh_projection();
            }
        }
        // Focusing marks the active tab's panes seen — an idle agent
        // reports `Done` only while its workspace stayed background.
        self.mark_active_seen();
        self.refresh_projection();
        self.sync_target();
        self.pane.on_view(&self.view);
        if let Some(popup) = self.popup.as_mut() {
            popup.pane.on_view(&self.view);
        }
        if let Some(peek) = self.peek.as_mut() {
            peek.pane.on_view(&self.view);
        }
        self.sync_observers();
        self.reconcile_popups();
        self.synthesize_alerts(Instant::now());
        self.share_seen();
    }
}

#[cfg(test)]
#[path = "mod_tests/mod.rs"]
mod tests;

mod hostname;
