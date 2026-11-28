//! `skill.install` / `skill.uninstall` through `plugin.backlot.req`:
//! person execution, exact agent rows, role refusal and the foreign-copy
//! force rule — all against a temporary HOME.

use std::fs;

use backlot_model::req_results::SkillResult;
use backlot_model::req_wire::ReqAction;

use crate::doc_req_studio_seed::{Server, err_line};
use crate::doc_req_verbs_seed::{env_for, make_fake_runner};
use crate::test_support::TestDir;

fn skill_file(env: &crate::cli::Env, dir: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env.get("HOME").unwrap())
        .join(dir)
        .join("skills/backlot/SKILL.md")
}

#[test]
fn skill_install_from_the_person_installs_exactly_the_named_rows() {
    let dir = TestDir::new("blops-skill-install");
    let env = env_for(&dir, &make_fake_runner(&dir));
    fs::create_dir_all(skill_file(&env, ".agents").parent().unwrap()).unwrap();
    fs::create_dir_all(skill_file(&env, ".config/devin").parent().unwrap()).unwrap();
    let mut server = Server::start(&env);
    let result: SkillResult = server.typed(
        &ReqAction::SkillInstall {
            agents: vec!["codex".into(), "devin".into()],
            force: false,
            as_role: None,
        },
        None,
    );
    assert_eq!(result.action, "install");
    assert!(skill_file(&env, ".agents").exists());
    assert!(skill_file(&env, ".config/devin").exists());
    assert!(
        !skill_file(&env, ".cursor").exists(),
        "an unnamed row is never created"
    );
    assert!(
        result
            .skill
            .rows
            .iter()
            .any(|row| row.agent == "codex"
                && row.state == backlot_model::summary::SkillState::Current),
        "the ack carries the refreshed state"
    );
}

#[test]
fn skill_requests_from_a_role_are_refused() {
    let dir = TestDir::new("blops-skill-role");
    let env = env_for(&dir, &make_fake_runner(&dir));
    let mut server = Server::start(&env);
    for action in [
        ReqAction::SkillInstall {
            agents: vec!["codex".into()],
            force: false,
            as_role: Some("director".into()),
        },
        ReqAction::SkillUninstall {
            agents: vec!["codex".into()],
            as_role: Some("director".into()),
        },
    ] {
        let ack = server.send(&action, None);
        assert!(err_line(&ack).contains("person-only"), "{:?}", ack.error);
    }
    assert!(!skill_file(&env, ".agents").exists());
}

#[test]
fn replacing_a_foreign_copy_requires_force_and_uninstall_leaves_it() {
    let dir = TestDir::new("blops-skill-force");
    let env = env_for(&dir, &make_fake_runner(&dir));
    let file = skill_file(&env, ".agents");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "mine\n").unwrap();
    let mut server = Server::start(&env);
    let install = |force| ReqAction::SkillInstall {
        agents: vec!["codex".into()],
        force,
        as_role: None,
    };
    let _: SkillResult = server.typed(&install(false), None);
    assert_eq!(fs::read_to_string(&file).unwrap(), "mine\n");
    let _: SkillResult = server.typed(&install(true), None);
    assert!(fs::read_to_string(&file).unwrap().contains("generated-by"));

    fs::write(&file, "mine\n").unwrap();
    let _: SkillResult = server.typed(
        &ReqAction::SkillUninstall {
            agents: vec!["codex".into()],
            as_role: None,
        },
        None,
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "mine\n");
}
