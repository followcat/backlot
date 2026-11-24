use crate::guard_rules::refused_command_line;

#[test]
fn refuses_git_push_variants_and_global_flags() {
    assert_eq!(
        refused_command_line("git push origin HEAD").as_deref(),
        Some("git push origin")
    );
    assert_eq!(
        refused_command_line("/usr/bin/git -C /tmp/repo push -u origin main").as_deref(),
        Some("git push origin")
    );
    assert_eq!(
        refused_command_line("GIT_TRACE=1 env -u HOME git --no-pager push").as_deref(),
        Some("git push")
    );
    assert_eq!(refused_command_line("git status"), None);
    assert_eq!(refused_command_line("git commit -m 'git push'"), None);
}

#[test]
fn refuses_chained_piped_multiline_and_nested_shells() {
    assert_eq!(
        refused_command_line("cd /tmp/repo && git push origin main").as_deref(),
        Some("git push origin")
    );
    assert_eq!(
        refused_command_line("echo hi; gh pr create --title x").as_deref(),
        Some("gh pr create --title x")
    );
    assert_eq!(
        refused_command_line("set -e\n# comment git push\ncargo check"),
        None
    );
    assert_eq!(
        refused_command_line("set -e\ncargo publish").as_deref(),
        Some("cargo publish")
    );
    assert_eq!(
        refused_command_line(r#"bash -lc 'echo ok && ssh host-a uptime'"#).as_deref(),
        Some("ssh host-a uptime")
    );
    assert_eq!(refused_command_line(r#"echo "git push origin HEAD""#), None);
    assert_eq!(refused_command_line("command -v gh"), None);
}
