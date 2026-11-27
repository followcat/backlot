//! Unit tests for implicit `AccountId` derivation and the strict no-secrets boundary.

use std::collections::BTreeMap;

use backlot_model::hollywood::sha256_hex;

use super::*;

fn host_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("HOME".into(), "/home/user".into()),
        ("XDG_CONFIG_HOME".into(), "/home/user/.config".into()),
    ])
}

#[test]
fn all_runners_default_when_no_overrides() {
    let host = host_env();
    let empty = BTreeMap::new();
    for (runner, model, expected_id, expected_src) in [
        ("claude", "opus-5-5", "claude:default", "default"),
        ("codex", "gpt-5.4", "codex:default", "default"),
        ("cursor", "composer-1", "cursor:default", "default"),
        ("agy", "gemini-4-neon-high", "agy:default", "default"),
        ("devin", "default", "devin:default", "default"),
        ("cmd", "sonnet", "cmd:default", "default"),
        (
            "cline",
            "cline-pass/deepseek-v4.1-flash",
            "cline:default",
            "default",
        ),
        ("opencode", "big-pickle", "opencode:default", "default"),
        ("fake", "fake", "fake:default", "default"),
    ] {
        let acct = derive_account(runner, model, &empty, &host);
        assert_eq!(acct.id, expected_id, "{runner}");
        assert_eq!(acct.runner, runner);
        assert_eq!(acct.source, expected_src, "{runner}");
    }
}

#[test]
fn claude_config_dir_and_codex_home_derive_tags_and_collapse_default() {
    let host = host_env();

    let claude_default = BTreeMap::from([("CLAUDE_CONFIG_DIR".into(), "~/.claude".into())]);
    let acct = derive_account("claude", "opus-5-5", &claude_default, &host);
    assert_eq!(acct.id, "claude:default");
    assert_eq!(acct.source, "default");

    let claude_work = BTreeMap::from([("CLAUDE_CONFIG_DIR".into(), "~/.claude-work".into())]);
    let acct = derive_account("claude", "opus-5-5", &claude_work, &host);
    assert_eq!(acct.id, "claude:work");
    assert_eq!(acct.source, "config-dir (~/.claude-work)");

    let codex_default = BTreeMap::from([("CODEX_HOME".into(), "/home/user/.codex/.".into())]);
    let acct = derive_account("codex", "gpt-5.4", &codex_default, &host);
    assert_eq!(acct.id, "codex:default");

    let codex_b = BTreeMap::from([("CODEX_HOME".into(), "/home/user/.codex-b".into())]);
    let acct = derive_account("codex", "gpt-5.4", &codex_b, &host);
    assert_eq!(acct.id, "codex:b");
    assert_eq!(acct.source, "config-dir (~/.codex-b)");
}

#[test]
fn colliding_dir_basenames_disambiguate_with_non_secret_path_sha256() {
    let host = host_env();
    let env_a = BTreeMap::from([("CLAUDE_CONFIG_DIR".into(), "/a/.claude-x".into())]);
    let env_b = BTreeMap::from([("CLAUDE_CONFIG_DIR".into(), "/b/.claude-x".into())]);
    let env_same = BTreeMap::from([("CLAUDE_CONFIG_DIR".into(), "/a/../a/.claude-x".into())]);

    let resolved = resolve_accounts(
        &[
            ("claude", "opus-5-5", &env_a),
            ("claude", "sonnet-5", &env_b),
            ("claude", "haiku", &env_same),
        ],
        &host,
    );
    assert_ne!(resolved[0].id, resolved[1].id);
    assert_eq!(resolved[0].id, resolved[2].id);
    let expected_suffix_a = &sha256_hex(b"/a/.claude-x")[..6];
    let expected_suffix_b = &sha256_hex(b"/b/.claude-x")[..6];
    assert_eq!(resolved[0].id, format!("claude:x-{expected_suffix_a}"));
    assert_eq!(resolved[1].id, format!("claude:x-{expected_suffix_b}"));
}

#[test]
fn cursor_ignores_per_role_config_dirs() {
    let host = host_env();
    let role_a = BTreeMap::from([
        (
            "CURSOR_CONFIG_DIR".into(),
            "/tmp/studios/acme-shop/roles/actor-t1-c1/cursor".into(),
        ),
        (
            "CURSOR_DATA_DIR".into(),
            "/tmp/studios/acme-shop/roles/actor-t1-c1/cursor/data".into(),
        ),
    ]);
    let role_b = BTreeMap::from([(
        "CURSOR_CONFIG_DIR".into(),
        "/tmp/studios/acme-shop/roles/director/cursor".into(),
    )]);

    let a = derive_account("cursor", "composer-1", &role_a, &host);
    let b = derive_account("cursor", "composer-1", &role_b, &host);
    assert_eq!(a.id, "cursor:default");
    assert_eq!(b.id, "cursor:default");
}

#[test]
fn home_and_xdg_and_opencode_providers_and_env_tokens_resolve() {
    let host = host_env();

    let agy_work = BTreeMap::from([("HOME".into(), "/home/user/homes/work".into())]);
    assert_eq!(
        derive_account("agy", "gemini-4-neon-high", &agy_work, &host).id,
        "agy:work"
    );

    let cmd_alt = BTreeMap::from([("HOME".into(), "/opt/cmd-alt".into())]);
    assert_eq!(
        derive_account("cmd", "sonnet", &cmd_alt, &host).id,
        "cmd:alt"
    );

    let cline_alt = BTreeMap::from([("HOME".into(), "/opt/cline_team".into())]);
    assert_eq!(
        derive_account("cline", "cline-pass/glm-5.3", &cline_alt, &host).id,
        "cline:team"
    );

    let devin_xdg = BTreeMap::from([("XDG_CONFIG_HOME".into(), "/home/user/.config-b".into())]);
    assert_eq!(
        derive_account("devin", "default", &devin_xdg, &host).id,
        "devin:config-b"
    );

    let empty = BTreeMap::new();
    let oc_cline = derive_account("opencode", "cline-pass/model-a", &empty, &host);
    assert_eq!(oc_cline.id, "opencode:cline-pass");
    assert_eq!(oc_cline.source, "provider (cline-pass)");

    let oc_go = derive_account("opencode", "opencode-go/model-b", &empty, &host);
    assert_eq!(oc_go.id, "opencode:opencode-go");
    assert_eq!(oc_go.source, "provider (opencode-go)");

    let tok = BTreeMap::from([("OPENAI_API_KEY".into(), "sk-secret-openai-999".into())]);
    let codex_tok = derive_account("codex", "gpt-5.4", &tok, &host);
    assert_eq!(codex_tok.id, "codex:env-token");
    assert_eq!(codex_tok.source, "env-token");

    let cursor_tok = BTreeMap::from([("CURSOR_API_KEY".into(), "cur-secret-777".into())]);
    assert_eq!(
        derive_account("cursor", "composer-1", &cursor_tok, &host).id,
        "cursor:env-token"
    );
}

#[test]
fn no_secret_value_or_secret_hash_ever_leaks_into_derived_account() {
    let host = host_env();
    let secret_api = "sk-secret-xyz-do-not-leak";
    let secret_oauth = "tok-secret-oauth-do-not-leak";
    let env_a = BTreeMap::from([("ANTHROPIC_API_KEY".into(), secret_api.into())]);
    let env_b = BTreeMap::from([("CLAUDE_CODE_OAUTH_TOKEN".into(), secret_oauth.into())]);

    for (env, secret) in [(&env_a, secret_api), (&env_b, secret_oauth)] {
        let acct = derive_account("claude", "opus-5-5", env, &host);
        assert_eq!(acct.id, "claude:env-token");
        assert_eq!(acct.source, "env-token");
        let dump = format!("{acct:?}");
        assert!(!dump.contains(secret), "secret leaked: {dump}");
        let secret_hash = sha256_hex(secret.as_bytes());
        assert!(!dump.contains(&secret_hash), "secret hash leaked: {dump}");
        assert!(
            !dump.contains(&secret_hash[..6]),
            "short secret hash leaked: {dump}"
        );
    }
}

#[test]
fn derive_for_state_dir_reads_config_without_exposing_env_secrets() {
    let tmp = crate::test_support::TestDir::new("backlot-accounts-test");
    std::fs::write(
        tmp.join("config.toml"),
        "[runner_env.claude]\nCLAUDE_CONFIG_DIR = \"~/.claude-team\"\nANTHROPIC_API_KEY = \"sk-secret-toml-never-leak\"\n",
    )
    .unwrap();
    let host = host_env();
    let acct = derive_for_state_dir(&tmp, &host, "claude", "opus-5-5");
    assert_eq!(acct.id, "claude:team");
    assert_eq!(acct.source, "config-dir (~/.claude-team)");
    let dump = format!("{acct:?}");
    assert!(!dump.contains("sk-secret-toml-never-leak"));
}
