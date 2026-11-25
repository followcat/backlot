//! B1b: report parity (`--json` == ack result) and stale-plan handling
//! for the destructive studio verbs.

use backlot_model::req_results::StudioPlan;
use backlot_model::req_wire::ReqAction;

use crate::cli_testkit::{self, CAST_TOML, fixture, git, ok, shared_area, shared_repo};
use crate::delete::DeleteReport;
use crate::doc_req_studio_seed::{Server, err_line};
use crate::prune::{PruneReport, Step};

#[test]
fn prune_reports_a_stale_plan_and_json_matches_the_dry_run_ack() {
    let (scratch, env) = cli_testkit::scratch();
    let repo = shared_repo(&scratch);
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
    shared_area(&scratch, "integration");
    ok(&env, "studio finish --reason done --force --as user");
    let mut server = Server::start(&env);

    let prune = |dry_run: bool| ReqAction::StudioPrune {
        studio: Some("acme-shop".into()),
        all_finished: false,
        dry_run,
        force: false,
    };

    let local: serde_json::Value =
        serde_json::from_str(&ok(&env, "studio prune acme-shop --dry-run --json")).unwrap();
    let plan: StudioPlan<PruneReport> = server.typed(&prune(true), None);
    assert_eq!(
        serde_json::to_value(&plan.report).unwrap(),
        local,
        "the CLI --json and the ack result must be one document"
    );
    assert_eq!(
        err_line(&server.send(&prune(false), None)),
        "confirm this from a dry run first"
    );

    // A branch appears between the review and the commit.
    git(
        &repo,
        &[
            "branch",
            "backlot/acme-shop/extra",
            "backlot/acme-shop/integration",
        ],
    );
    assert_eq!(
        err_line(&server.send(&prune(false), Some(plan.confirm_token))),
        "the studio changed since you reviewed it; run it again"
    );
    let fresh: StudioPlan<PruneReport> = server.typed(&prune(true), None);
    let done: PruneReport = server.typed(&prune(false), Some(fresh.confirm_token));
    assert!(done.studios[0].steps.iter().any(|s| matches!(
        s,
        Step::DeletedBranch(b) if b == "backlot/acme-shop/extra"
    )));

    // The probes answer from the hub, and their --json is the ack.
    let local_repos: serde_json::Value =
        serde_json::from_str(&ok(&env, "repos --json --all")).unwrap();
    let ack = server.send(
        &ReqAction::Repos {
            from: None,
            all: true,
        },
        None,
    );
    assert!(ack.ok, "{:?}", ack.error);
    assert_eq!(ack.result.unwrap(), local_repos);

    let local_runners: serde_json::Value =
        serde_json::from_str(&ok(&env, "runners --json")).unwrap();
    let ack = server.send(&ReqAction::Runners, None);
    assert!(ack.ok, "{:?}", ack.error);
    assert_eq!(ack.result.unwrap(), local_runners);
}

#[test]
fn delete_dry_run_json_and_ack_result_are_the_same_document() {
    let (scratch, env) = cli_testkit::scratch();
    let repo = shared_repo(&scratch);
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
    shared_area(&scratch, "integration");
    ok(&env, "studio finish --reason done --force --as user");
    let mut server = Server::start(&env);

    let local: serde_json::Value =
        serde_json::from_str(&ok(&env, "studio delete acme-shop --dry-run --json")).unwrap();
    let plan: StudioPlan<DeleteReport> = server.typed(
        &ReqAction::StudioDelete {
            studio: Some("acme-shop".into()),
            dry_run: true,
            force: false,
        },
        None,
    );
    assert_eq!(serde_json::to_value(&plan.report).unwrap(), local);
}
