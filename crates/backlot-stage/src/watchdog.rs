//! `backlot stage watch`: the external heartbeat the design's gap
//! leaves open — nothing else notices a dead stage, a lost role
//! terminal, or attention piling up while everyone looks away.
//! It polls the core's terminal list and the studio ledgers and
//! sends one `<watch>` line per condition into `--target`.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use hc_proto::terminal::{ListParams, Terminal};

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::link;
use crate::reconcile;
use crate::stage::STAGE_NAME;
use crate::store;

/// One poll round's picture: what the core and the ledgers show.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct View {
    /// The `backlot-stage` terminal is live.
    pub stage_live: bool,
    /// `bl-<studio>-…` terminal names currently live.
    pub role_terms: BTreeSet<String>,
    /// `bl-<studio>-<role>` names each ledger wants live.
    pub wanted: BTreeSet<String>,
    /// Open-attention count per studio.
    pub attention: BTreeMap<String, usize>,
}

/// The `<watch>` lines a round-to-round change yields — pure, so
/// the whole watch table tests without a core. Each condition is
/// edge-triggered: a dead stage says so once, a role that stays
/// lost is not re-reported, attention growing again is a new edge.
pub(crate) fn diff(prev: &View, now: &View) -> Vec<String> {
    let mut out = Vec::new();
    if prev.stage_live && !now.stage_live {
        out.push("stage terminal dead".to_string());
    }
    if !prev.stage_live && now.stage_live {
        out.push("stage terminal live".to_string());
    }
    for name in &prev.role_terms {
        if now.wanted.contains(name) && !now.role_terms.contains(name) {
            out.push(format!("role terminal lost: {name}"));
        }
    }
    for (studio, n) in &now.attention {
        if *n > prev.attention.get(studio).copied().unwrap_or(0) {
            out.push(format!("attention grew: {n} open in {studio}"));
        }
    }
    out
}

/// The view one poll round builds: the core's live terminals
/// against every studio ledger in the state dir. A `--studio`
/// argument narrows the ledgers to that one studio.
fn view(terms: &[Terminal], state_dir: &std::path::Path, only: Option<&str>) -> Result<View, Fail> {
    let mut v = View::default();
    for t in terms.iter().filter(|t| t.state == "live") {
        if t.name.as_deref() == Some(STAGE_NAME) {
            v.stage_live = true;
        }
        if let Some(name) = &t.name
            && name.starts_with("bl-")
        {
            v.role_terms.insert(name.clone());
        }
    }
    for studio in store::studios(state_dir).map_err(|e| Fail::refused(e.to_string()))? {
        if only.is_some_and(|s| s != studio) {
            continue;
        }
        let dir = store::studio_dir(state_dir, &studio);
        let Ok(state) = store::load_state(&dir) else {
            continue;
        };
        v.attention
            .insert(studio.clone(), state.open_attention().count());
        for (key, role) in &state.roles {
            if role.live
                && let Some(name) = reconcile::terminal_name(&studio, key)
            {
                v.wanted.insert(name);
            }
        }
    }
    Ok(v)
}

/// The poll loop: every round computes the view, diffs it against
/// the last, and sends each new `<watch>` line into `--target`.
/// With no target the lines go to stderr — the watch still
/// watches. A lost core ends the watch; anything else retries.
async fn run(common: &Common, env: &Env, target: Option<String>) -> Result<(), Fail> {
    let link = link::connect(
        env,
        common.session.as_deref().map(std::ffi::OsStr::new),
        common.remote.as_deref(),
    )
    .await?;
    let state_dir = link::state_dir(
        env,
        common.session.as_deref().map(std::ffi::OsStr::new),
        common.remote.as_deref(),
    )
    .await?;
    let only = env.get("BACKLOT_STUDIO").map(String::from);
    let mut prev = View {
        stage_live: true,
        ..View::default()
    };
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    loop {
        tick.tick().await;
        let terms = match link.client.terminal_list(ListParams::default()).await {
            Ok(l) => l.terminals,
            Err(e) => {
                eprintln!("stage watch: terminal.list failed: {e}");
                continue;
            }
        };
        let now = view(&terms, &state_dir, only.as_deref())?;
        for line in diff(&prev, &now) {
            let text = format!("<watch>{line}</watch>");
            eprintln!("stage watch: {text}");
            if let Some(t) = &target {
                let sent = link
                    .client
                    .call(
                        "terminal.send",
                        Some(serde_json::json!({"terminal": t, "text": text})),
                    )
                    .await;
                if let Err(e) = sent {
                    eprintln!("stage watch: send to {t} failed: {e}");
                }
            }
        }
        prev = now;
    }
}

/// `stage watch`: poll forever; the first round reports the stage
/// dead only if it is absent from the start.
pub fn watch(common: &Common, env: &Env, target: Option<String>) -> Result<String, Fail> {
    link::runtime()?.block_on(run(common, env, target))?;
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_report_once() {
        // The loop seeds `prev` stage-live so a dead first round
        // still reports.
        let mut prev = View {
            stage_live: true,
            ..View::default()
        };
        // Round one: the stage dies and t2's attention grows.
        let mut now = View {
            stage_live: false,
            role_terms: BTreeSet::from(["bl-shop-producer".into()]),
            wanted: BTreeSet::from(["bl-shop-producer".into(), "bl-shop-t1".into()]),
            attention: BTreeMap::from([("shop".into(), 2)]),
        };
        let got = diff(&prev, &now);
        assert_eq!(
            got,
            vec!["stage terminal dead", "attention grew: 2 open in shop"]
        );
        // Steady-state: nothing new.
        assert!(diff(&now, &now).is_empty());
        // The lost role reports once, when the ledger still wants it.
        prev = now.clone();
        now.role_terms.remove("bl-shop-producer");
        assert_eq!(
            diff(&prev, &now),
            vec!["role terminal lost: bl-shop-producer"]
        );
        // Recovery reports.
        prev = now.clone();
        now.stage_live = true;
        assert_eq!(diff(&prev, &now), vec!["stage terminal live"]);
    }
}
