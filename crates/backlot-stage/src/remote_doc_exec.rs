//! Sending one request (or a reviewed dry run plus its commit) and
//! formatting the ack (B1b).

use std::time::Duration;

use backlot_model::req_results::StudioPlan;
use backlot_model::req_wire::ReqAction;
use hc_proto::client::Client;
use hc_proto::endpoint::Endpoint;
use serde_json::Value;

use crate::cli::Fail;

use super::format_ack_json;

/// The same, choosing the command's `--json` document over its text.
/// A destructive commit first sends its own dry run and echoes the
/// returned confirm token, so one invocation behaves like the local
/// command.
pub(crate) async fn execute_client_with(
    client: &Client,
    endpoint: Option<&Endpoint>,
    action: &ReqAction,
    timeout: Duration,
    json: bool,
) -> Result<String, Fail> {
    execute_client_confirmed(client, endpoint, action, timeout, json, None).await
}

/// The same with a token a dialog already reviewed: a destructive
/// commit carrying it is sent as-is, so a stale plan is refused
/// instead of silently re-reviewed. `None` keeps the one-invocation
/// dry-run-then-commit flow.
pub async fn execute_client_confirmed(
    client: &Client,
    endpoint: Option<&Endpoint>,
    action: &ReqAction,
    timeout: Duration,
    json: bool,
    token: Option<String>,
) -> Result<String, Fail> {
    if let Some(token) = token
        && review_request(action).is_some()
    {
        let ack =
            crate::doc_client::send_request(client, endpoint, action, Some(token), timeout).await?;
        return format_ack_json(action, ack, json);
    }
    if let Some(dry) = review_request(action) {
        let ack = crate::doc_client::send_request(client, endpoint, &dry, None, timeout).await?;
        if !ack.ok {
            return format_ack_json(&dry, ack, json);
        }
        let token = plan_token(&ack)?;
        let ack =
            crate::doc_client::send_request(client, endpoint, action, Some(token), timeout).await?;
        return format_ack_json(action, ack, json);
    }
    let ack = crate::doc_client::send_request(client, endpoint, action, None, timeout).await?;
    format_ack_json(action, ack, json)
}

/// A dry run's two answers: the text/JSON the local command prints and
/// the confirm token its commit must echo. The dialogs show the text
/// and keep the token.
pub async fn execute_plan(
    client: &Client,
    endpoint: Option<&Endpoint>,
    action: &ReqAction,
    timeout: Duration,
    json: bool,
) -> Result<(String, String), Fail> {
    let ack = crate::doc_client::send_request(client, endpoint, action, None, timeout).await?;
    if !ack.ok {
        let Err(e) = format_ack_json(action, ack, json) else {
            unreachable!("a refused ack always formats to an error")
        };
        return Err(e);
    }
    let token = plan_token(&ack)?;
    let text = format_ack_json(action, ack, json)?;
    Ok((text, token))
}

/// A commit that must be reviewed: its own dry run, carried out in the
/// same invocation.
fn review_request(action: &ReqAction) -> Option<ReqAction> {
    match action {
        ReqAction::StudioDelete {
            studio,
            dry_run: false,
            force,
        } => Some(ReqAction::StudioDelete {
            studio: studio.clone(),
            dry_run: true,
            force: *force,
        }),
        ReqAction::StudioPrune {
            studio,
            all_finished,
            dry_run: false,
            force,
        } => Some(ReqAction::StudioPrune {
            studio: studio.clone(),
            all_finished: *all_finished,
            dry_run: true,
            force: *force,
        }),
        ReqAction::StudioRestore {
            archived,
            name,
            dry_run: false,
        } => Some(ReqAction::StudioRestore {
            archived: archived.clone(),
            name: name.clone(),
            dry_run: true,
        }),
        _ => None,
    }
}

fn plan_token(ack: &backlot_model::req_wire::ResAck) -> Result<String, Fail> {
    let val = ack
        .result
        .clone()
        .ok_or_else(|| Fail::core("server ack omitted result payload"))?;
    let plan: StudioPlan<Value> = serde_json::from_value(val)
        .map_err(|e| Fail::core(format!("malformed server plan: {e}")))?;
    Ok(plan.confirm_token)
}
