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
) -> Result<Option<Written>, String> {
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
    let required: &[&str] = match &request.action {
        Action::Create(creation) if creation.promotion.is_some() => &[
            "doc.get",
            "doc.set",
            "terminal.run",
            "terminal.label",
            "terminal.get",
        ],
        Action::Create(_) => &["doc.get", "doc.set", "terminal.run"],
        Action::Close { .. } => &["doc.get", "doc.set", "terminal.get", "terminal.kill"],
        Action::RenamePane { .. } => &["terminal.get", "terminal.label"],
        Action::Edit(_) => &["doc.get", "doc.set"],
    };
    for method in required {
        if let Some(reason) = client.explain_unavailable(method) {
            return Err(reason);
        }
    }
    match request.action {
        Action::Edit(edit) => update(client, |document| edit.apply(document))
            .await
            .map(Some),
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
            let mut edit = creation.edit;
            match &mut edit.change {
                Change::Split { terminal: id, .. } | Change::NewTab { terminal: id, .. } => {
                    *id = terminal.address
                }
                _ => return Err("Invalid creation placement".into()),
            }
            update(client, |document| {
                edit.apply(document)?;
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
            .map(Some)
        }
        Action::Close {
            terminals,
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
            Ok(last)
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
            Ok(None)
        }
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
