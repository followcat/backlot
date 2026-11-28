//! The stage's published Agent Skill state and its cheap ten-minute
//! rescan cadence.

use std::collections::BTreeMap;
use std::fs;

use backlot_model::summary::SkillState;

use crate::doc_req_studio_seed::Server;
use crate::doc_req_verbs_seed::{env_for, make_fake_runner};
use crate::test_support::TestDir;

#[test]
fn the_summary_publishes_the_skill_report_and_rescans_after_ten_minutes() {
    let dir = TestDir::new("backlot-skill-stage-summary");
    let env = env_for(&dir, &make_fake_runner(&dir));
    fs::create_dir_all(dir.join("home/.claude")).unwrap();
    let server = Server::start(&env);
    let first = server.stage.summary_view_at(&BTreeMap::new(), 1_000);
    let skill = first.agent_skill.as_ref().expect("skill summary");
    assert_eq!(skill.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(skill.rows.len(), crate::skill_agents::AGENTS.len());
    let claude = skill
        .rows
        .iter()
        .find(|row| row.agent == "claude-code")
        .unwrap();
    assert_eq!(claude.state, SkillState::Missing);
    assert_eq!(claude.dir, "~/.claude/skills");

    let file = dir.join("home/.claude/skills/backlot/SKILL.md");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, crate::skill::generated(env!("CARGO_PKG_VERSION"))).unwrap();
    let cached = server.stage.summary_view_at(&BTreeMap::new(), 2_000);
    assert_eq!(
        cached
            .agent_skill
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.agent == "claude-code")
            .unwrap()
            .state,
        SkillState::Missing,
        "the one-second tick reuses the cheap cache"
    );
    let later = server
        .stage
        .summary_view_at(&BTreeMap::new(), 1_000 + 10 * 60 * 1000 + 1);
    assert_eq!(
        later
            .agent_skill
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.agent == "claude-code")
            .unwrap()
            .state,
        SkillState::Current,
        "the ten-minute scan picks the written copy up"
    );
}
