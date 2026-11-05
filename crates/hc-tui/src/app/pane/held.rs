// Derived from herdr src/server/clients.rs (Apache-2.0).
//! The source server retains delivered presses independently of client UI
//! leases. Byte-stream core has no such ledger, so graceful detach owns it.
use super::*;
use crate::app::input::TerminalKey;
use hc_vt::{Action, Key};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct Held(HashMap<Key, TerminalKey>);

impl Held {
    fn changed_code(item: &Pending) -> Option<Key> {
        let (Pending::Key(key) | Pending::Prepared(_, key)) = item else {
            return None;
        };
        match key.input.action {
            Action::Press if key.text_commit.is_none() || key.has_physical_identity() => {
                Some(key.input.key)
            }
            Action::Release => Some(key.input.key),
            _ => None,
        }
    }

    pub(super) fn forget_parked(parked: &mut [Parked], item: &Pending, keep: u64) {
        if let Some(code) = Self::changed_code(item) {
            for route in parked.iter_mut().filter(|route| route.route != keep) {
                route.held.0.remove(&code);
            }
        }
    }
    pub(super) fn track(&mut self, item: &Pending) {
        let (Pending::Key(key) | Pending::Prepared(_, key)) = item else {
            return;
        };
        match key.input.action {
            Action::Press if key.text_commit.is_none() || key.has_physical_identity() => {
                self.0.insert(key.input.key, key.clone());
            }
            Action::Release => {
                self.0.remove(&key.input.key);
            }
            _ => {}
        }
    }

    pub(super) fn release_into(
        &mut self,
        vt: &Vt,
        pending: &mut VecDeque<Out>,
        error: &mut Option<String>,
    ) {
        for (_, key) in self.0.drain() {
            let release = Pending::Key(key.with_action(Action::Release));
            match input::encode_pending(vt, &release) {
                Ok(bytes) if !bytes.is_empty() => pending.push_back(Out::Release(bytes)),
                Ok(_) => {}
                Err(why) => *error = Some(why),
            }
        }
    }
}

impl Pane {
    pub(crate) fn parked_routes(&self) -> Vec<u64> {
        self.parked.iter().map(|parked| parked.route).collect()
    }

    pub(super) fn forget_held_elsewhere(&mut self, item: &Pending, keep: u64) {
        if let Some(code) = Held::changed_code(item)
            && self.generation != keep
            && let State::Live(live) = &mut self.state
        {
            live.held.0.remove(&code);
        }
        Held::forget_parked(&mut self.parked, item, keep);
    }
    pub(crate) fn parked_held(&self, route: u64) -> bool {
        self.parked
            .iter()
            .any(|parked| parked.route == route && !parked.held.0.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hc_vt::{KeyInput, Mods};

    #[test]
    fn teardown_tracks_forwarded_codes_even_if_the_last_chord_encodes_nothing() {
        let mut held = Held::default();
        let mut vt = Vt::new(80, 24, 0);
        vt.feed(b"\x1b[=31u");
        let key = |ch, mods| {
            TerminalKey::new(KeyInput {
                key: Key::Char(ch),
                mods,
                action: Action::Press,
            })
        };
        held.track(&Pending::Key(key('b', Mods::CTRL)));
        held.track(&Pending::Key(key('c', Mods::CTRL)));
        held.track(&Pending::Key(key('c', Mods::META)));
        let mut output = VecDeque::new();
        let mut error = None;
        held.release_into(&vt, &mut output, &mut error);
        assert_eq!(error, None);
        let bytes: Vec<_> = output
            .into_iter()
            .filter_map(|out| match out {
                Out::Release(bytes) => Some(bytes),
                _ => None,
            })
            .collect();
        assert_eq!(bytes, [b"\x1b[98;5:3u".to_vec()]);
    }
}
