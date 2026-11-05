//! Agree on adoption labels through CAS before changing fallback identities.
use super::*;
use hc_proto::{client::Client, terminal::TerminalParams};
use serde_json::json;

pub(super) fn space_id(raw: &str) -> String {
    format!("space:{}:{raw}", raw.len())
}
pub(super) fn tab_id(space: &str, raw: &str) -> String {
    format!("{space}/tab:{}:{raw}", raw.len())
}

pub(super) async fn prepare(client: &Client, creation: &mut Creation) -> Result<(), String> {
    let Some(promotion) = &creation.promotion else {
        return Ok(());
    };
    let written = super::rpc::update(client, |document| {
        let chosen = document
            .adoptions
            .entry(promotion.key.clone())
            .or_insert_with(|| promotion.proposal.clone());
        for (key, raw) in &promotion.proposal.tabs {
            chosen
                .tabs
                .entry(key.clone())
                .or_insert_with(|| raw.clone());
        }
        if chosen.machine != promotion.proposal.machine {
            return Err("Adoption machine changed".into());
        }
        let chosen = chosen.clone();
        let workspace = space_id(&chosen.space);
        let mut aliases = vec![(
            promotion.key.clone(),
            tab_key(chosen.machine.as_deref(), &workspace),
        )];
        aliases.extend(chosen.tabs.iter().map(|(old, raw)| {
            (
                old.clone(),
                tab_key(chosen.machine.as_deref(), &tab_id(&workspace, raw)),
            )
        }));
        super::edit::migrate(document, &aliases);
        Ok(())
    })
    .await?;
    let chosen = written
        .document
        .adoptions
        .get(&promotion.key)
        .ok_or("Missing agreed adoption labels")?;
    let workspace = space_id(&chosen.space);
    let machine = chosen.machine.as_deref();
    let tab = match &creation.edit.change {
        Change::Split { .. } => chosen
            .tabs
            .get(&creation.edit.tab)
            .cloned()
            .ok_or("Split tab no longer exists")?,
        Change::NewTab { .. } => creation
            .params
            .labels
            .get("herdr.tab")
            .cloned()
            .ok_or("Missing new tab identity")?,
        _ => return Err("Invalid creation placement".into()),
    };
    creation.edit.space = tab_key(machine, &workspace);
    creation.edit.tab = tab_key(machine, &tab_id(&workspace, &tab));
    creation
        .params
        .labels
        .insert("herdr.space".into(), chosen.space.clone());
    creation.params.labels.insert("herdr.tab".into(), tab);
    creation.order = creation
        .order
        .iter()
        .map(|old| {
            let key = tab_key(machine, old);
            chosen
                .tabs
                .get(&key)
                .map(|raw| tab_id(&workspace, raw))
                .unwrap_or_else(|| old.clone())
        })
        .collect();
    for member in &promotion.members {
        let raw_tab = chosen
            .tabs
            .get(&member.tab)
            .ok_or("Adopted tab no longer exists")?;
        let labels = BTreeMap::from([
            ("herdr.space".to_owned(), chosen.space.clone()),
            ("herdr.tab".to_owned(), raw_tab.clone()),
        ]);
        let current = client
            .terminal_get(TerminalParams {
                terminal: member.terminal.clone(),
                caller: None,
            })
            .await
            .map_err(|e| e.to_string())?;
        let matches = |expected: &BTreeMap<String, String>| {
            ["herdr.space", "herdr.tab"]
                .iter()
                .all(|key| current.labels.get(*key) == expected.get(*key))
        };
        if matches(&labels) {
            continue;
        }
        if !matches(&member.labels) {
            return Err("Space labels changed; refresh before retrying the action".into());
        }
        client
            .call(
                "terminal.label",
                Some(json!({"terminal":member.terminal,"labels":labels})),
            )
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
