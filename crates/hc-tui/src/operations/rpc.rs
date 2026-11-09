use super::*;
use hc_proto::{
    client::{CallError, Client},
    doc::{DocGetParams, DocSetParams},
    terminal::TerminalParams,
};
use serde_json::json;

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
    if cached.is_none() {
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
        Action::PutImage { .. } => &["terminal.get", "terminal.put"],
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
        Action::RenamePane {
            terminal,
            created_ms,
            name,
        } => {
            let current = client
                .terminal_get(TerminalParams {
                    terminal: terminal.clone(),
                    caller: None,
                })
                .await
                .map_err(|e| e.to_string())?;
            if current.created_ms != created_ms {
                return Err("Rename target was replaced; reopen the dialog".into());
            }
            client
                .call(
                    "terminal.label",
                    Some(json!({"terminal":terminal,"labels":{"herdr.title":name}})),
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Done)
        }
        Action::RenameSpace { key, proposal } => update(client, |document| {
            let chosen = document
                .adoptions
                .entry(key.clone())
                .or_insert_with(|| proposal.clone());
            chosen.name = proposal.name.clone();
            chosen.custom_label = proposal.custom_label;
            Ok(())
        })
        .await
        .map(|w| Outcome::Written(Box::new(w))),
        Action::Machine(op) => {
            let (method, params) = match op {
                MachineOp::Add { target } => ("machine.add", json!({"target": target})),
                MachineOp::Connect { machine } => ("machine.connect", json!({"machine": machine})),
                MachineOp::Rename { machine, new_name } => (
                    "machine.rename",
                    json!({"machine": machine, "new_name": new_name}),
                ),
                MachineOp::Remove { machine } => ("machine.rm", json!({"machine": machine})),
            };
            client
                .call(method, Some(params))
                .await
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Done)
        }
        Action::SendLine {
            terminal,
            created_ms,
            text,
        } => {
            verify_incarnation(client, &terminal, created_ms, "Send").await?;
            client
                .call(
                    "terminal.send",
                    Some(json!({"terminal":terminal,"text":text})),
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Done)
        }
        Action::Interrupt {
            terminal,
            created_ms,
        } => {
            verify_incarnation(client, &terminal, created_ms, "Interrupt").await?;
            client
                .call("terminal.interrupt", Some(json!({"terminal":terminal})))
                .await
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Done)
        }
        Action::Keep {
            terminal,
            created_ms,
        } => {
            verify_incarnation(client, &terminal, created_ms, "Keep").await?;
            client
                .call(
                    "terminal.keep",
                    Some(json!({"terminal":terminal,"keep":true})),
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Done)
        }
        Action::Park {
            terminal,
            created_ms,
        } => {
            verify_incarnation(client, &terminal, created_ms, "Park").await?;
            client
                .call(
                    "terminal.label",
                    Some(json!({"terminal":terminal,"labels":{"herdr.space":"","herdr.tab":""}})),
                )
                .await
                .map_err(|e| e.to_string())?;
            client
                .call(
                    "terminal.keep",
                    Some(json!({"terminal":terminal,"keep":false})),
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Done)
        }
        Action::Kill {
            terminal,
            created_ms,
            allow_gone,
        } => {
            match verify_incarnation(client, &terminal, created_ms, "Kill").await {
                Err(error) if allow_gone && gone(&error) => return Ok(Outcome::Done),
                result => result?,
            }
            client
                .terminal_kill(TerminalParams {
                    terminal,
                    caller: None,
                })
                .await
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Done)
        }
        Action::Exec { line, machine } => {
            client
                .call("machine.exec", Some(json!({"line": line, "on": machine})))
                .await
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Done)
        }
        Action::RunPopup { params } => {
            // `terminal.run` outcome is never replayed: like Create, a
            // transport failure leaves an unknown number of children.
            let terminal = client
                .terminal_run(params)
                .await
                .map_err(|e| format!("Popup failed: {e}; the request was not replayed"))?
                .terminal;
            Ok(Outcome::Ran(terminal.address))
        }
        Action::PeekKeep {
            mut edit,
            terminal,
            created_ms,
            labels,
            order,
        } => {
            verify_incarnation(client, &terminal, created_ms, "Keep").await?;
            client
                .call(
                    "terminal.label",
                    Some(json!({"terminal":terminal,"labels":labels})),
                )
                .await
                .map_err(|e| e.to_string())?;
            if let Change::NewTab { terminal: id, .. } = &mut edit.change {
                *id = terminal.clone();
            }
            let tab_local = edit::local_key(&edit.tab)
                .ok_or("Invalid keep tab identity")?
                .to_owned();
            let written = update(client, |document| {
                edit.apply(document)?;
                let order = document
                    .order
                    .entry(edit.space.clone())
                    .or_insert(order.clone());
                if !order.contains(&tab_local) {
                    order.push(tab_local.clone());
                }
                Ok(())
            })
            .await?;
            client
                .call(
                    "terminal.keep",
                    Some(json!({"terminal":terminal,"keep":true})),
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Written(Box::new(written)))
        }
        Action::PutImage {
            terminal,
            created_ms,
            name,
            data,
        } => {
            verify_incarnation(client, &terminal, created_ms, "Image paste").await?;
            let result = client
                .call(
                    "terminal.put",
                    Some(json!({"terminal":terminal,"name":name,"data":data})),
                )
                .await
                .map_err(|e| e.to_string())?;
            let path = result["path"]
                .as_str()
                .ok_or("terminal.put replied without a path")?
                .to_owned();
            Ok(Outcome::Put { terminal, path })
        }
    }
}

/// The incarnation check the reference performs for rename/close:
/// `terminal.get` must return the same `created_ms` the menu saw, so a
/// stale menu never acts on a replacement terminal.
async fn verify_incarnation(
    client: &Client,
    terminal: &str,
    created_ms: u64,
    what: &str,
) -> Result<(), String> {
    let current = client
        .terminal_get(TerminalParams {
            terminal: terminal.into(),
            caller: None,
        })
        .await
        .map_err(|e| e.to_string())?;
    if current.created_ms != created_ms {
        return Err(format!("{what} target was replaced; reopen the menu"));
    }
    Ok(())
}

/// The core's tombstone reply for a terminal it already removed.
fn gone(error: &str) -> bool {
    error.contains("not_found") || error.contains("removed")
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
