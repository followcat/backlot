//! cline's session discovery against fixture manifests: the launch's
//! own manifest, another folder's, an older start, a directory without
//! one, another process's, and the pid tree that tells them apart.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::cline::session_at;
use super::cline_tests::tmp;

/// One fixture manifest: `<root>/<id>/<id>.json` with the fields a
/// real launch writes, plus whatever the test overrides.
fn manifest(root: &Path, id: &str, fields: serde_json::Value) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    let mut v = serde_json::json!({
        "version": 1,
        "session_id": id,
        "source": "cli",
        "cwd": "/home/user/acme-shop",
        "workspace_root": "/home/user/acme-shop",
        "interactive": true,
        "status": "running",
    });
    if let (Some(base), Some(extra)) = (v.as_object_mut(), fields.as_object()) {
        for (k, val) in extra {
            base.insert(k.clone(), val.clone());
        }
    }
    fs::write(dir.join(format!("{id}.json")), v.to_string()).unwrap();
}

/// The fixture process tree: engine pid → the terminal's pid.
fn tree(pairs: &[(u32, u32)]) -> impl Fn(u32) -> Option<u32> + use<> {
    let pairs: Vec<(u32, u32)> = pairs.to_vec();
    move |pid: u32| {
        pairs
            .iter()
            .find(|(p, _)| *p == pid)
            .map(|(_, parent)| *parent)
    }
}

/// The launch's own manifest is found: the role cwd, its pid under the
/// terminal, at or after the launch. Another folder's session, an
/// older one, a directory with no manifest and another process's
/// session are all skipped (S14a section 3).
#[test]
fn session_at_finds_the_launch_manifest() {
    let dir = tmp("sessions");
    let root = dir.join("sessions");
    fs::create_dir_all(&root).unwrap();
    let owner = 4241;
    let mine = "1794894104593_rurer";
    manifest(&root, mine, serde_json::json!({"pid": 4242}));
    // Another folder's session.
    manifest(
        &root,
        "1794894104594_other",
        serde_json::json!({"pid": 4200, "cwd": "/home/user/other-repo"}),
    );
    // A session from another process tree.
    manifest(
        &root,
        "1794894104595_stray",
        serde_json::json!({"pid": 9000}),
    );
    // A dir with no manifest at all, and one whose manifest is
    // corrupt.
    fs::create_dir_all(root.join("1794894104596_none")).unwrap();
    fs::create_dir_all(root.join("1794894104597_bad")).unwrap();
    fs::write(root.join("1794894104597_bad/bad.json"), "not json").unwrap();
    // A person's own session in the same folder.
    manifest(
        &root,
        "1794894104598_human",
        serde_json::json!({"pid": 9100, "source": "cli"}),
    );
    let taken = BTreeSet::new();
    let found = session_at(
        &root,
        "/home/user/acme-shop",
        1,
        &taken,
        Some(owner),
        &tree(&[(4242, 4241), (9000, 8000), (9100, 9200)]),
    );
    assert_eq!(found.as_deref(), Some(mine));
    // A taken id is skipped; the other folder's and other processes'
    // sessions stay out, so nothing is left.
    let taken = BTreeSet::from([mine.to_string()]);
    assert_eq!(
        session_at(
            &root,
            "/home/user/acme-shop",
            1,
            &taken,
            Some(owner),
            &tree(&[(4242, 4241), (9000, 8000), (9100, 9200)])
        ),
        None
    );
}

/// Manifests older than the launch are skipped: `started_ms` past
/// every file's time finds nothing. A manifest whose parent pid is
/// unknown is not rejected on a guess, and without an owner pid the
/// check is skipped entirely.
#[test]
fn session_at_respects_the_launch_time_and_the_pid() {
    let dir = tmp("sessions-time");
    let root = dir.join("sessions");
    fs::create_dir_all(&root).unwrap();
    let id = "1794894104593_rurer";
    manifest(&root, id, serde_json::json!({"pid": 4242}));
    let none = BTreeSet::new();
    // "Older start": every file precedes the launch.
    assert_eq!(
        session_at(
            &root,
            "/home/user/acme-shop",
            u64::MAX,
            &none,
            Some(4241),
            &tree(&[(4242, 4241)])
        ),
        None
    );
    // The parent is this role's terminal.
    assert_eq!(
        session_at(
            &root,
            "/home/user/acme-shop",
            1,
            &none,
            Some(4241),
            &tree(&[(4242, 4241)])
        )
        .as_deref(),
        Some(id)
    );
    // A different parent is another session.
    assert_eq!(
        session_at(
            &root,
            "/home/user/acme-shop",
            1,
            &none,
            Some(1111),
            &tree(&[(4242, 4241)])
        ),
        None
    );
    // A pid the lookup cannot resolve keeps the session: nothing
    // proves it is somebody else's.
    assert_eq!(
        session_at(
            &root,
            "/home/user/acme-shop",
            1,
            &none,
            Some(4241),
            &tree(&[])
        )
        .as_deref(),
        Some(id)
    );
    // Without the terminal's pid there is nothing to compare.
    assert_eq!(
        session_at(&root, "/home/user/acme-shop", 1, &none, None, &tree(&[])).as_deref(),
        Some(id)
    );
    // A missing store is no session.
    assert_eq!(
        session_at(
            &dir.join("gone"),
            "/home/user/acme-shop",
            1,
            &none,
            None,
            &tree(&[])
        ),
        None
    );
}

/// The store is the user's own `~/.cline/data/sessions`, and the pid
/// lookup reads a real process tree.
#[test]
fn the_store_and_the_pid_lookup() {
    assert_eq!(
        super::cline::sessions_root(Path::new("/home/user")),
        Path::new("/home/user/.cline/data/sessions")
    );
    // This test process's parent is a real pid.
    #[cfg(target_os = "linux")]
    {
        let parent = super::cline::parent_pid(std::process::id()).expect("/proc is readable");
        assert!(parent > 0, "{parent}");
        assert_eq!(
            super::cline::parent_pid(u32::MAX - 1),
            None,
            "no such process"
        );
    }
}
