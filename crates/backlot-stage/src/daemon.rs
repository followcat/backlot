//! The stage loop: each tick reloads any ledger that grew, reconciles
//! desired roles against the live terminal inventory and carries the
//! actions out ([`crate::act`]). Inputs are the ledger file lengths
//! (polled about once a second) and hc events via `events.subscribe`;
//! a core without it is polled on the same cadence. The summary
//! document regenerates every tick and is written on change, at most
//! once per gap. A studio whose ledger cannot be loaded is skipped —
//! and shown with `error` — until the file loads again, and a failing
//! action, watch or delivery logs once and moves on; losing the core
//! is the one failure that still exits the stage.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use backlot_model::state::StudioState;
use backlot_model::summary::{self, Summary, Work};
use hc_proto::client::{CallError, Client};
use hc_proto::events::SubscribeParams;
use hc_proto::subscribe::Subscription;
use hc_proto::terminal::ListParams;

use crate::args::Common;
use crate::checkrun;
use crate::checks;
use crate::checkwork;
use crate::cli::{Env, Fail};
use crate::config;
use crate::connerr;
use crate::deliver;
use crate::discover;
use crate::doc;
use crate::exchange_read;
use crate::link;
use crate::ratelimit;
use crate::reconcile;
use crate::relabel;
use crate::signals;
use crate::store::{self, StoreError};
use crate::watch;
use crate::workareas;

/// Poll cadence: ledgers grow-check and, without `events.subscribe`,
/// the terminal inventory.
const TICK: Duration = Duration::from_secs(1);

/// The live terminal view the loop works on.
#[derive(Default)]
pub(crate) struct Inventory {
    /// Every live named terminal; per-studio views come from labels.
    pub(crate) terms: Vec<reconcile::LiveTerminal>,
    /// Named records whose program has ended — the finish sweep's
    /// input, so a finished studio's dead panes leave every client.
    pub(crate) finished: Vec<reconcile::LiveTerminal>,
    /// Address → what the program reports doing, for the summary.
    pub(crate) doing: BTreeMap<String, String>,
}

/// Every live terminal the core knows — and, once a studio has
/// finished, every finished named record beside it, the finish
/// sweep's input — kept for the per-studio label views and with its
/// reported status for the summary. A core with no finished studio is
/// asked for live terminals only, so its older tombstones cost
/// nothing; one that does not report finished records simply reports
/// none, and the sweep then leaves the panes as they were.
async fn live_terminals(client: &Client, finished: bool) -> Result<Inventory, Fail> {
    let list = client
        .terminal_list(ListParams {
            all: finished,
            ..ListParams::default()
        })
        .await
        .map_err(Fail::call)?;
    let mut inv = Inventory {
        terms: Vec::new(),
        finished: Vec::new(),
        doing: BTreeMap::new(),
    };
    for t in list.terminals {
        if t.state == "finished"
            && (t.name.is_some()
                || t.labels.contains_key("backlot.role")
                || t.labels.contains_key("backlot.studio"))
        {
            inv.finished.push(t.into());
        } else if t.state == "live" && t.name.is_some() {
            if let Some(status) = &t.status {
                let doing = if status.msg.is_empty() {
                    status.state.clone()
                } else {
                    status.msg.clone()
                };
                if !doing.is_empty() {
                    inv.doing.insert(t.address.clone(), doing);
                }
            }
            inv.terms.push(t.into());
        }
    }
    Ok(inv)
}

/// The loop's refresh decision, split for tests: a dead wire
/// (`Fail::Core`) exits the stage; any other failed refresh keeps
/// the last inventory — `live_terminals` is tried again on the next
/// pass, and its message is logged once by the caller.
pub(crate) fn keep_or_fail(r: Result<Inventory, Fail>, last: Inventory) -> Result<Inventory, Fail> {
    match r {
        Err(e @ Fail::Core(_)) => Err(e),
        Err(_) => Ok(last),
        ok => ok,
    }
}

/// The refresh itself: the non-core failure's message is logged
/// once here, then the last view carries on.
async fn refresh_inventory(
    client: &Client,
    stage: &mut Stage,
    last: Inventory,
) -> Result<Inventory, Fail> {
    let r = live_terminals(client, stage.sweeps_finished()).await;
    if let Err(e) = &r
        && !matches!(e, Fail::Core(_))
    {
        stage.log_once("core", e.message());
    }
    keep_or_fail(r, last)
}

/// What one ledger file looked like when its state was loaded.
pub(crate) struct Seen {
    len: u64,
    mtime_ms: u64,
}

/// Connections and per-studio cache the loop owns. `act` reaches the
/// client and the role environment fields.
pub(crate) struct Stage {
    pub(crate) client: Client,
    /// The state base (`studios/<name>` lives under it).
    pub(crate) base: PathBuf,
    /// `PATH` for role terminals: this exe's directory first.
    pub(crate) path_env: String,
    /// This exe's own directory: it leads a configured `PATH` too.
    pub(crate) exe_dir: String,
    /// `BACKLOT_STATE_DIR` for role terminals.
    pub(crate) state_dir: String,
    /// The stage's own environment: adapters scrub inherited markers
    /// from it, and `HOME` locates runner session files.
    pub(crate) env_vars: BTreeMap<String, String>,
    /// How external commands run — `discover::real_exec` live, a fake
    /// in tests.
    pub(crate) exec: Box<discover::Exec>,
    /// Every terminal, git and role-file call the stage makes — a
    /// [`crate::driver::LocalDriver`] live, a recording or remote one to
    /// come.
    pub(crate) driver: Box<dyn crate::driver::MachineDriver>,
    /// How git runs for work areas — `workareas::real_git` live, a
    /// fake in tests. Shared with `driver`, so a test's replacement is
    /// what every routed git call uses.
    pub(crate) git: crate::driver::SharedGit,
    /// How a stage check spawns — `checkrun::real_spawn` live, a
    /// fake in tests.
    pub(crate) spawn_check: Box<checkrun::Spawn>,
    /// `studio` → the one check process in flight for it; a restart
    /// re-derives what is owed from the ledger.
    pub(crate) checking: std::cell::RefCell<BTreeMap<String, checkwork::Running>>,
    /// How outward push/PR card commands spawn (`real_spawn_argv` live, scripted in tests).
    pub(crate) spawn_outward: Box<crate::hcard_outward::OutwardSpawn>,
    /// In-flight non-blocking outward card processes, keyed by `card.seq`.
    pub(crate) outward_running:
        std::cell::RefCell<BTreeMap<u64, crate::hcard_outward::OutwardRunning>>,
    /// Studios whose leftover `checks/run-*` trees the first pass
    /// already swept.
    pruned: std::cell::RefCell<BTreeSet<String>>,
    /// `<program> --version` answers, asked once per launch program
    /// (`argv[0]`) per run.
    pub(crate) versions: std::cell::RefCell<BTreeMap<String, Option<String>>>,
    /// Session ids the running pass recorded — `cached` cannot see
    /// them until the ledgers reload, so the watch adds them itself.
    pub(crate) found_sessions: std::cell::RefCell<std::collections::BTreeSet<String>>,
    /// `(studio, role)` → when discovery last looked for the
    /// session id — the watch's five-second throttle.
    pub(crate) discover_looked: std::cell::RefCell<BTreeMap<(String, String), u64>>,
    /// `(studio, role)` → the trust exchange's pace: the dialog's
    /// first sighting and the last key sent.
    pub(crate) trust_pace: std::cell::RefCell<BTreeMap<(String, String), watch::TrustPace>>,
    /// `(studio, role)` → the screen's latest work classification —
    /// the watch samples it, the summary reads it. It stays out of
    /// the ledger entirely.
    pub(crate) work: std::cell::RefCell<BTreeMap<(String, String), Work>>,
    /// `(studio, role)` → when the work sample last read the screen —
    /// the one-second throttle.
    pub(crate) work_looked: std::cell::RefCell<BTreeMap<(String, String), u64>>,
    /// `(studio, role)` → the screen-stability track the send gate
    /// reads — updated with each work sample.
    pub(crate) screens: std::cell::RefCell<BTreeMap<(String, String), deliver::Screen>>,
    /// `(studio, role)` → when the last `terminal.send` to it
    /// finished — ok or refused — for the five-second send gap.
    pub(crate) sent_ms: std::cell::RefCell<BTreeMap<(String, String), u64>>,
    /// `(studio, role)` → the one in-flight `terminal.send`, spawned
    /// off the loop and collected on a later tick.
    pub(crate) sending: std::cell::RefCell<BTreeMap<(String, String), deliver::InFlight>>,
    /// `(studio, role)` → when the current blocked episode started;
    /// the D9 attention dedups per episode, not per sample.
    pub(crate) blocked_since: std::cell::RefCell<BTreeMap<(String, String), u64>>,
    /// `(studio, role)` → when the role was first seen out of its
    /// blocked episode — a permission prompt answered elsewhere gets
    /// one window before its stale attention clears.
    pub(crate) unblocked_since: std::cell::RefCell<BTreeMap<(String, String), u64>>,
    /// `(studio, role)` → start of the current `question` dialog
    /// episode, kept independent of `permission prompt` episodes.
    pub(crate) q_blocked_since: std::cell::RefCell<BTreeMap<(String, String), u64>>,
    /// `(studio, role)` → when the role left a `question` dialog
    /// before clearing `dialog closed` after `EPISODE_MIN_MS`.
    pub(crate) q_unblocked_since: std::cell::RefCell<BTreeMap<(String, String), u64>>,
    /// `(studio, role)` → when the current idle stretch began — the
    /// nudge pass's clock. In-memory: a restart starts it over.
    pub(crate) idle_since: std::cell::RefCell<BTreeMap<(String, String), u64>>,
    /// `(studio, role)` → the last hook signal from the role's own
    /// session (S9f) — the first work source, the signal stall's
    /// clock and the approval bind's request identity.
    pub(crate) last_signal: std::cell::RefCell<BTreeMap<(String, String), signals::Signal>>,
    /// `(studio, role)` -> unified D4 liveness evidence track (hook signal,
    /// activity revision, normalised screen, local process tree, local work
    /// area, and role ledger activity).
    pub(crate) liveness: std::cell::RefCell<BTreeMap<(String, String), crate::liveness::Evidence>>,
    /// How local process trees are sampled — `liveness_probe::real_proc_tree`
    /// live, or an injected fake in tests.
    pub(crate) proc_read: Box<crate::liveness_probe::ProcRead>,
    /// How local git work areas are sampled — `liveness_diff::real_work_area`
    /// live, or an injected fake in tests.
    pub(crate) work_area_read: Box<crate::liveness_diff::WorkAreaRead>,
    /// Whether the stage's `hc` core runs on this local host (`false` when
    /// connected to a remote endpoint, disabling OS process/git probes).
    pub(crate) is_local: bool,
    /// `(studio, role)` → the rate-limit retry exchange (S12d):
    /// `continue`s already sent and when the next is due. An entry
    /// that outlives its attention marks the episode adjudicated —
    /// a manually cleared or escalated limit never re-raises while
    /// it holds.
    pub(crate) rate_pace: std::cell::RefCell<BTreeMap<(String, String), ratelimit::RatePace>>,
    /// `(studio, role)` → the connection-error retry exchange: the
    /// same adjudication invariant as `rate_pace`, on a shorter
    /// 15-second wait.
    pub(crate) conn_pace: std::cell::RefCell<BTreeMap<(String, String), connerr::ConnPace>>,
    /// `(studio, role)` pairs whose session-less state passed the
    /// default window and already logged its soft warning — the
    /// runner's own window still bounds the attention.
    pub(crate) session_warned: std::cell::RefCell<std::collections::BTreeSet<(String, String)>>,
    /// Reduced state per studio, reloaded when its log grew.
    pub(crate) cached: BTreeMap<String, StudioState>,
    /// `events.jsonl` length and mtime at each state's last load.
    pub(crate) seen: BTreeMap<String, Seen>,
    /// Studio → the last load error while its ledger stays unreadable:
    /// the tick skips it and the summary shows it.
    pub(crate) broken: BTreeMap<String, String>,
    /// Studio → the last pass failure — logged once, shown in the
    /// summary, but the studio still runs next tick. A good pass
    /// clears it; a broken ledger's error shows instead while both
    /// stand.
    pub(crate) failed: BTreeMap<String, String>,
    /// Every error line already printed — each distinct message logs
    /// once, whatever its studio.
    pub(crate) logged: BTreeSet<String>,
    /// Hollywood's reduced state — the overseer is not a studio, so
    /// it keeps its own ledger, its own state and its own seen mark
    /// (`hseen`) under `<base>/hollywood/` (D13).
    pub(crate) hstate: std::cell::RefCell<backlot_model::hollywood::HollywoodState>,
    /// `events.jsonl` (len, mtime) at `hstate`'s last load.
    pub(crate) hseen: std::cell::RefCell<(u64, u64)>,
    /// The `[hollywood]` config the tick last read — the summary's
    /// runner/model/effort line, and absent means no Hollywood at all.
    pub(crate) hcfg: std::cell::RefCell<Option<config::HollywoodCfg>>,
    /// The `[stage]` config the tick last read — the stage daemon's
    /// own switches. A malformed file logs once and keeps the default
    /// (`answer_trust` on): a config typo never silently disables the
    /// disclosure the wizard promised.
    pub(crate) stage_cfg: std::cell::Cell<config::StageCfg>,
    /// The `[design]` config the tick last read — layered across
    /// `<state_dir>/config.toml` and user config (`hook_capture` on by
    /// default).
    pub(crate) design_cfg: std::cell::Cell<Option<config::DesignCfg>>,
    /// Hollywood-pass error lines already printed — same once-per-
    /// message rule as `logged`, kept apart so `&self` passes can use it.
    pub(crate) hlogged: std::cell::RefCell<BTreeSet<String>>,
    /// The permission prompt or dialog on `bl-hollywood`'s screen,
    /// read by the last work sample — the summary publishes it as
    /// `needs the user` and, for permission prompts, the band offers
    /// the answer.
    pub(crate) hprompt: std::cell::RefCell<Option<String>>,
    /// True when `hprompt` is a non-permission `question` dialog
    /// (`allow`/`deny` keys are suppressed).
    pub(crate) hprompt_is_question: std::cell::Cell<bool>,
    /// The prompt whose episode Hollywood's answer belongs to, with
    /// the ledger `seq` when it first appeared — an approval recorded
    /// before that is somebody else's.
    pub(crate) hprompt_base: std::cell::RefCell<Option<(String, u64)>>,
    /// Approval seqs already answered on Hollywood's terminal, so a
    /// slow screen never sends the same key twice.
    pub(crate) hanswered: std::cell::RefCell<BTreeSet<u64>>,
    /// Passive context keys (`card.done`, `accepted`) currently riding
    /// with an in-flight `terminal.send` to Hollywood.
    pub(crate) hpassive_sending:
        std::cell::RefCell<BTreeMap<(String, String), Vec<crate::hnotice::PassiveKey>>>,
    /// Passive context keys already accounted for in a finished delivery
    /// (loaded once from disk per `hdir` so stage restarts never repeat).
    pub(crate) hpassive_done: std::cell::RefCell<BTreeSet<crate::hnotice::PassiveKey>>,
    /// True once `hdir/passive-accounted.json` has been loaded into `hpassive_done`.
    pub(crate) hpassive_loaded: std::cell::Cell<bool>,
    /// True once this stage run has synced `hdir/AGENTS.md` on boot.
    pub(crate) hagents_synced: std::cell::Cell<bool>,
    /// Cached `(studio, seq) -> snippet` lines for `StudioSummary.exchange`
    /// so immutable message/report files are read from disk at most once.
    pub(crate) exchange_cache: std::cell::RefCell<BTreeMap<(String, u64), String>>,
    /// Per-`(studio, role)` tracking for terminal-answered open `tell user`
    /// questions.
    pub(crate) ask_watch:
        std::cell::RefCell<BTreeMap<(String, String), crate::ask_answer::AskWatch>>,
    /// `(studio, role)` → highest open `tell user` seq touched by a
    /// stage terminal write (covers ledgerless sends such as `continue`
    /// and trust navigation keys).
    pub(crate) ask_tainted: std::cell::RefCell<BTreeMap<(String, String), u64>>,
    /// Per-studio tracking for `producer` terminal input capture and
    /// unrecorded-turn fallback gating during `DesignPhase::WaitingUser`.
    pub(crate) design_watch: std::cell::RefCell<BTreeMap<String, crate::design_watch::DesignWatch>>,
    /// `studio` → true while a stage terminal write to `producer` during
    /// `WaitingUser` has not yet finished its resulting turn.
    pub(crate) design_tainted: std::cell::RefCell<BTreeMap<String, bool>>,
    /// Whether the core still answers `terminal.label` on a live
    /// terminal — `false` once a call is refused or unoffered: a core
    /// that cannot change labels keeps every terminal where it
    /// launched, and the relabel pass stops asking (U2).
    pub(crate) labels_ok: std::cell::Cell<bool>,
    /// Remaining ticks to include `finished` records in `terminal.list`
    /// so dead `bl-hollywood` tombstones from this or an earlier launch
    /// get swept via `terminal.rm` even with no finished studio.
    pub(crate) sweep_hollywood_ticks: std::cell::Cell<u8>,
    /// Rate/connection/error watch state for active Hollywood.
    pub(crate) hrate_pace: std::cell::RefCell<Option<crate::ratelimit::RatePace>>,
    pub(crate) hconn_pace: std::cell::RefCell<Option<crate::connerr::ConnPace>>,
    pub(crate) herror: std::cell::RefCell<Option<crate::handoff::HBlockedKind>>,
    /// Retention expiry (unix ms) for the `hollywood-previous` tombstone.
    pub(crate) hprevious_until_ms: std::cell::Cell<Option<u64>>,
    /// Cached summary projection of the active or most recent handoff.
    pub(crate) hlast_handoff: std::cell::RefCell<Option<backlot_model::summary::HandoffSummary>>,
    /// When this stage process was born — a `cannot resume` close
    /// older than it means the person restarted the stage, the one
    /// way the give-up comes back (R1-4).
    pub(crate) started_ms: u64,
}

impl Stage {
    /// The git runner every routed git call drives — the same handle the
    /// driver holds, so a test's replacement reaches both.
    pub(crate) fn git_runner(&self) -> std::cell::RefMut<'_, Box<workareas::GitRun<'static>>> {
        self.git.borrow_mut()
    }

    /// `eprintln!` each distinct line once — a studio stuck in a
    /// failing pass reports its error without spamming every tick.
    fn log_once(&mut self, studio: &str, msg: &str) {
        if self.logged.insert(format!("{studio}: {msg}")) {
            crate::stagelog::stage_log!("{studio}: {msg}");
        }
    }

    /// Re-read the user config's `[stage]` section for this tick —
    /// the same file the launch env and `[hollywood]` come from. An
    /// error logs once and the last read stands, so a broken edit
    /// never flips the stage's own switches silently.
    fn read_stage_config(&self) {
        match config::stage(&self.env_vars) {
            Ok(cfg) => self.stage_cfg.set(cfg),
            Err(e) => {
                if self.hlogged.borrow_mut().insert(e.clone()) {
                    crate::stagelog::stage_log!("{e}");
                }
            }
        }
        let state_dir = (!self.state_dir.is_empty()).then(|| Path::new(&self.state_dir));
        match config::design_layered(state_dir, &self.env_vars) {
            Ok(cfg) => self.design_cfg.set(Some(cfg)),
            Err(e) => {
                if self.hlogged.borrow_mut().insert(e.clone()) {
                    crate::stagelog::stage_log!("{e}");
                }
            }
        }
    }

    /// Evict every in-memory studio key not in `present` (deleted/archived
    /// studios disappear immediately without restart or stale errors),
    /// while preserving Hollywood's own synthetic `(HOLLYWOOD, HOLLYWOOD)`
    /// entries.
    fn evict_missing(&mut self, present: &BTreeSet<String>) {
        let keep = |s: &str| present.contains(s) || s == backlot_model::hollywood::HOLLYWOOD;
        self.cached.retain(|k, _| present.contains(k));
        self.seen.retain(|k, _| present.contains(k));
        self.broken.retain(|k, _| present.contains(k));
        self.failed.retain(|k, _| present.contains(k));
        self.pruned.borrow_mut().retain(|k| present.contains(k));
        self.checking
            .borrow_mut()
            .retain(|k, _| present.contains(k));
        self.discover_looked
            .borrow_mut()
            .retain(|(s, _), _| keep(s));
        self.trust_pace.borrow_mut().retain(|(s, _), _| keep(s));
        self.work.borrow_mut().retain(|(s, _), _| keep(s));
        self.work_looked.borrow_mut().retain(|(s, _), _| keep(s));
        self.screens.borrow_mut().retain(|(s, _), _| keep(s));
        self.sent_ms.borrow_mut().retain(|(s, _), _| keep(s));
        self.sending.borrow_mut().retain(|(s, _), _| keep(s));
        self.blocked_since.borrow_mut().retain(|(s, _), _| keep(s));
        self.unblocked_since
            .borrow_mut()
            .retain(|(s, _), _| keep(s));
        self.q_blocked_since
            .borrow_mut()
            .retain(|(s, _), _| keep(s));
        self.q_unblocked_since
            .borrow_mut()
            .retain(|(s, _), _| keep(s));
        self.idle_since.borrow_mut().retain(|(s, _), _| keep(s));
        self.last_signal.borrow_mut().retain(|(s, _), _| keep(s));
        self.liveness.borrow_mut().retain(|(s, _), _| keep(s));
        self.rate_pace.borrow_mut().retain(|(s, _), _| keep(s));
        self.conn_pace.borrow_mut().retain(|(s, _), _| keep(s));
        self.session_warned.borrow_mut().retain(|(s, _)| keep(s));
        self.exchange_cache.borrow_mut().retain(|(s, _), _| keep(s));
        self.ask_watch.borrow_mut().retain(|(s, _), _| keep(s));
        self.ask_tainted.borrow_mut().retain(|(s, _), _| keep(s));
        self.design_watch.borrow_mut().retain(|s, _| keep(s));
        self.design_tainted.borrow_mut().retain(|s, _| keep(s));
        let mut outward = self.outward_running.borrow_mut();
        for entry in outward.values_mut() {
            if !present.contains(&entry.studio) {
                entry.child.kill();
            }
        }
        outward.retain(|_, r| present.contains(&r.studio));
    }

    /// Evict a single studio key across all in-memory caches (used when a
    /// studio disappears mid-tick).
    fn evict_studio(&mut self, name: &str) {
        self.cached.remove(name);
        self.seen.remove(name);
        self.broken.remove(name);
        self.failed.remove(name);
        self.pruned.borrow_mut().remove(name);
        self.checking.borrow_mut().remove(name);
        self.discover_looked
            .borrow_mut()
            .retain(|(s, _), _| s != name);
        self.trust_pace.borrow_mut().retain(|(s, _), _| s != name);
        self.work.borrow_mut().retain(|(s, _), _| s != name);
        self.work_looked.borrow_mut().retain(|(s, _), _| s != name);
        self.screens.borrow_mut().retain(|(s, _), _| s != name);
        self.sent_ms.borrow_mut().retain(|(s, _), _| s != name);
        self.sending.borrow_mut().retain(|(s, _), _| s != name);
        self.blocked_since
            .borrow_mut()
            .retain(|(s, _), _| s != name);
        self.unblocked_since
            .borrow_mut()
            .retain(|(s, _), _| s != name);
        self.q_blocked_since
            .borrow_mut()
            .retain(|(s, _), _| s != name);
        self.q_unblocked_since
            .borrow_mut()
            .retain(|(s, _), _| s != name);
        self.idle_since.borrow_mut().retain(|(s, _), _| s != name);
        self.last_signal.borrow_mut().retain(|(s, _), _| s != name);
        self.liveness.borrow_mut().retain(|(s, _), _| s != name);
        self.rate_pace.borrow_mut().retain(|(s, _), _| s != name);
        self.conn_pace.borrow_mut().retain(|(s, _), _| s != name);
        self.session_warned.borrow_mut().retain(|(s, _)| s != name);
        self.exchange_cache
            .borrow_mut()
            .retain(|(s, _), _| s != name);
        self.ask_watch.borrow_mut().retain(|(s, _), _| s != name);
        self.ask_tainted.borrow_mut().retain(|(s, _), _| s != name);
        self.design_watch.borrow_mut().retain(|s, _| s != name);
        self.design_tainted.borrow_mut().retain(|s, _| s != name);
        let mut outward = self.outward_running.borrow_mut();
        for entry in outward.values_mut() {
            if entry.studio == name {
                entry.child.kill();
            }
        }
        outward.retain(|_, r| r.studio != name);
    }

    /// Reload changed ledgers, reconcile each studio and act. Each
    /// studio sees only the terminals its label owns — the name is
    /// never enough to stop, adopt or drop. One studio's bad ledger
    /// or failing pass logs and skips it; the others carry on, and
    /// only a lost core (`Fail::Core`) ends the stage.
    pub(crate) async fn tick(&mut self, live: &Inventory) -> Result<(), Fail> {
        self.read_stage_config();
        let present: BTreeSet<String> = store::studios(&self.base)
            .map_err(|e| Fail::refused(format!("cannot read {}: {e}", self.base.display())))?
            .into_iter()
            .collect();
        self.evict_missing(&present);
        for name in present {
            let dir = store::studio_dir(&self.base, &name);
            if !dir.join("events.jsonl").is_file() {
                self.evict_studio(&name);
                continue;
            }
            let meta = fs::metadata(dir.join("events.jsonl"));
            let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            // A broken studio retries every tick — a repair that keeps
            // the length, or a transient read error, must not leave it
            // skipped until something appends.
            if self.seen.get(&name).map(|s| s.len) != Some(len) || self.broken.contains_key(&name) {
                if !dir.join("events.jsonl").is_file() {
                    self.evict_studio(&name);
                    continue;
                }
                match load_studio(&dir) {
                    Ok(state) => {
                        if !state.unknown_kinds.is_empty() {
                            let msg = backlot_model::refusal::upgrade_required_message(
                                state.unknown_kinds.iter().map(String::as_str),
                            );
                            self.log_once(&name, &msg);
                        }
                        self.cached.insert(name.clone(), state);
                        self.broken.remove(&name);
                    }
                    // A ledger that cannot be loaded marks the studio
                    // broken; it stays skipped until the file loads.
                    Err(e) => {
                        if !dir.join("events.jsonl").is_file() {
                            self.evict_studio(&name);
                            continue;
                        }
                        let msg = e.message().to_string();
                        self.broken.insert(name.clone(), msg.clone());
                        self.log_once(&name, &msg);
                    }
                }
            }
            let mtime_ms = meta
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            self.seen.insert(name, Seen { len, mtime_ms });
        }
        // At most one real runner launches per tick, across studios.
        let mut real_slot_free = true;
        // And at most one live role still looking for its post-launch
        // session id per (runner, folder) — a second start in the
        // same pair would race it for the same session.
        let blocked =
            watch::discovery_blocked(&self.cached, &|name| store::studio_dir(&self.base, name));
        self.found_sessions.borrow_mut().clear();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        for name in self.cached.keys().cloned().collect::<Vec<_>>() {
            if self.broken.contains_key(&name) {
                continue;
            }
            match self
                .studio_pass(&name, live, &mut real_slot_free, &blocked, now_ms)
                .await
            {
                Err(e @ Fail::Core(_)) => return Err(e),
                Err(e) => {
                    if !store::studio_dir(&self.base, &name)
                        .join("events.jsonl")
                        .is_file()
                    {
                        self.evict_studio(&name);
                    } else {
                        self.failed.insert(name.clone(), e.message().to_string());
                        self.log_once(&name, e.message());
                    }
                }
                Ok(()) => {
                    self.failed.remove(&name);
                }
            }
        }
        // Hollywood's own pass after the studios: it reads the fresh
        // cached states for notices and card bases — and a card's own
        // studio-new may have just created the ledger the next tick
        // picks up.
        self.hollywood(&live.terms, &live.finished, &mut real_slot_free, now_ms)
            .await?;
        Ok(())
    }

    /// One studio's reconcile → act → watch → deliver for a tick, so
    /// any failure inside it is one studio's, never the stage's.
    pub(crate) async fn studio_pass(
        &self,
        name: &str,
        live: &Inventory,
        real_slot_free: &mut bool,
        blocked: &BTreeSet<(String, String)>,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let dir = store::studio_dir(&self.base, name);
        let live_ledger = || dir.join("events.jsonl").is_file();
        if !live_ledger() {
            return Ok(());
        }
        let state = &self.cached[name];
        if !state.unknown_kinds.is_empty() {
            return Ok(());
        }
        let view = reconcile::studio_view(name, &live.terms);
        for action in reconcile::reconcile(name, state, &view, now_ms) {
            if !live_ledger() {
                return Ok(());
            }
            self.act(name, &dir, state, &action, real_slot_free, blocked)
                .await?;
        }
        // A finished studio's dead panes: the kill landed, the record
        // did not. Ownership is the `backlot.studio` label, so another
        // studio's record and the user's own shells stay.
        for action in reconcile::finish_sweep(name, state, &live.finished) {
            if !live_ledger() {
                return Ok(());
            }
            self.act(name, &dir, state, &action, real_slot_free, blocked)
                .await?;
        }
        if !live_ledger() {
            return Ok(());
        }
        self.relabel(name, state, &live.terms).await?;
        if !live_ledger() {
            return Ok(());
        }
        self.watch(name, &dir, state, &live.terms, now_ms).await?;
        if !live_ledger() {
            return Ok(());
        }
        self.deliver(name, &dir, state, &live.terms, now_ms).await?;
        if !live_ledger() {
            return Ok(());
        }
        self.check(name, &dir, state, now_ms)
    }

    /// The U2 relabel: a live role terminal still carrying the old
    /// shared `backlot` space gets its studio's own space and the
    /// person's tab — `relabel::wanted_labels` keeps the pass
    /// idempotent, and the first refused or unoffered `terminal.label`
    /// ends it for good (`labels_ok`): a core that cannot change
    /// labels keeps every terminal where it launched.
    async fn relabel(
        &self,
        name: &str,
        state: &StudioState,
        terms: &[reconcile::LiveTerminal],
    ) -> Result<(), Fail> {
        if !self.labels_ok.get() {
            return Ok(());
        }
        for term in terms {
            let Some(patch) = relabel::wanted_labels(name, state, term) else {
                continue;
            };
            match self.driver.label(&self.client, &term.address, &patch).await {
                Ok(_) => {}
                // A timeout is transient — the terminal is asked again
                // next tick; a dead wire stays fatal.
                Err(CallError::Timeout) => continue,
                Err(e @ (CallError::Closed | CallError::Protocol(_))) => {
                    return Err(Fail::call(e));
                }
                Err(e) => {
                    self.labels_ok.set(false);
                    let msg = format!("relabel off: {e}");
                    if self.hlogged.borrow_mut().insert(msg.clone()) {
                        crate::stagelog::stage_log!("{msg}");
                    }
                }
            }
        }
        Ok(())
    }

    /// The check pass: poll the in-flight run to its `task.checked`
    /// append, or start the ledger's next owed check — at most one
    /// process per studio and never a wait that blocks the tick.
    pub(crate) fn check(
        &self,
        name: &str,
        dir: &Path,
        state: &StudioState,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let done = {
            let mut map = self.checking.borrow_mut();
            match map.get_mut(name) {
                Some(run) => run
                    .poll(now_ms)
                    .map(|out| (map.remove(name).expect("polled run"), out)),
                None => None,
            }
        };
        if let Some((run, out)) = done {
            return checkwork::finish(&mut *self.driver.git(), dir, state, run, out);
        }
        // One run per studio: a live run owns the slot — owed starts
        // only when the slot is empty, or the same check would be
        // spawned again every tick.
        if self.checking.borrow().contains_key(name) {
            return Ok(());
        }
        // The first pass sweeps the run trees a dead stage left —
        // but the ones a still-owed delivery needs.
        if self.pruned.borrow_mut().insert(name.to_string()) {
            checkwork::prune_run_trees(&mut *self.driver.git(), dir, state);
        }
        if let Some(owed) = checks::owed(state) {
            // `start`'s borrow ends before the match — the fail
            // path takes `git` again for its own cleanup.
            let started = checkwork::start(
                &mut *self.driver.git(),
                &*self.spawn_check,
                state,
                dir,
                &self.env_vars,
                &owed,
                now_ms,
            );
            match started {
                Ok(run) => {
                    self.checking.borrow_mut().insert(name.into(), run);
                }
                // A check that cannot start is still recorded —
                // failed — so the owed loop does not retry it
                // forever and the reviewer's notice can land.
                Err(e) => {
                    checkwork::fail_start(&mut *self.driver.git(), dir, state, &owed, e.message())?
                }
            }
        }
        Ok(())
    }

    /// Whether any cached studio has finished — the only state the
    /// finish sweep has work in, so finished records are listed for
    /// no other reason.
    pub(crate) fn sweeps_finished(&self) -> bool {
        self.cached.values().any(|state| state.finished.is_some())
            || self.sweep_hollywood_ticks.get() > 0
            || self.hprevious_until_ms.get().is_some()
    }

    /// The `backlot` document's view: cached states, most recently
    /// changed first (the summary keeps live studios and lets old
    /// finished ones drop off). A broken studio with no loaded state
    /// still gets a row — its `error` is all the panel can show.
    pub(crate) fn summary_view(&self, doing: &BTreeMap<String, String>) -> Summary {
        self.summary_view_at(doing, crate::live::now_ms())
    }

    pub(crate) fn summary_view_at(&self, doing: &BTreeMap<String, String>, now_ms: u64) -> Summary {
        let mut pairs: Vec<(&String, &StudioState)> = self.cached.iter().collect();
        pairs.sort_by_key(|(name, _)| {
            std::cmp::Reverse(self.seen.get(*name).map(|s| s.mtime_ms).unwrap_or(0))
        });
        let mut owned: Vec<(String, StudioState)> = pairs
            .into_iter()
            .map(|(n, s)| (n.clone(), s.clone()))
            .collect();
        for name in self.broken.keys() {
            if !self.cached.contains_key(name) {
                owned.push((name.clone(), StudioState::default()));
            }
        }
        // The ledger error first, otherwise the last pass failure.
        let mut errors = self.failed.clone();
        errors.extend(self.broken.iter().map(|(k, v)| (k.clone(), v.clone())));
        // `(studio, role)` → StopFailure error for a role whose last
        // hook signal was a StopFailure (S9f) — `last turn failed:
        // <error>` until its next signal.
        let failed: BTreeMap<(String, String), String> = self
            .last_signal
            .borrow()
            .iter()
            .filter(|(_, s)| s.event == "StopFailure")
            .map(|(k, s)| {
                (
                    k.clone(),
                    s.error.clone().unwrap_or_else(|| "unknown".into()),
                )
            })
            .collect();
        let snippets = exchange_read::studio_snippets(
            &self.base,
            &owned,
            &mut self.exchange_cache.borrow_mut(),
        );
        let proc_running: BTreeMap<(String, String), summary::RoleRunning> = {
            let work = self.work.borrow();
            self.liveness
                .borrow()
                .iter()
                .filter(|(pair, _)| work.get(*pair) == Some(&Work::Working))
                .filter_map(|(pair, ev)| {
                    ev.deepest_cmd().map(|(cmd, since_ms)| {
                        (
                            pair.clone(),
                            summary::RoleRunning {
                                cmd: summary::cap_running(cmd),
                                since_ms,
                            },
                        )
                    })
                })
                .collect()
        };
        let mut s = summary::summary_with_running(
            &owned,
            doing,
            &self.work.borrow(),
            &failed,
            &errors,
            &snippets,
            &proc_running,
            0,
        );
        summary::attach_outward_cards(&mut s, &self.hstate.borrow());
        // U6c: the stage's own version rides the document so clients
        // read each machine's backlot version without probing. The
        // build id joins it when the build provided one (S2).
        s.backlot_version = Some(env!("CARGO_PKG_VERSION").to_string());
        s.backlot_build = option_env!("BACKLOT_BUILD_ID").map(str::to_owned);
        // U2: each role's workarea — the pure `role_dir` answer over
        // the studio dir, so a studio shell opens where the person it
        // was focused on works. Waiting rows get theirs too: the dir
        // is fixed before the role is live.
        for studio in &mut s.studios {
            let Some(state) = self.cached.get(&studio.id) else {
                continue;
            };
            let Some(cfg) = &state.studio else {
                continue;
            };
            let dir = store::studio_dir(&self.base, &studio.id);
            for role in &mut studio.roles {
                role.workdir = Some(
                    workareas::role_dir(&dir, &cfg.repo, &role.role, cfg.no_git)
                        .to_string_lossy()
                        .into_owned(),
                );
                // The dock reads `idle <n>m` from the same stretch the
                // nudge pass keeps — only while the role is live and
                // waiting at its prompt.
                if role.state == "live" && role.work == Some(Work::Waiting) {
                    role.idle_since_ms = self
                        .idle_since
                        .borrow()
                        .get(&(studio.id.clone(), role.role.clone()))
                        .copied();
                }
                let pair = (studio.id.clone(), role.role.clone());
                let work = self.work.borrow().get(&pair).copied().or(role.work);
                let rate_active = self.rate_pace.borrow().contains_key(&pair);
                let terminal_live = role.state == "live" && role.terminal.is_some();
                role.queue = crate::queue_status::role_queue_status(
                    state,
                    role,
                    work,
                    rate_active,
                    terminal_live,
                    now_ms,
                );
            }
        }
        // The optional `hollywood` object — absent unless `[hollywood]`
        // configures the overseer (D16).
        if let Some(cfg) = self.hcfg.borrow().as_ref() {
            let hs = self.hstate.borrow();
            let state = if !hs.role.recorded() || hs.role.closed {
                "down"
            } else if hs.role.live {
                "live"
            } else {
                "lost"
            };
            let hwork = self.work.borrow().get(&crate::hstage::hkey()).copied();
            // `doing` is the same read a role's takes: the terminal's
            // reported status, keyed by its address.
            let hdoing = hs
                .role
                .terminal
                .as_ref()
                .and_then(|t| doing.get(t))
                .map(String::as_str);
            let mut hw = backlot_model::summary::hollywood_section_with_activity(
                &hs,
                state,
                Some(&cfg.runner),
                Some(&cfg.model),
                cfg.effort.as_deref(),
                hwork,
                hdoing,
                self.hprompt.borrow().as_deref(),
            );
            if hw.attention.is_some() && self.hprompt_is_question.get() {
                hw.attention = Some(format!(
                    "{} — needs the user",
                    backlot_model::event::QUESTION
                ));
                hw.can_answer_prompt = false;
            }
            if hw.attention.is_none()
                && let Some(err) = self.herror.borrow().as_ref()
            {
                let (reason, detail) = err.attention_fields();
                hw.attention = Some(reason.to_string());
                hw.attention_detail = Some(detail);
                hw.attention_user = true;
                hw.can_answer_prompt = false;
            }
            let hdir = store::hollywood_dir(&self.base);
            let handoff = self.hlast_handoff.borrow().clone().or_else(|| {
                crate::handoff::load_handoff(&hdir)
                    .ok()
                    .flatten()
                    .filter(|r| r.visible_in_summary(now_ms))
                    .map(|r| r.to_summary())
            });
            let queued_count = hs
                .messages
                .iter()
                .filter(|m| {
                    m.to == backlot_model::hollywood::HOLLYWOOD
                        && !m.delivered
                        && !m.dropped
                        && !m.seen
                })
                .count() as u32;
            if queued_count > 0 {
                let (reason, detail) = if state == "down" {
                    (
                        backlot_model::summary::QueueReason::Down,
                        Some("hollywood is not running".into()),
                    )
                } else if let Some(ref h) = handoff
                    && matches!(
                        h.phase.as_str(),
                        "preparing" | "starting" | "verifying" | "needs_approval" | "committing"
                    )
                {
                    let eff = h
                        .target_effort
                        .as_deref()
                        .map(|e| format!("/{e}"))
                        .unwrap_or_default();
                    (
                        backlot_model::summary::QueueReason::Handoff,
                        Some(format!("{}/{}{eff}", h.target_runner, h.target_model)),
                    )
                } else if let Some(err) = self.herror.borrow().as_ref() {
                    err.queue_reason()
                } else if self.hprompt.borrow().is_some() || hwork == Some(Work::Blocked) {
                    (
                        backlot_model::summary::QueueReason::Prompt,
                        self.hprompt.borrow().clone(),
                    )
                } else if hwork == Some(Work::Working) {
                    (backlot_model::summary::QueueReason::Busy, hw.doing.clone())
                } else if now_ms.saturating_sub(hs.role.since_ms) < crate::deliver::MIN_UP_MS {
                    (backlot_model::summary::QueueReason::Starting, None)
                } else {
                    (backlot_model::summary::QueueReason::WaitingQuiet, None)
                };
                hw.queue = Some(backlot_model::summary::QueueStatus {
                    count: queued_count,
                    reason,
                    detail,
                });
            }
            hw.handoff = handoff;
            s.hollywood = Some(hw);
        }
        s
    }
}

#[cfg(test)]
impl Stage {
    /// A stage for unit tests: the given client, every track empty.
    /// `sample_work`/`deliver` need no base or state dir.
    pub(crate) fn for_test(client: Client) -> Stage {
        let git = crate::driver::shared_git();
        Stage {
            client,
            base: PathBuf::new(),
            path_env: String::new(),
            exe_dir: String::new(),
            state_dir: String::new(),
            env_vars: BTreeMap::new(),
            exec: Box::new(discover::real_exec),
            driver: Box::new(crate::driver::LocalDriver::with_git(git.clone())),
            git,
            spawn_check: Box::new(checkrun::real_spawn),
            checking: std::cell::RefCell::new(BTreeMap::new()),
            spawn_outward: Box::new(checkrun::real_spawn_argv),
            outward_running: std::cell::RefCell::new(BTreeMap::new()),
            pruned: std::cell::RefCell::new(BTreeSet::new()),
            versions: std::cell::RefCell::new(BTreeMap::new()),
            found_sessions: std::cell::RefCell::new(std::collections::BTreeSet::new()),
            discover_looked: std::cell::RefCell::new(BTreeMap::new()),
            trust_pace: std::cell::RefCell::new(BTreeMap::new()),
            work: std::cell::RefCell::new(BTreeMap::new()),
            work_looked: std::cell::RefCell::new(BTreeMap::new()),
            screens: std::cell::RefCell::new(BTreeMap::new()),
            sent_ms: std::cell::RefCell::new(BTreeMap::new()),
            sending: std::cell::RefCell::new(BTreeMap::new()),
            blocked_since: std::cell::RefCell::new(BTreeMap::new()),
            unblocked_since: std::cell::RefCell::new(BTreeMap::new()),
            q_blocked_since: std::cell::RefCell::new(BTreeMap::new()),
            q_unblocked_since: std::cell::RefCell::new(BTreeMap::new()),
            idle_since: std::cell::RefCell::new(BTreeMap::new()),
            last_signal: std::cell::RefCell::new(BTreeMap::new()),
            liveness: std::cell::RefCell::new(BTreeMap::new()),
            proc_read: Box::new(crate::liveness_probe::real_proc_tree),
            work_area_read: Box::new(crate::liveness_diff::real_work_area),
            is_local: true,
            rate_pace: std::cell::RefCell::new(BTreeMap::new()),
            conn_pace: std::cell::RefCell::new(BTreeMap::new()),
            session_warned: std::cell::RefCell::new(std::collections::BTreeSet::new()),
            cached: BTreeMap::new(),
            seen: BTreeMap::new(),
            broken: BTreeMap::new(),
            failed: BTreeMap::new(),
            logged: BTreeSet::new(),
            hstate: std::cell::RefCell::new(Default::default()),
            hseen: std::cell::RefCell::new((0, 0)),
            hcfg: std::cell::RefCell::new(None),
            stage_cfg: std::cell::Cell::new(config::StageCfg::default()),
            design_cfg: std::cell::Cell::new(None),
            hlogged: std::cell::RefCell::new(BTreeSet::new()),
            hprompt: std::cell::RefCell::new(None),
            hprompt_is_question: std::cell::Cell::new(false),
            hprompt_base: std::cell::RefCell::new(None),
            hanswered: std::cell::RefCell::new(BTreeSet::new()),
            hpassive_sending: std::cell::RefCell::new(BTreeMap::new()),
            hpassive_done: std::cell::RefCell::new(BTreeSet::new()),
            hpassive_loaded: std::cell::Cell::new(false),
            hagents_synced: std::cell::Cell::new(false),
            exchange_cache: std::cell::RefCell::new(BTreeMap::new()),
            ask_watch: std::cell::RefCell::new(BTreeMap::new()),
            ask_tainted: std::cell::RefCell::new(BTreeMap::new()),
            design_watch: std::cell::RefCell::new(BTreeMap::new()),
            design_tainted: std::cell::RefCell::new(BTreeMap::new()),
            labels_ok: std::cell::Cell::new(true),
            sweep_hollywood_ticks: std::cell::Cell::new(3),
            hrate_pace: std::cell::RefCell::new(None),
            hconn_pace: std::cell::RefCell::new(None),
            herror: std::cell::RefCell::new(None),
            hprevious_until_ms: std::cell::Cell::new(None),
            hlast_handoff: std::cell::RefCell::new(None),
            // A test stage is conservative: born at 0, it never reads
            // a `cannot resume` close as older than itself — no
            // surprise `terminal.run` inside a unit test.
            started_ms: 0,
        }
    }
}

/// One studio's state read for `tick`. The caller's `log_once` and
/// the summary row above it already name the studio, so the error
/// starts with the reason — the panel's `error` line is short.
pub(crate) fn load_studio(dir: &Path) -> Result<StudioState, Fail> {
    store::load_state(dir).map_err(|e| match e {
        StoreError::Io(e) => Fail::refused(format!("{e}")),
        StoreError::Corrupt(c) => Fail::refused(format!("{c}")),
        _ => Fail::refused("unexpected store error"),
    })
}

/// The stage's state base, created when missing and resolved once —
/// studio dirs, role dirs, prompts and `BACKLOT_STATE_DIR` for roles
/// all see the real path from then on, matching what the OS reports
/// to terminals (`term.cwd`) and git (`worktree list`). A base that
/// cannot resolve keeps the path as given.
pub(crate) fn resolved_base(base: PathBuf) -> PathBuf {
    let _ = fs::create_dir_all(&base);
    base.canonicalize().unwrap_or(base)
}

/// `stage run`: connect, then tick on every hc event and once a
/// second on the ledger lengths.
/// Start the stage daemon loop (used by `backlot stage run`).
pub async fn run(common: &Common, env: &Env) -> Result<(), Fail> {
    run_impl(common, None, env, None, true).await
}

/// Start the daemon loop with an optional pre-resolved base directory and server
/// lease guard (used by `backlot server run`).
pub(crate) async fn run_with(
    common: &Common,
    base_override: PathBuf,
    env: &Env,
    lease: Option<&mut crate::server_lock::ServerLockGuard>,
) -> Result<(), Fail> {
    run_impl(common, Some(base_override), env, lease, false).await
}

async fn run_impl(
    common: &Common,
    base_override: Option<PathBuf>,
    env: &Env,
    mut lease: Option<&mut crate::server_lock::ServerLockGuard>,
    check_stage_guard: bool,
) -> Result<(), Fail> {
    let link = link::connect(
        env,
        common.session.as_deref().map(std::ffi::OsStr::new),
        common.remote.as_deref(),
    )
    .await?;
    if check_stage_guard && matches!(link.endpoint, hc_proto::endpoint::Endpoint::Local(_)) {
        crate::server_lock::refuse_if_server_or_retired(&env.vars, Some(&link.core_id))?;
    }
    let base = resolved_base(match base_override {
        Some(dir) => dir,
        None => match env.get("BACKLOT_STATE_DIR") {
            Some(d) => PathBuf::from(d),
            None => crate::server_lock::effective_base(&env.vars, &link.core_id),
        },
    });
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let path_env = match env.get("PATH") {
        Some(p) => format!("{}:{}", exe_dir.display(), p),
        None => exe_dir.to_string_lossy().into_owned(),
    };
    let git = crate::driver::shared_git();
    let mut stage = Stage {
        client: link.client.clone(),
        base: base.clone(),
        path_env,
        exe_dir: exe_dir.to_string_lossy().into_owned(),
        state_dir: base.to_string_lossy().into_owned(),
        env_vars: env.vars.clone(),
        exec: Box::new(discover::real_exec),
        driver: Box::new(crate::driver::LocalDriver::with_git(git.clone())),
        git,
        spawn_check: Box::new(checkrun::real_spawn),
        checking: std::cell::RefCell::new(BTreeMap::new()),
        spawn_outward: Box::new(checkrun::real_spawn_argv),
        outward_running: std::cell::RefCell::new(BTreeMap::new()),
        pruned: std::cell::RefCell::new(BTreeSet::new()),
        versions: std::cell::RefCell::new(BTreeMap::new()),
        found_sessions: std::cell::RefCell::new(std::collections::BTreeSet::new()),
        discover_looked: std::cell::RefCell::new(BTreeMap::new()),
        trust_pace: std::cell::RefCell::new(BTreeMap::new()),
        work: std::cell::RefCell::new(BTreeMap::new()),
        work_looked: std::cell::RefCell::new(BTreeMap::new()),
        screens: std::cell::RefCell::new(BTreeMap::new()),
        sent_ms: std::cell::RefCell::new(BTreeMap::new()),
        sending: std::cell::RefCell::new(BTreeMap::new()),
        blocked_since: std::cell::RefCell::new(BTreeMap::new()),
        unblocked_since: std::cell::RefCell::new(BTreeMap::new()),
        q_blocked_since: std::cell::RefCell::new(BTreeMap::new()),
        q_unblocked_since: std::cell::RefCell::new(BTreeMap::new()),
        idle_since: std::cell::RefCell::new(BTreeMap::new()),
        last_signal: std::cell::RefCell::new(BTreeMap::new()),
        liveness: std::cell::RefCell::new(BTreeMap::new()),
        proc_read: Box::new(crate::liveness_probe::real_proc_tree),
        work_area_read: Box::new(crate::liveness_diff::real_work_area),
        is_local: !matches!(link.endpoint, hc_proto::endpoint::Endpoint::Remote(_)),
        rate_pace: std::cell::RefCell::new(BTreeMap::new()),
        conn_pace: std::cell::RefCell::new(BTreeMap::new()),
        session_warned: std::cell::RefCell::new(std::collections::BTreeSet::new()),
        cached: BTreeMap::new(),
        seen: BTreeMap::new(),
        broken: BTreeMap::new(),
        failed: BTreeMap::new(),
        logged: BTreeSet::new(),
        hstate: std::cell::RefCell::new(Default::default()),
        hseen: std::cell::RefCell::new((0, 0)),
        hcfg: std::cell::RefCell::new(None),
        stage_cfg: std::cell::Cell::new(config::StageCfg::default()),
        design_cfg: std::cell::Cell::new(None),
        hlogged: std::cell::RefCell::new(BTreeSet::new()),
        hprompt: std::cell::RefCell::new(None),
        hprompt_is_question: std::cell::Cell::new(false),
        hprompt_base: std::cell::RefCell::new(None),
        hanswered: std::cell::RefCell::new(BTreeSet::new()),
        hpassive_sending: std::cell::RefCell::new(BTreeMap::new()),
        hpassive_done: std::cell::RefCell::new(BTreeSet::new()),
        hpassive_loaded: std::cell::Cell::new(false),
        hagents_synced: std::cell::Cell::new(false),
        exchange_cache: std::cell::RefCell::new(BTreeMap::new()),
        ask_watch: std::cell::RefCell::new(BTreeMap::new()),
        ask_tainted: std::cell::RefCell::new(BTreeMap::new()),
        design_watch: std::cell::RefCell::new(BTreeMap::new()),
        design_tainted: std::cell::RefCell::new(BTreeMap::new()),
        labels_ok: std::cell::Cell::new(true),
        sweep_hollywood_ticks: std::cell::Cell::new(3),
        hrate_pace: std::cell::RefCell::new(None),
        hconn_pace: std::cell::RefCell::new(None),
        herror: std::cell::RefCell::new(None),
        hprevious_until_ms: std::cell::Cell::new(None),
        hlast_handoff: std::cell::RefCell::new(None),
        started_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    };
    // A core without `events.subscribe` still works, polled on TICK.
    let mut sub = Subscription::connect(&link.endpoint, None, &SubscribeParams::default())
        .await
        .ok();
    let mut throttle = doc::Throttle::default();
    let mut catalog = crate::catalog_doc::Publisher::new(link.core_id.clone(), env);
    let mut doc_consumer = crate::doc_req::Consumer::load(&stage.base, crate::live::now_ms());
    let mut last_req_poll_ms = 0u64;
    let mut req_wake = true;
    let mut live = live_terminals(&link.client, true).await?;
    // The facts last written to `live.json`, so a tick writes only on a
    // change or the heartbeat (`live::due`) — never every second, which
    // would make the file's age meaningless.
    let mut live_written: Option<crate::live::Live> = None;
    crate::stagelog::stage_log!("live on core {}", link.core_id);
    loop {
        if let Some(guard) = lease.as_deref_mut() {
            guard.refresh_if_due();
        }
        step_daemon(
            &mut stage,
            &live,
            &mut throttle,
            &mut catalog,
            &mut doc_consumer,
            &mut live_written,
            &mut req_wake,
            &mut last_req_poll_ms,
        )
        .await?;
        live = match sub.as_mut() {
            Some(sub) => match tokio::time::timeout(TICK, sub.recv()).await {
                // A terminal or request-doc event, or the cadence: refetch the view.
                Ok(Ok(notif)) => {
                    if crate::doc_req::is_req_wake_event(&notif) {
                        req_wake = true;
                    }
                    refresh_inventory(&link.client, &mut stage, live).await?
                }
                Err(_) => refresh_inventory(&link.client, &mut stage, live).await?,
                Ok(Err(e)) => {
                    return Err(Fail::core(format!("lost the core event stream: {e}")));
                }
            },
            None => {
                tokio::time::sleep(TICK).await;
                refresh_inventory(&link.client, &mut stage, live).await?
            }
        };
    }
}

/// One full daemon cycle: drain `plugin.backlot.req` if woken or 60 s elapsed,
/// tick studio/Hollywood state machines, publish `backlot` and `backlot-cat`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn step_daemon(
    stage: &mut Stage,
    live: &Inventory,
    throttle: &mut doc::Throttle,
    catalog: &mut crate::catalog_doc::Publisher,
    doc_consumer: &mut crate::doc_req::Consumer,
    live_written: &mut Option<crate::live::Live>,
    req_wake: &mut bool,
    last_req_poll_ms: &mut u64,
) -> Result<bool, Fail> {
    let now_ms = crate::live::now_ms();
    let should_poll = *req_wake
        || *last_req_poll_ms == 0
        || now_ms.saturating_sub(*last_req_poll_ms) >= crate::doc_req::POLL_INTERVAL_MS;
    let drained = if should_poll {
        *req_wake = false;
        *last_req_poll_ms = now_ms;
        match doc_consumer.process(stage, now_ms).await {
            Ok(d) => d,
            Err(e @ Fail::Core(_)) => return Err(e),
            Err(e) => {
                stage.log_once("doc_req", e.message());
                false
            }
        }
    } else {
        false
    };
    if drained {
        throttle.force_next();
    }

    stage.tick(live).await?;
    // The summary regenerates each tick; the throttle writes it
    // only on change, at most once per gap. The same facts land in
    // `live.json` beside the ledgers, on a change and a heartbeat: a
    // role terminal carries `BACKLOT_STATE_DIR` and never connects
    // to the core, so its `backlot status` reads them from there,
    // and a reader drops a file older than `STALE_MS`.
    let summary = stage.summary_view(&live.doing);
    let now_ms = crate::live::now_ms();
    let live_facts = crate::live::from_summary(&summary, now_ms);
    if crate::live::due(&live_facts, live_written.as_ref(), now_ms) == crate::live::Write::Now {
        match crate::live::write(&stage.base, &live_facts) {
            Ok(()) => *live_written = Some(live_facts),
            Err(e) => stage.log_once("live", &format!("live facts: {e}")),
        }
    }
    throttle.note(summary);
    // A non-core publish failure leaves the summary to the next
    // tick, logged once; a dead wire exits.
    match doc::publish(&stage.client, throttle, now_ms).await {
        Ok(true) => crate::stagelog::stage_log!("summary written"),
        Ok(false) => {}
        Err(e @ Fail::Core(_)) => return Err(e),
        Err(e) => stage.log_once("core", e.message()),
    }
    match catalog
        .publish_if_changed(&stage.client, stage, now_ms)
        .await
    {
        Ok(true) => crate::stagelog::stage_log!("catalog written"),
        Ok(false) => {}
        Err(e @ Fail::Core(_)) => return Err(e),
        Err(e) => stage.log_once("catalog", e.message()),
    }
    Ok(drained)
}
