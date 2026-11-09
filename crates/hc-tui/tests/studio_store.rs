//! Binary-level tests for the store side: concurrent appends across
//! processes and the studio-name checks before any path is built.

mod studio_kit;

use std::fs;
use std::process::Command;

use studio_kit::*;

#[test]
fn concurrent_appends_across_processes() {
    let s = Scratch::new();
    let cast = s.fixture("cast.toml", CAST_TOML);
    ok(
        &s,
        &[
            "studio",
            "new",
            "--title",
            "Shop",
            "--repo",
            "/home/user/acme-shop",
            "--cast",
            &cast,
        ],
    );

    let procs = 8;
    let per = 25;
    let mut children = Vec::new();
    for _ in 0..procs {
        children.push(std::thread::spawn({
            let root = s.root.clone();
            let state = s.state.clone();
            move || {
                for _ in 0..per {
                    let o = Command::new(env!("CARGO_BIN_EXE_backlot"))
                        .args(["plan", "request", "--as", "producer"])
                        .env_clear()
                        .env("PATH", "/usr/bin:/bin")
                        .env("HOME", root.join("home"))
                        .env("BACKLOT_STATE_DIR", &state)
                        .env("BACKLOT_STUDIO", "acme-shop")
                        .current_dir(&root)
                        .output()
                        .unwrap();
                    assert!(
                        o.status.success(),
                        "{}",
                        String::from_utf8(o.stderr).unwrap()
                    );
                }
            }
        }));
    }
    for c in children {
        c.join().unwrap();
    }

    let log = fs::read_to_string(s.events()).unwrap();
    let mut seqs: Vec<u64> = log
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["seq"]
                .as_u64()
                .unwrap()
        })
        .collect();
    assert_eq!(seqs.len(), 1 + procs * per);
    // In-file order is already the total order.
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");
    seqs.sort_unstable();
    seqs.dedup();
    let total = (1 + procs * per) as u64;
    assert_eq!(seqs, (1..=total).collect::<Vec<_>>());
}

/// Studio names are `[a-z0-9_-]{1,16}`: bad names exit 2 before any
/// path is built, and an append on a missing studio exits 1 without
/// creating anything — `../../escape` can never leave the state dir.
#[test]
fn studio_names_are_checked_before_paths() {
    let s = Scratch::new();
    for bad in ["../x", "../../escape", "Upper", "seventeen_charname"] {
        let o = run(&s, &["status", "--studio", bad]);
        assert_eq!(o.status.code(), Some(2), "{bad}: {}", stderr(&o));
        let o = run(
            &s,
            &["plan", "request", "--studio", bad, "--as", "producer"],
        );
        assert_eq!(o.status.code(), Some(2), "{bad}: {}", stderr(&o));
    }
    let studios = s.state.join("studios");
    assert!(!studios.exists() || fs::read_dir(studios).unwrap().next().is_none());
    assert!(!s.root.join("escape").exists());

    let o = run(
        &s,
        &["plan", "request", "--studio", "nosuch", "--as", "producer"],
    );
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    assert!(stderr(&o).contains("no studio 'nosuch'"), "{}", stderr(&o));
    assert!(!s.state.join("studios/nosuch").exists());
}
