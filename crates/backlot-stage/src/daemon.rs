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
use std::time::Duration;

use backlot_model::state::StudioState;
use backlot_model::summary::{self, Summary, Work};
use hc_proto::client::Client;
use hc_proto::events::SubscribeParams;
use hc_proto::subscribe::Subscription;
use hc_proto::terminal::ListParams;

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::deliver;
use crate::discover;
use crate::doc;
use crate::link;
use crate::reconcile;
use crate::store::{self, StoreError};
use crate::watch;
use crate::workareas;

/// Poll cadence: ledgers grow-check and, without `events.subscribe`,
/// the terminal inventory.
const TICK: Duration = Duration::from_secs(1);

/// The live terminal view the loop works on.
pub(crate) struct Inventory {
    /// Every live named terminal; per-studio views come from labels.
    pub(crate) terms: Vec<reconcile::LiveTerminal>,
    /// Address → what the program reports doing, for the summary.
    pub(crate) doing: BTreeMap<String, String>,
}

/// Every live terminal the core knows, kept for the per-studio
/// label views and with its reported status for the summary.
async fn live_terminals(client: &Client) -> Result<Inventory, Fail> {
    let list = client
        .terminal_list(ListParams::default())
        .await
        .map_err(|e| Fail::refused(format!("terminal.list failed: {e}")))?;
    let mut inv = Inventory {
        terms: Vec::new(),
        doing: BTreeMap::new(),
    };
    for t in list.terminals.into_iter().filter(|t| t.state == "live") {
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
        if let Some(name) = t.name {
            inv.terms.push(reconcile::LiveTerminal {
                name,
                address: t.address,
                argv: t.argv,
                cwd: t.cwd,
                labels: t.labels,
            });
        }
    }
    Ok(inv)
}

/// What one ledger file looked like when its state was loaded.
struct Seen {
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
    /// How git runs for work areas — `workareas::real_git` live, a
    /// fake in tests.
    pub(crate) git: std::cell::RefCell<Box<workareas::GitRun<'static>>>,
    /// `<runner> --version` answers, asked once per runner per run.
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
    /// Reduced state per studio, reloaded when its log grew.
    pub(crate) cached: BTreeMap<String, StudioState>,
    /// `events.jsonl` length and mtime at each state's last load.
    seen: BTreeMap<String, Seen>,
    /// Studio → the last load error while its ledger stays unreadable:
    /// the tick skips it and the summary shows it.
    broken: BTreeMap<String, String>,
    /// Every error line already printed — each distinct message logs
    /// once, whatever its studio.
    logged: BTreeSet<String>,
}

impl Stage {
    /// `eprintln!` each distinct line once — a studio stuck in a
    /// failing pass reports its error without spamming every tick.
    fn log_once(&mut self, studio: &str, msg: &str) {
        if self.logged.insert(format!("{studio}: {msg}")) {
            eprintln!("stage: {studio}: {msg}");
        }
    }

    /// Reload changed ledgers, reconcile each studio and act. Each
    /// studio sees only the terminals its label owns — the name is
    /// never enough to stop, adopt or drop. One studio's bad ledger
    /// or failing pass logs and skips it; the others carry on, and
    /// only a lost core (`Fail::Core`) ends the stage.
    pub(crate) async fn tick(&mut self, live: &Inventory) -> Result<(), Fail> {
        for name in store::studios(&self.base)
            .map_err(|e| Fail::refused(format!("cannot read {}: {e}", self.base.display())))?
        {
            let dir = store::studio_dir(&self.base, &name);
            let meta = fs::metadata(dir.join("events.jsonl"));
            let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            if self.seen.get(&name).map(|s| s.len) != Some(len) {
                match load_studio(&dir, &name) {
                    Ok(state) => {
                        self.cached.insert(name.clone(), state);
                        self.broken.remove(&name);
                    }
                    // A ledger that cannot be loaded marks the studio
                    // broken; it stays skipped until the file loads.
                    Err(e) => {
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
                Err(e) => self.log_once(&name, e.message()),
                Ok(()) => {}
            }
        }
        Ok(())
    }

    /// One studio's reconcile → act → watch → deliver for a tick, so
    /// any failure inside it is one studio's, never the stage's.
    async fn studio_pass(
        &self,
        name: &str,
        live: &Inventory,
        real_slot_free: &mut bool,
        blocked: &BTreeSet<(String, String)>,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let dir = store::studio_dir(&self.base, name);
        let state = &self.cached[name];
        let view = reconcile::studio_view(name, &live.terms);
        for action in reconcile::reconcile(name, state, &view, now_ms) {
            self.act(name, &dir, state, &action, real_slot_free, blocked)
                .await?;
        }
        self.watch(name, &dir, state, &live.terms, now_ms).await?;
        self.deliver(name, &dir, state, &live.terms, now_ms).await
    }

    /// The `backlot` document's view: cached states, most recently
    /// changed first (the summary keeps live studios and lets old
    /// finished ones drop off). A broken studio with no loaded state
    /// still gets a row — its `error` is all the panel can show.
    pub(crate) fn summary_view(&self, doing: &BTreeMap<String, String>) -> Summary {
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
        summary::summary(&owned, doing, &self.work.borrow(), &self.broken, 0)
    }
}

#[cfg(test)]
impl Stage {
    /// A stage for unit tests: the given client, every track empty.
    /// `sample_work`/`deliver` need no base or state dir.
    pub(crate) fn for_test(client: Client) -> Stage {
        Stage {
            client,
            base: PathBuf::new(),
            path_env: String::new(),
            exe_dir: String::new(),
            state_dir: String::new(),
            env_vars: BTreeMap::new(),
            exec: Box::new(discover::real_exec),
            git: std::cell::RefCell::new(Box::new(workareas::real_git)),
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
            cached: BTreeMap::new(),
            seen: BTreeMap::new(),
            broken: BTreeMap::new(),
            logged: BTreeSet::new(),
        }
    }
}

/// One studio's state read for `tick`, errors saying which studio.
fn load_studio(dir: &Path, name: &str) -> Result<StudioState, Fail> {
    store::load_state(dir).map_err(|e| match e {
        StoreError::Io(e) => Fail::refused(format!("studio '{name}': {e}")),
        StoreError::Corrupt(c) => Fail::refused(format!("studio '{name}': {c}")),
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
pub async fn run(common: &Common, env: &Env) -> Result<(), Fail> {
    let link = link::connect(
        env,
        common.session.as_deref().map(std::ffi::OsStr::new),
        common.remote.as_deref(),
    )
    .await?;
    let base = resolved_base(match env.get("BACKLOT_STATE_DIR") {
        Some(d) => PathBuf::from(d),
        None => store::base_dir(&env.vars, &link.core_id),
    });
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let path_env = match env.get("PATH") {
        Some(p) => format!("{}:{}", exe_dir.display(), p),
        None => exe_dir.to_string_lossy().into_owned(),
    };
    let mut stage = Stage {
        client: link.client.clone(),
        base: base.clone(),
        path_env,
        exe_dir: exe_dir.to_string_lossy().into_owned(),
        state_dir: base.to_string_lossy().into_owned(),
        env_vars: env.vars.clone(),
        exec: Box::new(discover::real_exec),
        git: std::cell::RefCell::new(Box::new(workareas::real_git)),
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
        cached: BTreeMap::new(),
        seen: BTreeMap::new(),
        broken: BTreeMap::new(),
        logged: BTreeSet::new(),
    };
    // A core without `events.subscribe` still works, polled on TICK.
    let mut sub = Subscription::connect(&link.endpoint, None, &SubscribeParams::default())
        .await
        .ok();
    let mut throttle = doc::Throttle::default();
    let mut live = live_terminals(&link.client).await?;
    println!("stage: live on core {}", link.core_id);
    loop {
        stage.tick(&live).await?;
        // The summary regenerates each tick; the throttle writes it
        // only on change, at most once per gap.
        throttle.note(stage.summary_view(&live.doing));
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        if doc::publish(&link.client, &mut throttle, now_ms).await? {
            eprintln!("stage: summary written");
        }
        live = match sub.as_mut() {
            Some(sub) => match tokio::time::timeout(TICK, sub.recv()).await {
                // A terminal event or the cadence: refetch the view.
                Ok(Ok(_)) | Err(_) => live_terminals(&link.client).await?,
                Ok(Err(e)) => {
                    return Err(Fail::refused(format!("lost the core event stream: {e}")));
                }
            },
            None => {
                tokio::time::sleep(TICK).await;
                live_terminals(&link.client).await?
            }
        };
    }
}
