// Derived from herdr src/workspace.rs, src/server/client_shell.rs and
// herdr-core docs/repository-facts.md (Apache-2.0).
//! Project origin facts without probing paths or Git on the viewer machine.

use hc_proto::terminal::{RepositoryFacts, Terminal};

use crate::chrome::snapshot::ClientShellWorktree;
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

pub(super) fn worktree(facts: &RepositoryFacts) -> Option<ClientShellWorktree> {
    if facts.state != "ready" {
        return None;
    }
    let checkout = facts.checkout.as_ref()?;
    Some(ClientShellWorktree {
        key: key(&[&facts.core_id, &checkout.repository_key]),
        label: clean(&checkout.repo_name),
        is_linked_worktree: checkout.is_linked_worktree,
    })
}

#[cfg(test)]
#[path = "project_repository_tests.rs"]
mod tests;
