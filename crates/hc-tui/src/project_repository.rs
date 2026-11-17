// Derived from herdr src/workspace.rs, src/server/client_shell.rs and
// herdr-core docs/repository-facts.md (Apache-2.0).
//! Project origin facts without probing paths or Git on the viewer machine.

use hc_proto::terminal::{Checkout, RepositoryFacts, Terminal};

use crate::chrome::snapshot::ClientShellCheckout;
use crate::sidebar::clean;

fn key(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| format!("{}:{part}", part.len()))
        .collect()
}

pub(crate) fn folder(term: &Terminal) -> &str {
    term.repository.as_ref().map_or(term.cwd.as_str(), |facts| {
        facts
            .checkout
            .as_ref()
            .map_or(facts.cwd.as_str(), |checkout| checkout.repo_root.as_str())
    })
}

pub(super) fn space_identity(term: &Terminal) -> Option<(String, String)> {
    let facts = term.repository.as_ref()?;
    let checkout = facts.checkout.as_ref()?;
    Some((
        format!(
            "checkout:{}",
            key(&[
                &facts.core_id,
                &checkout.repository_key,
                &checkout.checkout_key
            ])
        ),
        clean(
            facts
                .auto_label
                .as_deref()
                .unwrap_or(&checkout.checkout_name),
        ),
    ))
}

pub(super) fn counts(facts: &RepositoryFacts) -> Option<(usize, usize)> {
    if facts.state != "ready" {
        return None;
    }
    let counts = facts.ahead_behind.as_ref()?;
    Some((
        usize::try_from(counts.ahead).ok()?,
        usize::try_from(counts.behind).ok()?,
    ))
}

pub(super) fn checkout(facts: &RepositoryFacts) -> Option<ClientShellCheckout> {
    if facts.state != "ready" {
        return None;
    }
    let checkout = facts.checkout.as_ref()?;
    Some(ClientShellCheckout {
        repository_key: key(&[&facts.core_id, &checkout.repository_key]),
        checkout_key: checkout.checkout_key.clone(),
        repo_name: clean(&checkout.repo_name),
        checkout_name: clean(&checkout.checkout_name),
        repo_root: checkout.repo_root.clone(),
        is_linked_worktree: checkout.is_linked_worktree,
        is_bare: checkout.is_bare,
    })
}

/// Identity facts that `structure::read` groups by; branch, counts and
/// state are display-only and never reach the cached structure.
pub(crate) fn repository_equal(a: Option<&RepositoryFacts>, b: Option<&RepositoryFacts>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.core_id == b.core_id
                && a.cwd == b.cwd
                && a.auto_label == b.auto_label
                && checkout_equal(a.checkout.as_ref(), b.checkout.as_ref())
        }
        _ => false,
    }
}

fn checkout_equal(a: Option<&Checkout>, b: Option<&Checkout>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.repository_key == b.repository_key
                && a.checkout_key == b.checkout_key
                && a.checkout_name == b.checkout_name
                && a.repo_root == b.repo_root
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "project_repository_tests.rs"]
mod tests;
