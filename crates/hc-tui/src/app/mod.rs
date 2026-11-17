//! The interactive client: owns inventory, shared layout, pane connections
//! and the draw scheduler. One owner for all UI/VT state; the loop awaits
//! input, protocol notifications and bounded actor results, and redraws
//! only when dirty under the ~60 Hz cap — there is no idle poll timer.
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
pub(crate) mod popup;
mod popup_selection;
mod preferences;
mod presentation;
mod probe;
mod reap;
mod reload;
mod routing;
mod runtime;
mod sched;
mod seen;
mod selection_copy;
mod settings;
mod shared;
mod shell_state;
mod synthesize;
mod tab_status;
mod update_flow;
mod view_sync;
mod waiting;
mod waiting_wire;
mod wake;
mod window_title;
mod word_selection;

pub use input::UiEvent;
pub use pane::{OpenAttach, PaneEv, Role, Spec};
pub use probe::{HostOutput, PANE_ROLE_NAMES, Probe, WAKE_NAMES};
pub(crate) use shell_state::ShellState;

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

/// Where host-mode control writes go (keyboard push/pop, mouse modes,
/// notifications): the outer terminal in production, a test recorder
/// otherwise. A boxed `FnMut` keeps the app free of global-IO calls.
pub type HostControlSink = Box<dyn FnMut(&[u8]) -> std::io::Result<()>>;

/// A forwarded press and the wire it went out on. `route` binds the
/// attach generation so a release can never land on a replacement
/// wire. Repeats route only while the lease's wire is still active —
/// releases for an unviewed pane still pass, its repeats do not.
type PaneLease = key_leases::ForwardedInputLease<u64>;
type KeyLeases = key_leases::InputLeaseTable<u8, key_routing::Context, u64>;

/// The whole application.
pub struct App {
    notice: Option<notices::Notice>,
    /// The one-shot `hc server start` report — `Some` until it lands.
    core_start: Option<crate::core_start::Report>,
    update_check: Option<crate::update_check::Report>,
    /// The last known newer release's bare version — feeds the
    /// sidebar/mobile version badge after the toast expires.
    pub update_latest: Option<String>,
    /// The `update-check.json` state — skip/seen/notes/show-on-startup.
    pub(crate) update_state: crate::update_state::UpdateCheckState,
    /// Where `update_state` persists; `None` without an XDG/HOME root.
    update_state_path: Option<std::path::PathBuf>,
    /// The update toast's card rect — a click opens the modal.
    update_toast_hit: ratatui::layout::Rect,
    /// A release-notes fetch is in flight.
    update_notes_fetching: bool,
    /// The release-list fetch — injected; offline under `cfg(test)`.
    notes_fetch: update_flow::NotesFetch,
    /// Update worker results land on this channel.
    update_tx: mpsc::Sender<update_flow::UpdateEvent>,
    update_rx: mpsc::Receiver<update_flow::UpdateEvent>,
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
    layout_confirmed: Option<(String, Arc<hc_proto::watch::Document>)>,
    prefix: input::Prefix,
    mode: Mode,
    keyboard_locked: bool,
    previous_pane: Option<String>,
    last_focus: Option<String>,
    dialog: Option<dialogs::Dialog>,
    dialog_hits: crate::chrome::dialogs::DialogRender,
    /// Open right-click context menu (`ClientShellOverlay::ContextMenu`).
    menu: Option<crate::chrome::menu::ContextMenu>,
    /// Panes whose bare right-click goes to the program instead of the
    /// context menu — the per-pane `right_click_passthrough` toggle.
    right_click_panes: std::collections::HashSet<String>,
    help: bool,
    mobile_switcher_suspended: bool,
    help_state: crate::chrome::help::HelpState,
    help_hits: crate::chrome::help::HelpRender,
    help_scroll_grab: Option<u16>,
    /// The "Go to" overlay (`ClientShellOverlay::Navigator`).
    navigator: Option<crate::chrome::navigator::NavigatorState>,
    /// Last frame's navigator hit rectangles.
    navigator_hits: crate::chrome::navigator_render::NavigatorRender,
    /// The Settings overlay — the reference's
    /// `ClientShellOverlay::Settings`.
    settings: Option<crate::chrome::settings::SettingsOverlay>,
    /// Last frame's settings hit rectangles.
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
    /// `shell_state.sidebar_collapsed` was toggled by hand — persisted
    /// as `preferences.sidebar_collapsed`.
    sidebar_collapsed_manual: bool,
    /// `shell_state.sidebar_section_split` was dragged by hand —
    /// persisted as `preferences.sidebar_section_split`.
    sidebar_section_split_manual: bool,
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
    /// The clipboard-image read — a shared field so tests never touch a
    /// real clipboard and can close over stub-tool environments.
    image_paste_reader: image_paste::ImageReader,
    /// The dropped-file image read — the same seam for the file-drop path.
    image_drop_reader: image_paste::DropReader,
    /// One image read in flight, holding the gesture that started it
    /// and the terminal input that arrived since — see `image_paste`.
    image_read: Option<image_paste::ImageRead>,
    /// Matches a worker result to `image_read` — a late result after the
    /// hold expired lands dead.
    image_read_seq: u64,
    /// The `spawn_blocking` join for `image_read` — aborted on drop.
    image_read_task: Option<tokio::task::JoinHandle<()>>,
    /// Worker results land on this channel; the select arm dispatches them.
    image_read_done_tx: mpsc::Sender<image_paste::ImageReadDone>,
    image_read_done_rx: mpsc::Receiver<image_paste::ImageReadDone>,
    /// How long terminal-directed input waits behind one image read —
    /// above the reader's own probe timeouts, so a never-returning
    /// worker still resolves the gesture as a miss.
    image_read_hold: std::time::Duration,
    /// `true` while the held trigger is being replayed through `on_input`
    /// after a miss — the replay must not start another read.
    image_read_replay: bool,
    /// Last frame's clickable rects.
    pub(crate) hit_map: ShellHitMap,
    /// The host reports every key as an escape code — the reference's
    /// `keyboard_report_all_active`, upgraded while the pane negotiates
    /// report-all or a binding mode for IME/text commits.
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
    /// `Some(true)` only on a recognized-and-reset report; `None` when
    /// it never answered. `?1016h` stays off until cell geometry can
    /// translate pixel coordinates (the FACTS surface).
    host_sgr_pixel_mouse: Option<bool>,
    host_pixel_mouse_active: bool,
    host_mouse_capture_active: bool,
    host_mouse_pixels: Option<crate::mouse_geometry::HostPixels>,
    host_appearance: Option<crate::chrome::palette::HostAppearance>,
    host_appearance_explicit: bool,
    /// Host-mode control writes (keyboard pop/push), injected by cli —
    /// headless tests get the no-op default.
    host_control: HostControlSink,
    /// Pane-bound forwarded presses — the reference's `InputLeaseTable`
    /// scoped to one local source and one attach. A routed press keeps
    /// a lease: repeats keep routing, releases close it, and a detach
    /// or target move parks the wire for drain so releases land.
    pane_leases: KeyLeases,
    next_dispatch_id: u64,
    /// Last watcher view.
    view: View,
    /// Incremental per-endpoint projection cache for `refresh_projection`.
    project_cache: crate::project::cache::ProjectCache,
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
    /// The reference's `mark_active_tab_seen`/`apply_pane_state_change`:
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
    /// RPCs, kept off the layout store so a write can't raise a notice.
    seen_tx: mpsc::Sender<seen::SeenWrite>,
    seen_res_tx: mpsc::Sender<seen::SeenResult>,
    seen_rx: mpsc::Receiver<seen::SeenResult>,
    seen_req_rx: Option<mpsc::Receiver<seen::SeenWrite>>,
    /// The seen writer's task — aborted on drop so its `Client` clone
    /// never outlives the app.
    seen_task: Option<tokio::task::JoinHandle<()>>,
    /// Live Waiting verdicts by terminal address — presentation-only
    /// evidence, pinned to the fingerprint it was probed for.
    waiting: crate::waiting::Verdicts,
    /// Probe cycles queued for the worker; the worker exists only
    /// after `set_operations` (`waiting_req_rx` holds its request end
    /// until then).
    waiting_tx: mpsc::Sender<waiting::Cycle>,
    waiting_res_tx: mpsc::Sender<waiting::Outcome>,
    waiting_rx: mpsc::Receiver<waiting::Outcome>,
    waiting_req_rx: Option<mpsc::Receiver<waiting::Cycle>>,
    /// The probe worker's task — aborted on drop like the seen writer.
    waiting_task: Option<tokio::task::JoinHandle<()>>,
    /// The armed re-probe cadence deadline (`wake_deadline` picks it up).
    waiting_due: Option<Instant>,
    /// `(terminal, created_ms)` reaps queued for the worker — `None`
    /// until `set_operations` starts it.
    reap_tx: Option<mpsc::Sender<(String, u64)>>,
    /// The reaper's task — aborted on drop like the seen writer's.
    reap_task: Option<tokio::task::JoinHandle<()>>,
    /// `(address, created_ms)` pairs already queued or handled — pruned
    /// as records leave the inventory, cleared on worker restart.
    reaped_exited: Arc<std::sync::Mutex<std::collections::HashSet<(String, u64)>>>,
    outer_focused: Option<bool>,
    probe: Arc<Probe>,
}

#[cfg(test)]
#[path = "mod_tests/mod.rs"]
mod tests;

mod hostname;
