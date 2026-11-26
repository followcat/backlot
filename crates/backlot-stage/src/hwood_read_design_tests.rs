//! Fail-first tests for serving `design/` and `plan/` Markdown docs through
//! `hwood_read::report_relpath` and `hwood_read::report_show` while preserving
//! traversal, extension, dotfile, and escaping-symlink refusals.

use crate::args::Common;
use crate::cli::Env;
use crate::hwood_read::{report_relpath, report_show};
use crate::store;
use crate::test_support::TestDir;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn env_at(dir: &Path) -> Env {
    Env {
        vars: BTreeMap::from([(
            "BACKLOT_STATE_DIR".to_string(),
            dir.to_string_lossy().into_owned(),
        )]),
    }
}

fn common_acme() -> Common {
    Common {
        studio: Some("acme-shop".to_string()),
        ..Common::default()
    }
}

#[test]
fn report_relpath_allows_design_and_plan_markdown_only() {
    let good = [
        ("design/v1.md", "design/v1.md"),
        ("design/v2-final.md", "design/v2-final.md"),
        (
            "design/v1/r1-0014-proposal.md",
            "design/v1/r1-0014-proposal.md",
        ),
        ("plan/v1.md", "plan/v1.md"),
        ("plan/1-plan-x.md", "plan/1-plan-x.md"),
    ];
    for (input, want) in good {
        let rel = report_relpath(Path::new(input))
            .unwrap_or_else(|e| panic!("expected `{input}` allowed, got err: {e:?}"));
        assert_eq!(rel, Path::new(want), "input={input}");
    }

    let bad = [
        "design/../events.jsonl",
        "design/v1.sh",
        "design/.hook-capture.jsonl",
        "design/.secret.md",
        "plan/1-plan.sh",
        "plan/.hidden.md",
        "plan/../events.jsonl",
        "/etc/passwd",
        "design/v1.md\0.png",
        "design/",
        "plan/",
    ];
    for input in bad {
        assert!(
            report_relpath(Path::new(input)).is_err(),
            "expected `{input}` to be rejected"
        );
    }
}

#[test]
fn report_show_serves_design_and_plan_and_blocks_escaping_symlinks() {
    let dir = TestDir::new("hwood-read-design");
    let studio = store::studio_dir(&dir, "acme-shop");
    fs::create_dir_all(studio.join("design/v1")).unwrap();
    fs::create_dir_all(studio.join("plan")).unwrap();
    fs::write(studio.join("events.jsonl"), "").unwrap();

    fs::write(studio.join("design/v1.md"), "# Design v1\n\n## R1\n").unwrap();
    fs::write(
        studio.join("design/v1-final.md"),
        "# Design v1 (final)\n\n## Sign-off (#5)\n",
    )
    .unwrap();
    fs::write(
        studio.join("design/v1/r1-0014-proposal.md"),
        "### §R1.W Proposal · writer (#14)\n\nScope.\n",
    )
    .unwrap();
    fs::write(studio.join("plan/v1.md"), "# Plan v1\n\nStep 1.\n").unwrap();

    let outside = dir.path().join("outside-secret.md");
    fs::write(&outside, "top secret").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, studio.join("design/v9.md")).unwrap();

    let common = common_acme();
    let env = env_at(&dir);

    for (rel, expected_fragment) in [
        ("design/v1.md", "## R1"),
        ("design/v1-final.md", "## Sign-off (#5)"),
        ("design/v1/r1-0014-proposal.md", "### §R1.W Proposal"),
        ("plan/v1.md", "# Plan v1"),
    ] {
        let mut out = Vec::new();
        report_show(&common, &env, &dir, Path::new(rel), &mut out)
            .unwrap_or_else(|e| panic!("report_show `{rel}` failed: {e:?}"));
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains(expected_fragment), "rel={rel}\n{text}");
    }

    #[cfg(unix)]
    {
        let mut out = Vec::new();
        assert!(
            report_show(&common, &env, &dir, Path::new("design/v9.md"), &mut out).is_err(),
            "escaping symlink inside design/ must be refused"
        );
    }

    let mut out = Vec::new();
    assert!(
        report_show(&common, &env, &dir, Path::new("design/v1.sh"), &mut out).is_err(),
        "non-markdown file under design/ must be refused"
    );
}
