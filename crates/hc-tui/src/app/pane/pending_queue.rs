//! Semantic input and viewer controls retain one event order under backpressure.
use super::lifecycle::{dispatch_id, record_dispatch};
use super::*;

impl Pane {
    fn queue_control(&mut self, out: Out) -> Result<(), Out> {
        let same_kind = |a: &Out, b: &Out| {
            matches!(
                (a, b),
                (Out::Facts(_), Out::Facts(_)) | (Out::Resize(..), Out::Resize(..))
            )
        };
        if let Some(Pending::Control(last)) = self.pending.back_mut()
            && same_kind(last, &out)
        {
            *last = out;
            return Ok(());
        }
        if self.pending.len() >= PENDING_MAX_ITEMS + 8 {
            return Err(out);
        }
        self.pending.push_back(Pending::Control(out));
        Ok(())
    }

    /// A fresh connection synchronizes its current viewer state before input
    /// can use restored modes. Live event controls append after earlier input.
    pub(super) fn stage_initial_controls(&mut self) {
        let mut held = std::mem::take(&mut self.pending);
        // Recovery state supersedes controls from the old transport. Human
        // input accepted after loss stays behind this fresh state snapshot.
        held.retain(|item| !matches!(item, Pending::Control(_)));
        self.push_controls();
        self.pending.extend(held);
    }

    pub(super) fn push_controls(&mut self) {
        let State::Live(live) = &mut self.state else {
            return;
        };
        if live.spec.role == Role::Observe {
            return;
        }
        let facts = live.desired_facts.take();
        let size = live.desired_size.take();
        let focus = live.desired_focus.take();
        let sent_facts = live.sent_facts.clone();
        let sent_size = live.sent_size.unwrap_or(live.core_grid);
        if let Some(facts) = facts {
            let previous = self
                .pending
                .iter()
                .rev()
                .find_map(|item| match item {
                    Pending::Control(Out::Facts(facts)) => Some(facts),
                    _ => None,
                })
                .or(sent_facts.as_ref());
            if previous != Some(&facts)
                && let Err(Out::Facts(facts)) = self.queue_control(Out::Facts(facts))
                && let State::Live(live) = &mut self.state
            {
                live.desired_facts = Some(facts);
            }
        }
        if let Some(size) = size {
            let previous = self
                .pending
                .iter()
                .rev()
                .find_map(|item| match item {
                    Pending::Control(Out::Resize(cols, rows)) => Some((*cols, *rows)),
                    _ => None,
                })
                .unwrap_or(sent_size);
            if previous != size
                && let Err(Out::Resize(cols, rows)) =
                    self.queue_control(Out::Resize(size.0, size.1))
                && let State::Live(live) = &mut self.state
            {
                live.desired_size = Some((cols, rows));
            }
        }
        if let Some(focus) = focus
            && self.queue_control(Out::Focus(focus)).is_err()
            && let State::Live(live) = &mut self.state
        {
            live.desired_focus = Some(focus);
        }
        self.flush_pending();
    }

    pub(super) fn flush_pending(&mut self) {
        let State::Live(live) = &mut self.state else {
            return;
        };
        if live.spec.role == Role::Observe {
            return;
        }
        while let Some(front) = self.pending.front() {
            if !matches!(front, Pending::Control(_)) && (live.await_paint || self.vt.is_none()) {
                break;
            }
            let item = self.pending.pop_front().expect("front exists");
            let cost = charge(&item);
            match item {
                Pending::Control(out) => match live.out.try_send(out.clone()) {
                    Ok(()) => match out {
                        Out::Facts(facts) => live.sent_facts = Some(facts),
                        Out::Resize(cols, rows) => {
                            live.sent_size = Some((cols, rows));
                            if !live.await_paint
                                && let Some(vt) = &mut self.vt
                                && let Err(error) = vt.resize(cols, rows)
                            {
                                live.vt_broken = Some(error.to_string());
                            }
                        }
                        _ => {}
                    },
                    Err(error) => {
                        self.pending
                            .push_front(Pending::Control(error.into_inner()));
                        break;
                    }
                },
                item => {
                    let vt = self.vt.as_ref().expect("fresh VT required above");
                    let encoded = input::encode_pending(vt, &item);
                    self.pending_bytes = self.pending_bytes.saturating_sub(cost);
                    match encoded {
                        Ok(bytes) if bytes.is_empty() => {
                            held::Held::forget_parked(&mut self.parked, &item, self.generation);
                            live.held.track(&item);
                        }
                        Ok(bytes) => match live.out.try_send(Out::Input(bytes)) {
                            Ok(()) => {
                                held::Held::forget_parked(&mut self.parked, &item, self.generation);
                                live.held.track(&item);
                                if let Some(id) = dispatch_id(&item) {
                                    record_dispatch(&mut self.dispatched, id, self.generation);
                                }
                            }
                            Err(error) => {
                                if let Out::Input(bytes) = error.into_inner() {
                                    let frozen = match item {
                                        Pending::Key(key) | Pending::Prepared(_, key) => {
                                            Pending::Prepared(bytes, key)
                                        }
                                        other => Pending::Encoded(bytes, dispatch_id(&other)),
                                    };
                                    self.pending_bytes += charge(&frozen);
                                    self.pending.push_front(frozen);
                                }
                                break;
                            }
                        },
                        Err(error) => live.vt_broken = Some(error),
                    }
                }
            }
        }
    }
}
