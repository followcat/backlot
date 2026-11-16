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
        Action::Edit(_) | Action::Create(_) | Action::Close { .. } | Action::Forget { .. } => {
            unreachable!("dispatched in rpc::run")
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
