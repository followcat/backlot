use super::*;
use hc_proto::{
    client::{CallError, Client},
    doc::{DocGetParams, DocSetParams},
    terminal::TerminalParams,
};

/// A fresh read on every attempt preserves opaque fields and unrelated
/// concurrent edits. A connection failure is never treated as a conflict.
pub(super) async fn update(
    client: &Client,
    apply: impl Fn(&mut Document) -> Result<(), String>,
) -> Result<Written, String> {
    for _ in 0..8 {
        let current = client
            .doc_get(DocGetParams {
                name: "hc-tui".into(),
                on: None,
            })
            .await
            .map_err(|e| e.to_string())?;
        let mut document = Document::decode(current.value)?;
        apply(&mut document)?;
        let value = serde_json::to_value(&document).map_err(|e| e.to_string())?;
        if serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
            return Err("Layout exceeds the core's document limit".into());
        }
        match client
            .doc_set(DocSetParams {
                name: "hc-tui".into(),
                on: None,
                value,
                version: current.version,
            })
            .await
        {
            Ok(result) => {
                return Ok(Written {
                    core: client.core().id.clone(),
                    document,
                    version: result.version,
                    created: None,
                });
            }
            Err(CallError::Rpc(error))
                if error.is_kind(hc_proto::jsonrpc::error_kind::CONFLICT) =>
            {
                continue;
            }
            Err(error) => {
                return Err(format!(
                    "Layout write failed: {error}; the write was not replayed"
                ));
            }
        }
    }
    Err("Layout kept changing; retry the action".into())
}

pub(super) async fn run(
    open: &OpenRpc,
    cached: &mut Option<Client>,
    request: Request,
) -> Result<Outcome, String> {
    if cached.as_ref().is_none_or(Client::is_closed) {
        *cached = Some(open().await.map_err(|e| e.to_string())?);
    }
    let client = cached.as_ref().expect("connected above");
    if request.core.is_empty() || client.core().id != request.core {
        return Err("Core identity changed; wait for the inventory to reconnect".into());
    }
    if matches!(&request.action, Action::Create(_) | Action::Close { .. }) {
        if !client.capabilities().has_method("doc.set") {
            return Err("Core does not offer doc.set".into());
        }
        let current = client
            .doc_get(DocGetParams {
                name: "hc-tui".into(),
                on: None,
            })
            .await
            .map_err(|e| e.to_string())?;
        let document = Document::decode(current.value)?;
        if let Action::Create(creation) = &request.action {
            validate_creation(creation, &document)?;
        }
    }
    let machine_method = match &request.action {
        Action::Machine(MachineOp::Add { .. }) => "machine.add",
        Action::Machine(MachineOp::Connect { .. }) => "machine.connect",
        Action::Machine(MachineOp::Rename { .. }) => "machine.rename",
        Action::Machine(MachineOp::Remove { .. }) => "machine.rm",
        _ => "",
    };
    let required: &[&str] = match &request.action {
        Action::Create(creation) if creation.promotion.is_some() => &[
            "doc.get",
            "doc.set",
            "terminal.run",
            "terminal.label",
            "terminal.get",
            "terminal.keep",
        ],
        Action::Create(_) => &["doc.get", "doc.set", "terminal.run"],
        Action::Close { .. } => &["doc.get", "doc.set", "terminal.get", "terminal.kill"],
        Action::RenamePane { .. } => &["terminal.get", "terminal.label"],
        Action::Edit(_) | Action::RenameSpace { .. } | Action::Forget { .. } => {
            &["doc.get", "doc.set"]
        }
        Action::Machine(_) => &[machine_method],
        Action::SendLine { .. } => &["terminal.get", "terminal.send"],
        Action::Interrupt { .. } => &["terminal.get", "terminal.interrupt"],
        Action::Keep { .. } => &["terminal.get", "terminal.keep"],
        Action::Park { .. } => &["terminal.get", "terminal.label", "terminal.keep"],
        Action::Kill { .. } => &["terminal.get", "terminal.kill"],
        Action::Exec { .. } => &["machine.exec"],
        Action::RunPopup { .. } => &["terminal.run"],
        // `terminal.put` exists on every core hc-tui runs on; the pieces
        // path is chosen further in, so a core without `put_chunks`
        // still gets the whole-file upload rather than a refusal.
        Action::PutImage { .. } => &["terminal.get"],
        Action::PeekKeep { .. } => &[
            "terminal.get",
            "terminal.label",
            "doc.get",
            "doc.set",
            "terminal.keep",
        ],
    };
    for method in required {
        if let Some(reason) = client.explain_unavailable(method) {
            return Err(reason);
        }
    }
    match request.action {
        Action::Edit(edit) => update(client, |document| edit.apply(document))
            .await
            .map(|w| Outcome::Written(Box::new(w))),
        Action::Create(mut creation) => {
            super::adoption::prepare(client, &mut creation).await?;
            let current = client
                .doc_get(DocGetParams {
                    name: "hc-tui".into(),
                    on: None,
                })
                .await
                .map_err(|e| e.to_string())?;
            validate_creation(&creation, &Document::decode(current.value)?)?;
            let terminal = client
                .terminal_run(creation.params)
                .await
                .map_err(|e| format!("Creation failed: {e}; the request was not replayed"))?
                .terminal;
            if let Some(proposal) = &mut creation.space_adoption
                && !proposal.custom_label
                && proposal.name.is_empty()
            {
                let name = crate::app::creation::suggested_name(&terminal.cwd);
                proposal.name = if name.is_empty() {
                    "workspace".to_owned()
                } else {
                    name
                };
            }
            let address = terminal.address.clone();
            let mut edit = creation.edit;
            match &mut edit.change {
                Change::Split { terminal: id, .. } | Change::NewTab { terminal: id, .. } => {
                    *id = terminal.address
                }
                _ => return Err("Invalid creation placement".into()),
            }
            update(client, |document| {
                edit.apply(document)?;
                if let Some(proposal) = &creation.space_adoption {
                    document
                        .adoptions
                        .entry(edit.space.clone())
                        .or_insert_with(|| proposal.clone());
                }
                if matches!(&edit.change, Change::NewTab { .. }) {
                    let order = document
                        .order
                        .entry(edit.space.clone())
                        .or_insert_with(|| creation.order.clone());
                    for id in &creation.order {
                        if !order.contains(id) {
                            order.push(id.clone());
                        }
                    }
                    let id = edit::local_key(&edit.tab)
                        .ok_or("Invalid tab identity")?
                        .to_owned();
                    if !order.contains(&id) {
                        order.push(id);
                    }
                }
                Ok(())
            })
            .await
            .map(|mut written| {
                written.created = creation.report_created.then_some(address);
                Outcome::Written(Box::new(written))
            })
        }
        Action::Forget {
            terminal,
            created_ms,
        } => {
            // A `pane` command's overlay terminal exited: drop it from the
            // layout — the split collapses and the saved focus returns,
            // like the reference's `restore_overlay_after_exit`.
            update(client, |document| {
                edit::hide(document, &terminal, created_ms);
                Ok(())
            })
            .await
            .map(|w| Outcome::Written(Box::new(w)))
        }
        Action::Close {
            terminals,
            memberships,
            fallback,
            next,
        } => {
            let mut last = None;
            // A confirmed group close can partially succeed. Persist each
            // successful close before proceeding, and surface any later error.
            for (terminal, created_ms) in terminals {
                let current = client
                    .terminal_get(TerminalParams {
                        terminal: terminal.clone(),
                        caller: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                if current.created_ms != created_ms {
                    return Err("Close target was replaced; reopen the confirmation".into());
                }
                let expected = memberships
                    .get(&terminal)
                    .ok_or("Missing close membership")?;
                let machine = terminal.split_once('/').map(|(m, _)| m);
                let foreign = client
                    .doc_get(DocGetParams {
                        name: "herdr-app".into(),
                        on: machine.map(str::to_owned),
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                let spaces = crate::structure::read(&[&current], &foreign.value, machine);
                if !spaces.iter().any(|space| {
                    space.id == expected.0
                        && space.tabs.iter().any(|tab| {
                            tab.id == expected.1 && tab.root.terminals().contains(&terminal)
                        })
                }) {
                    return Err("Close target moved; reopen the confirmation".into());
                }
                match client
                    .terminal_kill(TerminalParams {
                        terminal: terminal.clone(),
                        caller: None,
                    })
                    .await
                {
                    Ok(_) => {}
                    Err(error) if error.is_not_found() => {}
                    Err(error) => return Err(error.to_string()),
                }
                rm_after_kill(client, &terminal).await?;
                last = Some(
                    update(client, |document| {
                        for (key, tab) in &fallback {
                            document
                                .tabs
                                .entry(key.clone())
                                .or_insert_with(|| tab.clone());
                        }
                        edit::hide(document, &terminal, created_ms);
                        if document.focus.is_none() {
                            document.focus = next.clone();
                        }
                        Ok(())
                    })
                    .await?,
                );
            }
            Ok(last.map_or(Outcome::Done, |w| Outcome::Written(Box::new(w))))
        }
        action => rpc_actions::run_action(client, action).await,
    }
}

/// The core's tombstone reply for a terminal it already removed.
pub(super) fn gone(error: &str) -> bool {
    error.contains("not_found") || error.contains("removed")
}

/// Best-effort `terminal.rm` after an explicit close/kill — removes the
/// finished record when the core offers the method. `not_found`,
/// `live` and tombstone races are clean; anything else propagates.
pub(super) async fn rm_after_kill(client: &Client, terminal: &str) -> Result<(), String> {
    if client
        .explain_unavailable(hc_proto::method::TERMINAL_RM)
        .is_some()
    {
        return Ok(());
    }
    match client
        .terminal_rm(TerminalParams {
            terminal: terminal.to_owned(),
            caller: None,
        })
        .await
    {
        Ok(_) => Ok(()),
        Err(error) if error.is_not_found() || error.is_kind("live") || gone(&error.to_string()) => {
            Ok(())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn validate_creation(creation: &Creation, document: &Document) -> Result<(), String> {
    let mut probe = document.clone();
    let mut edit = creation.edit.clone();
    match &mut edit.change {
        Change::Split { terminal, .. } => *terminal = "pending-created-terminal".into(),
        Change::NewTab { terminal, .. } => {
            let (tab, _) = super::rebase::canonical(document, &edit.tab, &edit.space);
            if document.tabs.get(&tab).is_some_and(|t| t.root.is_some()) {
                return Err("New tab identity already exists".into());
            }
            *terminal = "pending-created-terminal".into();
        }
        _ => return Err("Invalid creation placement".into()),
    }
    edit.apply(&mut probe)
}
