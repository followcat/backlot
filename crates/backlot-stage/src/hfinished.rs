//! Finished `bl-hollywood` tombstone sweep: `terminal.rm` for dead
//! overseer terminal records so restarts or crashes never accumulate
//! `finished` panes under the `bl-hollywood` tab.

use backlot_model::hollywood::{HOLLYWOOD, HOLLYWOOD_TERMINAL};
use hc_proto::client::CallError;

use crate::act::fatal;
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::reconcile::LiveTerminal;

/// Addresses of finished `bl-hollywood` records (named `bl-hollywood` or
/// carrying `backlot.role == "hollywood"`), excluding user shells.
pub(crate) fn finished_hollywood(finished: &[LiveTerminal]) -> Vec<String> {
    finished
        .iter()
        .filter(|term| !term.labels.contains_key("backlot.shell"))
        .filter(|term| {
            term.labels.get("backlot.role").map(String::as_str) == Some(HOLLYWOOD)
                || term.name == HOLLYWOOD_TERMINAL
        })
        .map(|term| term.address.clone())
        .collect()
}

impl Stage {
    /// Remove any finished `bl-hollywood` records via `terminal.rm`,
    /// keeping `sweep_hollywood_ticks` armed when an RPC refusal races
    /// a recent kill so the next tick retries cleanly.
    pub(crate) async fn sweep_hollywood_finished(
        &self,
        finished: &[LiveTerminal],
    ) -> Result<(), Fail> {
        let targets = finished_hollywood(finished);
        if targets.is_empty() {
            self.sweep_hollywood_ticks
                .set(self.sweep_hollywood_ticks.get().saturating_sub(1));
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
        ];
        assert_eq!(finished_hollywood(&finished), vec!["t10", "t11"]);
    }
}
