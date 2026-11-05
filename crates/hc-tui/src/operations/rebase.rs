//! Reconcile published members and stable adoption identities before mutation.
use super::*;
use crate::structure::Tree;

pub(super) fn canonical(document: &Document, tab: &str, space: &str) -> (String, String) {
    let mut tab = tab.to_owned();
    let mut space = space.to_owned();
    for _ in 0..16 {
        let mut changed = false;
        for (old, adoption) in &document.adoptions {
            if old == &space {
                let local = super::adoption::space_id(&adoption.space);
                space = tab_key(adoption.machine.as_deref(), &local);
                if let Some(raw) = adoption.tabs.get(&tab) {
                    tab = tab_key(
                        adoption.machine.as_deref(),
                        &super::adoption::tab_id(&local, raw),
                    );
                }
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    (tab, space)
}

pub(super) fn merge(
    edit: &Edit,
    document: &Document,
    stored: Option<&Tree>,
) -> Result<Tree, String> {
    // Stored roots already crossed CAS: hide prunes a closed leaf in the same
    // write as its tombstone. A stale inventory cannot delete a replacement
    // that a newer writer has already stored. Only fallback additions need
    // the creation-time cutoff; older inventories cannot resurrect a close.
    let keep = |id: &str| {
        document.closed.get(id).is_none_or(|closed| {
            edit.incarnations
                .get(id)
                .is_some_and(|created| created > closed)
        })
    };
    let fallback = edit.fallback.retain(&keep);
    let current = stored.cloned();
    let Some(mut root) = current.or_else(|| fallback.clone()) else {
        return Err("Pane no longer exists".into());
    };
    let present = root.terminals();
    let missing = fallback
        .map(|r| r.terminals())
        .unwrap_or_default()
        .into_iter()
        .filter(|id| !present.contains(id))
        .collect::<Vec<_>>();
    if let Some(added) = Tree::balanced(&missing, false) {
        root = Tree::split(root, added, false, 0.5);
    }
    if !root.valid() {
        root = Tree::balanced(&root.terminals(), false).ok_or("Invalid layout")?;
    }
    Ok(root)
}

pub(super) fn same_topology(a: &Tree, b: &Tree) -> bool {
    match (a, b) {
        (Tree::Pane { terminal: a }, Tree::Pane { terminal: b }) => a == b,
        (
            Tree::Split {
                vertical: a,
                first: af,
                second: as_,
                ..
            },
            Tree::Split {
                vertical: b,
                first: bf,
                second: bs,
                ..
            },
        ) => a == b && same_topology(af, bf) && same_topology(as_, bs),
        _ => false,
    }
}
