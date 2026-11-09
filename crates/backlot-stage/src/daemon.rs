//! The stage loop: each tick reloads any ledger that grew, reconciles
//! desired roles against the live terminal inventory and carries the
//! actions out ([`crate::act`]). Inputs are the ledger file lengths
//! (polled about once a second) and hc events via `events.subscribe`;
//! a core without it is polled on the same cadence. The summary
//! document regenerates every tick and is written on change, at most
//! once per gap. Losing the core exits non-zero — recovery is S6's.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use backlot_model::state::StudioState;
use backlot_model::summary::{self, Summary};
use hc_proto::client::Client;
use hc_proto::events::SubscribeParams;
use hc_proto::subscribe::Subscription;
use hc_proto::terminal::ListParams;

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::discover;
use crate::doc;
use crate::link;
use crate::reconcile;
use crate::store::{self, StoreError};
use crate::watch;

/// Poll cadence: ledgers grow-check and, without `events.subscribe`,
/// the terminal inventory.
const TICK: Duration = Duration::from_secs(1);

/// The live terminal view the loop works on.
struct Inventory {
    /// Every live named terminal; per-studio views come from labels.
    terms: Vec<reconcile::LiveTerminal>,
    /// Address → what the program reports doing, for the summary.
    doing: BTreeMap<String, String>,
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
    base: PathBuf,
    /// `PATH` for role terminals: this exe's directory first.
    pub(crate) path_env: String,
    /// `BACKLOT_STATE_DIR` for role terminals.
    pub(crate) state_dir: String,
    /// The stage's own environment: adapters scrub inherited markers
    /// from it, and `HOME` locates runner session files.
    pub(crate) env_vars: BTreeMap<String, String>,
    /// How external commands run — `discover::real_exec` live, a fake
    /// in tests.
    pub(crate) exec: Box<discover::Exec>,
    /// `<runner> --version` answers, asked once per runner per run.
    pub(crate) versions: std::cell::RefCell<BTreeMap<String, Option<String>>>,
    /// Session ids the running pass recorded — `cached` cannot see
    /// them until the ledgers reload, so the watch adds them itself.
    pub(crate) found_sessions: std::cell::RefCell<std::collections::BTreeSet<String>>,
    /// Reduced state per studio, reloaded when its log grew.
    pub(crate) cached: BTreeMap<String, StudioState>,
    /// `events.jsonl` length and mtime at each state's last load.
    seen: BTreeMap<String, Seen>,
}

impl Stage {
    /// Reload changed ledgers, reconcile each studio and act. Each
    /// studio sees only the terminals its label owns — the name is
    /// never enough to stop, adopt or drop.
    async fn tick(&mut self, live: &Inventory) -> Result<(), Fail> {
        for name in store::studios(&self.base)
            .map_err(|e| Fail::refused(format!("cannot read {}: {e}", self.base.display())))?
        {
            let dir = store::studio_dir(&self.base, &name);
            let meta = fs::metadata(dir.join("events.jsonl"));
            let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            if self.seen.get(&name).map(|s| s.len) != Some(len) {
                self.cached.insert(name.clone(), load_studio(&dir, &name)?);
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
        let blocked = watch::discovery_blocked(&self.cached);
        self.found_sessions.borrow_mut().clear();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        for name in self.cached.keys().cloned().collect::<Vec<_>>() {
            let dir = store::studio_dir(&self.base, &name);
            let state = &self.cached[&name];
            let view = reconcile::studio_view(&name, &live.terms);
            for action in reconcile::reconcile(&name, state, &view) {
                self.act(&name, &dir, state, &action, &mut real_slot_free, &blocked)
                    .await?;
            }
            self.watch(&name, &dir, state, &live.terms, now_ms).await?;
        }
        Ok(())
    }

    /// The `backlot` document's view: cached states, most recently
    /// changed first (the summary keeps live studios and lets old
    /// finished ones drop off).
    fn summary_view(&self, doing: &BTreeMap<String, String>) -> Summary {
        let mut pairs: Vec<(&String, &StudioState)> = self.cached.iter().collect();
        pairs.sort_by_key(|(name, _)| {
            std::cmp::Reverse(self.seen.get(*name).map(|s| s.mtime_ms).unwrap_or(0))
        });
        let owned: Vec<(String, StudioState)> = pairs
            .into_iter()
            .map(|(n, s)| (n.clone(), s.clone()))
            .collect();
        summary::summary(&owned, doing, 0)
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

/// `stage run`: connect, then tick on every hc event and once a
/// second on the ledger lengths.
pub async fn run(common: &Common, env: &Env) -> Result<(), Fail> {
    let link = link::connect(
        env,
        common.session.as_deref().map(std::ffi::OsStr::new),
        common.remote.as_deref(),
    )
    .await?;
    let base = match env.get("BACKLOT_STATE_DIR") {
        Some(d) => PathBuf::from(d),
        None => store::base_dir(&env.vars, &link.core_id),
    };
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
        state_dir: base.to_string_lossy().into_owned(),
        env_vars: env.vars.clone(),
        exec: Box::new(discover::real_exec),
        versions: std::cell::RefCell::new(BTreeMap::new()),
        found_sessions: std::cell::RefCell::new(std::collections::BTreeSet::new()),
        cached: BTreeMap::new(),
        seen: BTreeMap::new(),
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
