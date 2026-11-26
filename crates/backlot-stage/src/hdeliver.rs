//! Hollywood's inbox: the stage's guarded send into `bl-hollywood`
//! (D20). Owed notices land as `message.sent` records whose body
//! files sit under `messages/`; `tell hollywood` is already there
//! as the user's `message.sent`. A queued notice whose source
//! moved on records `message.dropped`; a sent batch records
//! `message.delivered` per seq. The gate is the studios' own:
//! live role, `waiting` sample, stable screen, up and gapped.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::cli::Fail;
use crate::daemon::Stage;
use crate::deliver::{Gate, InFlight, SEND_DEADLINE, gate_open, send_outcome};
use crate::hdeliver_fmt::{
    NOTICE_DEFER_MS, load_passive, notice_payload, person_payload, save_passive,
};
use crate::hnotice;
use crate::hstage::hkey;
use crate::reconcile::LiveTerminal;
use crate::sendfail;
use crate::store::{self, HPending, PendingFile};
use backlot_model::EventBy;
use backlot_model::hollywood::{HMessage, HollywoodKind, NewHollywoodEvent};
use backlot_model::state::StudioState;

/// The stage's errors on each studio — `broken` first, then the
/// last failed pass.
fn errors(stage: &Stage) -> BTreeMap<String, String> {
    let mut out = stage.broken.clone();
    out.extend(stage.failed.clone());
    out
}

impl Stage {
    /// Queue owed notices, drop moved sources, send what the gate
    /// allows — the studios' deliver pass on Hollywood's ledger.
    pub(crate) async fn hdeliver(
        &self,
        hdir: &Path,
        terminals: &[LiveTerminal],
        now_ms: u64,
    ) -> Result<(), Fail> {
        let pair = hkey();
        let studios: BTreeMap<String, StudioState> = self
            .cached
            .iter()
            .map(|(n, s)| (n.clone(), s.clone()))
            .collect();
        let errs = errors(self);

        // Owed notices land as `message.sent` with the body file in
        // the same append — `source_seq` dedups across restarts, and
        // the scratch write keeps a crash from leaving a torn body.
        for n in hnotice::owed(&studios, &errs, &self.hstate.borrow(), now_ms) {
            std::fs::create_dir_all(hdir.join("messages"))
                .map_err(|e| Fail::refused(format!("notice file: {e}")))?;
            let scratch = hdir.join("messages").join(format!(".new-{}", n.label));
            std::fs::write(&scratch, &n.body)
                .map_err(|e| Fail::refused(format!("notice file: {e}")))?;
            let label = n.label;
            let studio = n.studio.clone();
            let token = n.token.clone();
            let source = n.source_seq;
            let event = store::append_hollywood(hdir, move |_, seq| {
                Ok::<HPending, std::convert::Infallible>(HPending {
                    event: NewHollywoodEvent::new(
                        EventBy::role("stage"),
                        HollywoodKind::MessageSent {
                            to: "hollywood".into(),
                            studio,
                            text: token,
                            file: Some(format!("messages/{seq}-{label}.md")),
                            source_seq: source,
                        },
                    ),
                    files: vec![PendingFile {
                        src: scratch,
                        dest: format!("messages/{seq}-{label}.md"),
                        append: None,
                    }],
                })
            })
            .map_err(|e| match e {
                store::StoreError::Io(e) => Fail::refused(format!("notice: {e}")),
                store::StoreError::Corrupt(c) => Fail::refused(format!("{c}")),
                store::StoreError::Refused(r) => Fail::refused(format!("{r}")),
                store::StoreError::Build(_) => unreachable!("the build closure cannot fail"),
            })?;
            crate::stagelog::stage_log!("hollywood notice {} ({})", event.seq, label);
        }

        // A queued notice whose source no longer stands is dropped
        // — never pending again, still counted as told.
        let in_flight: BTreeSet<u64> = self
            .sending
            .borrow()
            .iter()
            .filter(|(k, _)| k.0 == pair.0)
            .flat_map(|(_, f)| f.seqs.iter().copied())
            .collect();
        self.hrefresh(hdir);
        let drops: Vec<(u64, String)> = {
            let hs = self.hstate.borrow();
            hs.messages
                .iter()
                .filter(|m| m.to == "hollywood" && !m.delivered && !m.dropped && !m.seen)
                .filter(|m| !in_flight.contains(&m.seq))
                .filter_map(|m| hnotice::stale_reason(m, &studios, &errs, &hs).map(|r| (m.seq, r)))
                .collect()
        };
        for (seq, reason) in drops {
            self.happend(
                hdir,
                HollywoodKind::MessageDropped {
                    target: seq,
                    reason,
                },
            )?;
        }

        // Finished sends collect their `message.delivered` and passive-context marks.
        let done = crate::deliver::take_finished(&mut self.sending.borrow_mut(), &pair.0);
        self.hrefresh(hdir);
        for (p, f) in done {
            let pending_passive = self
                .hpassive_sending
                .borrow_mut()
                .remove(&p)
                .unwrap_or_default();
            self.sent_ms.borrow_mut().insert(p.clone(), now_ms);
            match f.handle.await {
                Ok(Ok(v)) => match send_outcome(&v) {
                    Ok(()) => {
                        for seq in &f.seqs {
                            let _ = self
                                .happend(hdir, HollywoodKind::MessageDelivered { target: *seq });
                        }
                        if !pending_passive.is_empty() {
                            let mut done_set = self.hpassive_done.borrow_mut();
                            done_set.extend(pending_passive);
                            save_passive(hdir, &done_set);
                        }
                        crate::stagelog::stage_log!("delivered {:?} to hollywood", f.seqs);
                    }
                    Err(reason) => eprintln!(
                        "stage: send to hollywood not sent{}, still queued",
                        reason.map(|r| format!(" ({r})")).unwrap_or_default()
                    ),
                },
                Ok(Err(e)) if sendfail::retryable(&e) => {
                    let refusal = sendfail::refusal(&e);
                    eprintln!(
                        "stage: send to hollywood not delivered ({}), still queued",
                        refusal.text()
                    );
                }
                Ok(Err(e)) => return Err(Fail::call(e)),
                Err(e) => crate::stagelog::stage_log!("send to hollywood lost: {e}"),
            }
        }

        // Load persisted passive-context accounted marks once per stage process.
        if !self.hpassive_loaded.get() {
            load_passive(hdir, &mut self.hpassive_done.borrow_mut());
            self.hpassive_loaded.set(true);
        }

        // Two-lane outbox: Person (`from == "user"`) delivers inline first;
        // Stage notices defer until newest is >= NOTICE_DEFER_MS old and
        // run only when the Person lane is empty.
        self.hrefresh(hdir);
        let hs = self.hstate.borrow();
        let mut user_pending: Vec<&HMessage> = hs
            .messages
            .iter()
            .filter(|m| {
                m.to == "hollywood" && m.from == "user" && !m.delivered && !m.dropped && !m.seen
            })
            .collect();
        let mut notice_pending: Vec<&HMessage> = hs
            .messages
            .iter()
            .filter(|m| {
                m.to == "hollywood" && m.from != "user" && !m.delivered && !m.dropped && !m.seen
            })
            .collect();
        if user_pending.is_empty() && notice_pending.is_empty() {
            return Ok(());
        }
        user_pending.sort_by_key(|m| m.seq);
        notice_pending.sort_by_key(|m| (hnotice::rank(m), m.seq));
        let role = &hs.role;
        let active = crate::handoff_term::active_hollywood_terminal(role, terminals);
        let blocked = crate::handoff::handoff_blocks_delivery(hdir)
            || self
                .herror
                .borrow()
                .as_ref()
                .is_some_and(crate::handoff::HBlockedKind::blocks_delivery);
        let gate = Gate {
            live: role.live && active.is_some() && !blocked,
            work: self.work.borrow().get(&pair).copied(),
            screen: self.screens.borrow().get(&pair).cloned(),
            role_since_ms: role.since_ms,
            in_flight: self.sending.borrow().contains_key(&pair),
            sent_ms: self.sent_ms.borrow().get(&pair).copied(),
        };
        if !gate_open(&gate, now_ms) {
            return Ok(());
        }
        let passive = hnotice::passive_items(&studios, &hs, &self.hpassive_done.borrow());
        let (text, seqs) = if !user_pending.is_empty() {
            let text = person_payload(&user_pending, &notice_pending, &passive, hdir)?;
            let seqs = user_pending.iter().map(|m| m.seq).collect();
            (text, seqs)
        } else {
            let newest_ts = notice_pending.iter().map(|m| m.ts_ms).max().unwrap_or(0);
            if now_ms.saturating_sub(newest_ts) < NOTICE_DEFER_MS {
                return Ok(());
            }
            let text = notice_payload(&notice_pending, &passive, hdir)?;
            let seqs = notice_pending.iter().map(|m| m.seq).collect();
            (text, seqs)
        };
        let address = active.map(|t| t.address.clone()).unwrap_or_default();
        self.hpassive_sending
            .borrow_mut()
            .insert(pair.clone(), passive.into_iter().map(|p| p.key).collect());
        let send = self.driver.send_handle(&self.client);
        let handle = tokio::spawn(async move { send(address, text, SEND_DEADLINE).await });
        self.sending
            .borrow_mut()
            .insert(pair, InFlight { seqs, handle });
        Ok(())
    }

    /// One stage append on Hollywood's ledger.
    pub(crate) fn happend(&self, hdir: &Path, kind: HollywoodKind) -> Result<u64, Fail> {
        let event = store::append_hollywood(hdir, move |_, _| {
            Ok::<HPending, std::convert::Infallible>(HPending {
                event: NewHollywoodEvent::new(EventBy::role("stage"), kind),
                files: Vec::new(),
            })
        })
        .map_err(|e| match e {
            store::StoreError::Io(e) => Fail::refused(format!("hollywood append: {e}")),
            store::StoreError::Corrupt(c) => Fail::refused(format!("{c}")),
            store::StoreError::Refused(r) => Fail::refused(format!("{r}")),
            store::StoreError::Build(_) => unreachable!("the build closure cannot fail"),
        })?;
        Ok(event.seq)
    }
}
