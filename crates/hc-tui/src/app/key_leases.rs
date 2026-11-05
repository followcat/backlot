// Derived from herdr src/input/lease.rs (Apache-2.0).
use std::collections::HashMap;
use std::hash::Hash;

use super::input::TerminalKey;
use hc_vt::Key as KeyIdentity;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct InputLeaseKey<Source> {
    source: Source,
    identity: KeyIdentity,
}

impl<Source> InputLeaseKey<Source> {
    pub(crate) fn new(source: Source, key: &TerminalKey) -> Self {
        Self {
            source,
            identity: key.identity(),
        }
    }
}

/// A press that arrived as text (typed, or committed by an IME) rather than as
/// a native key record.
fn is_text_press(key: &TerminalKey) -> bool {
    key.text_commit.is_some() && key.physical_key_id().is_none()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ForwardedInputLease<Target> {
    pub(crate) target: Target,
    pub(crate) key: TerminalKey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ConsumedInputLease<Context> {
    ReprocessRepeats(Context),
    SuppressRepeats,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InputLease<Context, Target> {
    Forwarded(ForwardedInputLease<Target>),
    Consumed(ConsumedInputLease<Context>),
}

pub(crate) enum RepeatPlan<Context, Target> {
    Forwarded(Target),
    Reprocess {
        context: Context,
        repetitions: u16,
        tracked: bool,
    },
    Ignore,
}

pub(crate) struct InputLeaseTable<Source, Context, Target> {
    leases: HashMap<InputLeaseKey<Source>, InputLease<Context, Target>>,
}

impl<Source, Context, Target> Default for InputLeaseTable<Source, Context, Target> {
    fn default() -> Self {
        Self {
            leases: HashMap::new(),
        }
    }
}

impl<Source, Context, Target> InputLeaseTable<Source, Context, Target>
where
    Source: Copy + Eq + Hash,
    Context: Clone + Eq,
    Target: Clone + Eq,
{
    pub(crate) fn normalize_press(
        &mut self,
        lease_key: &InputLeaseKey<Source>,
        key: TerminalKey,
    ) -> TerminalKey {
        // A VT text press is always a new press: hosts send text repeats as
        // text, and a stale lease (release reported after Shift was let go)
        // must not turn the next press into a repeat routed to an old target.
        if key.input.action != hc_vt::Action::Press
            || (key.text_commit.is_some() && key.physical_key_id().is_none())
        {
            return key;
        }
        if key.has_physical_identity() && self.leases.contains_key(lease_key) {
            key.with_action(hc_vt::Action::Repeat)
        } else {
            self.leases.remove(lease_key);
            key
        }
    }

    pub(crate) fn complete_press(
        &mut self,
        lease_key: InputLeaseKey<Source>,
        key: &TerminalKey,
        initial_context: Option<&Context>,
        resulting_context: Option<&Context>,
        target: Option<Target>,
    ) -> RepeatPlan<Context, Target> {
        if key.text_commit.is_some() && !key.has_physical_identity() {
            return RepeatPlan::Ignore;
        }
        if let Some(target) = target {
            self.insert_forwarded(lease_key, target, key.clone());
            return RepeatPlan::Ignore;
        }
        if !self.leases.contains_key(&lease_key) {
            let disposition = match (initial_context, resulting_context) {
                (Some(initial), Some(resulting)) if initial == resulting => {
                    ConsumedInputLease::ReprocessRepeats(initial.clone())
                }
                _ => ConsumedInputLease::SuppressRepeats,
            };
            self.insert_consumed(lease_key, disposition);
        }
        match self.leases.get(&lease_key) {
            Some(InputLease::Consumed(ConsumedInputLease::ReprocessRepeats(context)))
                if key.repeat_count > 1 =>
            {
                RepeatPlan::Reprocess {
                    context: context.clone(),
                    repetitions: key.repeat_count - 1,
                    tracked: true,
                }
            }
            _ => RepeatPlan::Ignore,
        }
    }

    pub(crate) fn plan_repeat(
        &mut self,
        lease_key: InputLeaseKey<Source>,
        key: &TerminalKey,
        current_context: Option<&Context>,
    ) -> RepeatPlan<Context, Target> {
        match self.leases.get(&lease_key) {
            Some(InputLease::Forwarded(lease)) => {
                return RepeatPlan::Forwarded(lease.target.clone());
            }
            Some(InputLease::Consumed(ConsumedInputLease::ReprocessRepeats(context)))
                if current_context == Some(context) =>
            {
                return RepeatPlan::Reprocess {
                    context: context.clone(),
                    repetitions: key.repeat_count,
                    tracked: true,
                };
            }
            Some(InputLease::Consumed(ConsumedInputLease::ReprocessRepeats(_))) => {
                self.insert_consumed(lease_key, ConsumedInputLease::SuppressRepeats);
                return RepeatPlan::Ignore;
            }
            Some(InputLease::Consumed(ConsumedInputLease::SuppressRepeats)) => {
                return RepeatPlan::Ignore;
            }
            None => {}
        }
        match current_context {
            Some(context) => RepeatPlan::Reprocess {
                context: context.clone(),
                repetitions: key.repeat_count,
                tracked: false,
            },
            None => RepeatPlan::Ignore,
        }
    }

    pub(crate) fn reprocess_allowed(
        &mut self,
        lease_key: InputLeaseKey<Source>,
        expected_context: &Context,
        current_context: Option<&Context>,
        tracked: bool,
    ) -> bool {
        let allowed = current_context == Some(expected_context);
        if tracked && !allowed {
            self.insert_consumed(lease_key, ConsumedInputLease::SuppressRepeats);
        }
        allowed
    }

    /// The forwarded text press from `source` when it is the only key held.
    /// A text press ("?") names a character, not a key, so its release ("/"
    /// after Shift was let go) cannot be matched by identity. When it is the
    /// only tracked key the match is certain; with any other press held,
    /// forwarded or consumed by Herdr, nothing is taken.
    pub(crate) fn remove_sole_text_press(
        &mut self,
        source: Source,
    ) -> Option<InputLease<Context, Target>> {
        let mut held = self
            .leases
            .iter()
            .filter(|(lease_key, _)| lease_key.source == source);
        let (Some((lease_key, lease)), None) = (held.next(), held.next()) else {
            return None;
        };
        let is_text_press =
            matches!(lease, InputLease::Forwarded(lease) if is_text_press(&lease.key));
        let lease_key = *lease_key;
        is_text_press
            .then(|| self.leases.remove(&lease_key))
            .flatten()
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, key: &InputLeaseKey<Source>) -> bool {
        self.leases.contains_key(key)
    }

    pub(crate) fn insert_forwarded(
        &mut self,
        key: InputLeaseKey<Source>,
        target: Target,
        original: TerminalKey,
    ) {
        self.leases.insert(
            key,
            InputLease::Forwarded(ForwardedInputLease {
                target,
                key: original,
            }),
        );
    }

    pub(crate) fn insert_consumed(
        &mut self,
        key: InputLeaseKey<Source>,
        disposition: ConsumedInputLease<Context>,
    ) {
        self.leases.insert(key, InputLease::Consumed(disposition));
    }

    pub(crate) fn remove(
        &mut self,
        key: &InputLeaseKey<Source>,
    ) -> Option<InputLease<Context, Target>> {
        self.leases.remove(key)
    }

    pub(crate) fn remove_source(&mut self, source: Source) -> Vec<ForwardedInputLease<Target>> {
        let keys = self
            .leases
            .keys()
            .filter(|key| key.source == source)
            .copied()
            .collect::<Vec<_>>();
        self.remove_keys(keys)
    }

    #[cfg(test)]
    pub(crate) fn remove_target(&mut self, target: &Target) -> Vec<ForwardedInputLease<Target>> {
        let keys = self
            .leases
            .iter()
            .filter_map(|(key, lease)| match lease {
                InputLease::Forwarded(lease) if &lease.target == target => Some(*key),
                InputLease::Forwarded(_) | InputLease::Consumed(_) => None,
            })
            .collect::<Vec<_>>();
        self.remove_keys(keys)
    }

    fn remove_keys(
        &mut self,
        keys: impl IntoIterator<Item = InputLeaseKey<Source>>,
    ) -> Vec<ForwardedInputLease<Target>> {
        keys.into_iter()
            .filter_map(|key| match self.leases.remove(&key) {
                Some(InputLease::Forwarded(lease)) => Some(lease),
                Some(InputLease::Consumed(_)) | None => None,
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.leases.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.leases.len()
    }
    pub(crate) fn forwarded(&self) -> impl Iterator<Item = &ForwardedInputLease<Target>> {
        self.leases.values().filter_map(|lease| match lease {
            InputLease::Forwarded(lease) => Some(lease),
            InputLease::Consumed(_) => None,
        })
    }

    pub(crate) fn forwarded_for(
        &self,
        key: &InputLeaseKey<Source>,
    ) -> Option<&ForwardedInputLease<Target>> {
        match self.leases.get(key) {
            Some(InputLease::Forwarded(lease)) => Some(lease),
            _ => None,
        }
    }

    pub(crate) fn forwarded_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut ForwardedInputLease<Target>> {
        self.leases.values_mut().filter_map(|lease| match lease {
            InputLease::Forwarded(lease) => Some(lease),
            InputLease::Consumed(_) => None,
        })
    }
}

#[cfg(test)]
#[path = "key_leases_tests.rs"]
mod tests;
