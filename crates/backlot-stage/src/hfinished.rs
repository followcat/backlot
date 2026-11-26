//! Finished `bl-hollywood` tombstone sweep: `terminal.rm` for dead
//! overseer terminal records so restarts or crashes never accumulate
//! `finished` panes under the `bl-hollywood` tab.

use backlot_model::hollywood::{HOLLYWOOD, is_hollywood_terminal_name};
use hc_proto::client::CallError;

use crate::act::fatal;
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::reconcile::LiveTerminal;

/// Addresses of finished `bl-hollywood` records (named `bl-hollywood` or
/// carrying `backlot.role == "hollywood"`), excluding user shells and retaining
/// `hollywood-previous` tombstones until `backlot.previous_until_ms` expires.
pub(crate) fn finished_hollywood_at(finished: &[LiveTerminal], now_ms: u64) -> Vec<String> {
    finished
        .iter()
        .filter(|term| !term.labels.contains_key("backlot.shell"))
        .filter(|term| {
            let role_label = term.labels.get("backlot.role").map(String::as_str);
            if role_label == Some("hollywood-previous") {
                return term
                    .labels
                    .get("backlot.previous_until_ms")
                    .and_then(|s| s.parse::<u64>().ok())
                    .is_some_and(|until| now_ms >= until);
            }
            role_label == Some(HOLLYWOOD)
                || role_label == Some("hollywood-handoff")
                || is_hollywood_terminal_name(&term.name)
        })
        .map(|term| term.address.clone())
        .collect()
}

#[cfg(test)]
pub(crate) fn finished_hollywood(finished: &[LiveTerminal]) -> Vec<String> {
    finished_hollywood_at(finished, u64::MAX)
}

impl Stage {
    /// Remove any finished `bl-hollywood` records via `terminal.rm`,
    /// keeping `hollywood-previous` tombstones for their 10-minute grace
    /// window and keeping `sweep_hollywood_ticks` armed when an RPC
    /// refusal races a recent kill.
    pub(crate) async fn sweep_hollywood_finished(
        &self,
        finished: &[LiveTerminal],
        now_ms: u64,
    ) -> Result<(), Fail> {
        let targets = finished_hollywood_at(finished, now_ms);
        let has_pending_previous = finished.iter().any(|t| {
            t.labels.get("backlot.role").map(String::as_str) == Some("hollywood-previous")
        });
        if !has_pending_previous {
            self.hprevious_until_ms.set(None);
        }
        if targets.is_empty() {
            if !has_pending_previous {
                self.sweep_hollywood_ticks
                    .set(self.sweep_hollywood_ticks.get().saturating_sub(1));
            }
            return Ok(());
        }
        if self
            .client
            .explain_unavailable(hc_proto::method::TERMINAL_RM)
            .is_some()
        {
            self.sweep_hollywood_ticks
                .set(self.sweep_hollywood_ticks.get().saturating_sub(1));
            return Ok(());
        }
        let mut raced = false;
        for address in targets {
            match self
                .client
                .call(
                    "terminal.rm",
                    Some(serde_json::json!({ "terminal": address })),
                )
                .await
            {
                Ok(_) => {
                    crate::stagelog::stage_log!("cleared finished hollywood terminal ({address})")
                }
                Err(CallError::Rpc(_)) => raced = true,
                Err(e) => return Err(fatal(e)),
            }
        }
        if !raced {
            self.sweep_hollywood_ticks
                .set(self.sweep_hollywood_ticks.get().saturating_sub(1));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn fin_term(name: &str, address: &str, labels: &[(&str, &str)]) -> LiveTerminal {
        LiveTerminal {
            name: name.to_string(),
            address: address.to_string(),
            labels: labels
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect::<BTreeMap<_, _>>(),
            ..Default::default()
        }
    }

    #[test]
    fn finished_hollywood_matches_named_and_labelled_and_ignores_shells() {
        let finished = vec![
            fin_term("bl-hollywood", "t10", &[]),
            fin_term("", "t11", &[("backlot.role", "hollywood")]),
            fin_term("bl-hollywood", "t12", &[("backlot.shell", "1")]),
            fin_term("bl-acme-producer", "t13", &[("backlot.role", "producer")]),
            fin_term("bl-hollywood-next", "t14", &[]),
        ];
        assert_eq!(finished_hollywood(&finished), vec!["t10", "t11", "t14"]);
    }
}
