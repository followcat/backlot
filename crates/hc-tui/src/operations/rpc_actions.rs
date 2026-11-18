//! Secondary actions `rpc::run` dispatches after its capability gate:
//! everything that is not a shared-layout read or the close group.
use super::rpc::{gone, rm_after_kill, update};
use super::*;
use hc_proto::{client::Client, terminal::TerminalParams};
use serde_json::json;

pub(super) async fn run_action(client: &Client, action: Action) -> Result<Outcome, String> {
    match action {
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
                .map_err(send_failure)?;
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
            match client
                .terminal_kill(TerminalParams {
                    terminal: terminal.clone(),
                    caller: None,
                })
                .await
            {
                Ok(_) => {}
                Err(error) if allow_gone && error.is_not_found() => return Ok(Outcome::Done),
                Err(error) => return Err(error.to_string()),
            }
            rm_after_kill(client, &terminal).await?;
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
            Ok(Outcome::Ran(terminal.address, terminal.created_ms))
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
            bytes,
            pieces,
        } => {
            let capabilities = client.capabilities();
            let pieces_ok = pieces
                && capabilities.has_feature(hc_proto::hello::feature::PUT_CHUNKS)
                && capabilities.has_method("terminal.put_begin")
                && capabilities.has_method("terminal.put_chunk")
                && capabilities.has_method("terminal.put_finish");
            // Refuse before any call when neither path can run: a core
            // that offers neither the pieces nor `terminal.put` gets no
            // probe, exactly like the old static gate.
            if !pieces_ok && let Some(reason) = capabilities.explain_unavailable("terminal.put") {
                return Err(reason);
            }
            verify_incarnation(client, &terminal, created_ms, "Image paste").await?;
            let path = if pieces_ok {
                put_pieces(client, &terminal, &name, &bytes).await?
            } else {
                put_whole(client, &terminal, &name, &bytes).await?
            };
            Ok(Outcome::Put { terminal, path })
        }
        Action::Edit(_) | Action::Create(_) | Action::Close { .. } | Action::Forget { .. } => {
            unreachable!("dispatched in rpc::run")
        }
    }
}

/// Raw bytes per acknowledged piece. The core caps one piece at 1 MB; a
/// 64 KiB piece keeps each base64 line well inside that and lets typing
/// and credit cross between pieces on a slow SSH link.
pub(super) const PUT_CHUNK_BYTES: usize = 64 * 1024;

/// One `terminal.put`: the whole file in a single base64 parameter. Every
/// core that offers `terminal.put` takes it — older cores keep this path.
async fn put_whole(
    client: &Client,
    terminal: &str,
    name: &str,
    bytes: &[u8],
) -> Result<String, String> {
    let result = client
        .call(
            "terminal.put",
            Some(json!({
                "terminal": terminal,
                "name": name,
                "data": crate::selection::base64_encode(bytes),
            })),
        )
        .await
        .map_err(|e| e.to_string())?;
    result["path"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "terminal.put replied without a path".into())
}

/// `terminal.put_begin` → `terminal.put_chunk` (repeated, each acknowledged)
/// → `terminal.put_finish`; `terminal.put_abort` when a piece fails. Only
/// for a core advertising `put_chunks`, and only for a terminal on that
/// core (its `put_begin` rejects `machine/tN` addresses).
async fn put_pieces(
    client: &Client,
    terminal: &str,
    name: &str,
    bytes: &[u8],
) -> Result<String, String> {
    let transfer = transfer_handle();
    client
        .call(
            "terminal.put_begin",
            Some(json!({
                "terminal": terminal,
                "name": name,
                "bytes": bytes.len(),
                "transfer": transfer,
            })),
        )
        .await
        .map_err(|e| e.to_string())?;
    let mut sent = 0;
    while sent < bytes.len() {
        let end = (sent + PUT_CHUNK_BYTES).min(bytes.len());
        let reply = client
            .call(
                "terminal.put_chunk",
                Some(json!({
                    "transfer": transfer,
                    "offset": sent,
                    "data": crate::selection::base64_encode(&bytes[sent..end]),
                })),
            )
            .await;
        match reply {
            // Each piece is answered with the bytes the core holds now:
            // a different count means the transfer cannot continue.
            Ok(value) if value["bytes"].as_u64() == Some(end as u64) => sent = end,
            Ok(_) => {
                return Err(
                    put_abort(client, &transfer, "the core did not acknowledge the chunk").await,
                );
            }
            Err(e) => return Err(put_abort(client, &transfer, &e.to_string()).await),
        }
    }
    // A finish that fails is not retried: the core may already have the
    // file, so it carries the same warning the core's own client gives.
    match client
        .call("terminal.put_finish", Some(json!({ "transfer": transfer })))
        .await
    {
        Ok(result) => result["path"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "terminal.put_finish replied without a path".into()),
        Err(e) => Err(format!("{e}; the upload may have landed")),
    }
}

/// Removes a transfer the core still holds and reports `reason`. The
/// abort's own failure never replaces the real error.
async fn put_abort(client: &Client, transfer: &str, reason: &str) -> String {
    let _ = client
        .call("terminal.put_abort", Some(json!({ "transfer": transfer })))
        .await;
    reason.to_owned()
}

/// A transfer handle: 32 hex digits, inside the core's 16–64 range.
/// `/dev/urandom` when it opens; the clock and process id otherwise, so
/// two transfers never share a handle even then.
fn transfer_handle() -> String {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .is_err()
    {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        bytes.copy_from_slice(&(nanos ^ (u128::from(std::process::id()) << 96)).to_be_bytes());
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A failed `terminal.send`. The core's `cancelled` kind (hc 0.1.21 and
/// later) means the caller went away or the core stopped before anything
/// was typed: a refusal, shown like any other failed action and never as a
/// success. hc-tui offers no automatic retry for a send — the message says
/// so, and the line can be sent again from the agent menu. A client-side
/// deadline is its own case: the request may still be held behind a
/// person's unsent draft, so the core can type it later.
pub(super) fn send_failure(error: hc_proto::client::CallError) -> String {
    use hc_proto::client::CallError;
    match error {
        error if error.is_kind(hc_proto::jsonrpc::error_kind::CANCELLED) => {
            "Send cancelled; nothing was typed. Send it again.".into()
        }
        CallError::Timeout => "Send timed out; the core may still type it.".into(),
        error => error.to_string(),
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
