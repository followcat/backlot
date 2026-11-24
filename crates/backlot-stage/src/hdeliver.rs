//! Hollywood's inbox: the stage's guarded send into `bl-hollywood`
//! (D20). Owed notices land as `message.sent` records whose body
//! files sit under `messages/`; `tell hollywood` is already there
//! as the user's `message.sent`. A queued notice whose source
//! moved on records `message.dropped`; a sent batch records
//! `message.delivered` per seq. The gate is the studios' own:
//! live role, `waiting` sample, stable screen, up and gapped.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use backlot_model::EventBy;
use backlot_model::hollywood::{HMessage, HollywoodKind, NewHollywoodEvent};
use backlot_model::state::StudioState;
use serde_json::json;

use crate::cli::Fail;
use crate::daemon::Stage;
use crate::deliver::{Gate, InFlight, SEND_DEADLINE, gate_open, send_outcome};
use crate::hnotice;
use crate::hstage::hkey;
use crate::reconcile::LiveTerminal;
use crate::sendfail;
use crate::store::{self, HPending, PendingFile};

/// The digest file name — `messages/digest-<top seq>.md` like the
/// studios'.
fn digest_name(items: &[&HMessage]) -> String {
    let top = items.iter().map(|m| m.seq).max().unwrap_or(0);
    format!("messages/digest-{top}.md")
}

/// The digest body: sections by label, each notice's file inlined,
/// a person's `tell` quoted as its line.
fn digest_body(items: &[&HMessage], hdir: &Path) -> String {
    let mut out = String::from("# backlot digest\n\n");
    for m in items {
        let head = m
            .file
            .as_deref()
            .and_then(|_| hnotice::label(m))
            .map(|l| format!("## {l} (seq {})\n\n", m.seq))
            .unwrap_or_else(|| format!("## message from {} (seq {})\n\n", m.from, m.seq));
        out.push_str(&head);
        if let Some(f) = &m.file {
            let text = std::fs::read_to_string(hdir.join(f)).unwrap_or_default();
            out.push_str(text.trim_end());
            out.push_str("\n\n");
        } else if let Some(t) = &m.text {
            out.push_str(&format!("{t}\n\n"));
        }
    }
    out
}

/// The one line a single item sends.
fn item_line(m: &HMessage, hdir: &Path) -> String {
    match (&m.file, &m.text) {
        (Some(f), _) => format!(
            "[backlot] {} → hollywood: read {}",
            m.from,
            hdir.join(f).display()
        ),
        (None, Some(t)) => format!("[backlot] {} → hollywood: {t}", m.from),
        (None, None) => "[backlot] stage → hollywood: your attention is wanted".into(),
    }
}

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
        let drops: Vec<(u64, String)> = self
            .hstate
            .borrow()
            .messages
            .iter()
            .filter(|m| m.to == "hollywood" && !m.delivered && !m.dropped)
            .filter(|m| !in_flight.contains(&m.seq))
            .filter_map(|m| hnotice::stale_reason(m, &studios, &errs).map(|r| (m.seq, r)))
            .collect();
        for (seq, reason) in drops {
            self.happend(
                hdir,
                HollywoodKind::MessageDropped {
                    target: seq,
                    reason,
                },
            )?;
        }

        // Finished sends collect their `message.delivered` records.
        let done = crate::deliver::take_finished(&mut self.sending.borrow_mut(), &pair.0);
        self.hrefresh(hdir);
        for (p, f) in done {
            self.sent_ms.borrow_mut().insert(p.clone(), now_ms);
            match f.handle.await {
                Ok(Ok(v)) => match send_outcome(&v) {
                    Ok(()) => {
                        for seq in &f.seqs {
                            let _ = self
                                .happend(hdir, HollywoodKind::MessageDelivered { target: *seq });
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

        // The outbox: pending `to: hollywood` in (rank, seq) order.
        self.hrefresh(hdir);
        let hs = self.hstate.borrow();
        let mut pending: Vec<&HMessage> = hs
            .messages
            .iter()
            .filter(|m| m.to == "hollywood" && !m.delivered && !m.dropped)
            .collect();
        if pending.is_empty() {
            return Ok(());
        }
        pending.sort_by_key(|m| (hnotice::rank(m), m.seq));
        let role = &hs.role;
        let gate = Gate {
            live: role.live
                && terminals
                    .iter()
                    .any(|t| t.name == backlot_model::hollywood::HOLLYWOOD_TERMINAL),
            work: self.work.borrow().get(&pair).copied(),
            screen: self.screens.borrow().get(&pair).cloned(),
            role_since_ms: role.since_ms,
            in_flight: self.sending.borrow().contains_key(&pair),
            sent_ms: self.sent_ms.borrow().get(&pair).copied(),
        };
        if !gate_open(&gate, now_ms) {
            return Ok(());
        }
        let address = terminals
            .iter()
            .find(|t| t.name == backlot_model::hollywood::HOLLYWOOD_TERMINAL)
            .map(|t| t.address.clone())
            .unwrap_or_default();
        let (text, seqs) = match pending.as_slice() {
            [one] => (item_line(one, hdir), vec![one.seq]),
            many => {
                let file = digest_name(many);
                let path = hdir.join(&file);
                std::fs::write(&path, digest_body(many, hdir))
                    .map_err(|e| Fail::refused(format!("digest file: {e}")))?;
                (
                    format!(
                        "[backlot] stage → hollywood: read {} ({} items)",
                        path.display(),
                        many.len()
                    ),
                    many.iter().map(|m| m.seq).collect(),
                )
            }
        };
        let client = self.client.clone();
        let handle = tokio::spawn(async move {
            client
                .call_timeout(
                    "terminal.send",
                    Some(json!({ "terminal": address, "text": text })),
                    SEND_DEADLINE,
                )
                .await
        });
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
