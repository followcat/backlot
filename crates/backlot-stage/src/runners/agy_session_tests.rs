//! agy's session discovery against fixture trees: the
//! `conversations/<uuid>.db` byte scan for older agy and the
//! `cache/last_conversations.json` cwd → conversation map agy
//! 1.5.14 keeps beside it.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::agy::session_at;
use crate::test_support::TestDir;

fn tmp(tag: &str) -> TestDir {
    TestDir::new(&format!("bl-agy-session-{tag}"))
}

/// A file's birth time, or its mtime where birth is unsupported —
/// the same clock `session_at` reads.
fn file_time(path: &Path) -> u64 {
    let md = fs::metadata(path).unwrap();
    let t = md.created().or_else(|_| md.modified()).unwrap();
    t.duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// The session id a live agy role's launch minted: the newest
/// `<uuid>.db` under `conversations/` created at or after the
/// role's start whose bytes name `file://<cwd>` — no other role's
/// id, nothing older, nothing from another workdir.
#[test]
fn session_at_finds_the_roles_conversation_db() {
    let dir = tmp("sessions");
    let conv = dir.join("conversations");
    fs::create_dir_all(&conv).unwrap();
    let started_ms = 1_000_000;
    let db = |name: &str, cwd: &str| {
        let p = conv.join(format!("{name}.db"));
        fs::write(&p, format!("sqlite-ish file://{cwd}/ blob")).unwrap();
        p
    };
    // A db for the role's cwd, minted at start — the hit.
    let mine = db(
        "aaaaaaaa-0000-0000-0000-000000000001",
        "/home/user/acme-shop",
    );
    // Another workdir's db and an untagged one are skipped.
    db("bbbbbbbb-0000-0000-0000-000000000002", "/home/user/other");
    fs::write(
        conv.join("cccccccc-0000-0000-0000-000000000003.db"),
        "no uri",
    )
    .unwrap();
    // A non-db file is skipped.
    fs::write(conv.join("notes.txt"), "file:///home/user/acme-shop").unwrap();
    let id = session_at(&conv, "/home/user/acme-shop", started_ms, &BTreeSet::new()).unwrap();
    assert_eq!(id, "aaaaaaaa-0000-0000-0000-000000000001");
    // Already claimed by another role — none left.
    let taken = BTreeSet::from([id.clone()]);
    assert_eq!(
        session_at(&conv, "/home/user/acme-shop", started_ms, &taken),
        None
    );
    // A far-future start finds nothing.
    let far = file_time(&mine) + 10_000_000;
    assert_eq!(
        session_at(&conv, "/home/user/acme-shop", far, &BTreeSet::new()),
        None
    );
    // A different cwd finds its own.
    assert_eq!(
        session_at(&conv, "/home/user/other", started_ms, &BTreeSet::new()),
        Some("bbbbbbbb-0000-0000-0000-000000000002".to_string())
    );
}

/// `file://` URIs percent-encode what a path cannot hold: a cwd
/// with a space must still match — the db writes `%20`, the match
/// encodes the cwd the same way.
#[test]
fn session_at_matches_an_encoded_uri() {
    let dir = tmp("sessions-pct");
    let conv = dir.join("conversations");
    fs::create_dir_all(&conv).unwrap();
    fs::write(
        conv.join("dddddddd-0000-0000-0000-000000000004.db"),
        "blob file:///home/user/acme%20shop/blob",
    )
    .unwrap();
    assert_eq!(
        session_at(&conv, "/home/user/acme shop", 0, &BTreeSet::new()),
        Some("dddddddd-0000-0000-0000-000000000004".to_string())
    );
}

/// A temp `antigravity-cli` tree: the conversations dir the scan
/// reads, and the `cache/last_conversations.json` agy 1.5.14 keeps
/// beside it (written only when `map` is given).
fn tree(tag: &str, map: Option<&str>) -> (TestDir, std::path::PathBuf) {
    let dir = tmp(tag);
    let conv = dir.join("antigravity-cli/conversations");
    fs::create_dir_all(&conv).unwrap();
    if let Some(json) = map {
        let cache = dir.join("antigravity-cli/cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("last_conversations.json"), json).unwrap();
    }
    (dir, conv)
}

/// A `<uuid>.db` holding `bytes`.
fn conversation_db(conv: &Path, id: &str, bytes: &str) -> std::path::PathBuf {
    let p = conv.join(format!("{id}.db"));
    fs::write(&p, bytes).unwrap();
    p
}

/// agy 1.5.14's `cache/last_conversations.json` maps the role's cwd
/// to its conversation outright — the id is taken even when the db
/// holds no workspace URI at all, and it wins over a newer db the
/// needle scan would pick.
#[test]
fn session_at_takes_the_last_conversation_for_the_cwd() {
    let id = "eeeeeeee-0000-0000-0000-000000000005";
    let (dir, conv) = tree(
        "sessions-map",
        Some(&format!(
            "{{\"/home/user/acme-shop\": \"{id}\", \"/tmp\": \"ffffffff-0000-0000-0000-000000000006\"}}"
        )),
    );
    let mine = conversation_db(&conv, id, "sqlite-ish, no uri");
    // A newer db the old needle scan would prefer.
    conversation_db(
        &conv,
        "gggggggg-0000-0000-0000-000000000007",
        "blob file:///home/user/acme-shop/ blob",
    );
    assert_eq!(
        session_at(&conv, "/home/user/acme-shop", 0, &BTreeSet::new()).as_deref(),
        Some(id)
    );
    // Born at the role's start is still the role's.
    assert_eq!(
        session_at(
            &conv,
            "/home/user/acme-shop",
            file_time(&mine),
            &BTreeSet::new()
        )
        .as_deref(),
        Some(id)
    );
    drop(dir);
}

/// The map's id is never taken when it is already claimed, and
/// never when its db predates the role's start — the needle scan
/// decides in both cases; unclaimed and new, the map's id wins.
#[test]
fn session_at_ignores_a_mapped_id_that_is_claimed_or_too_old() {
    let gone = "hhhhhhhh-0000-0000-0000-000000000008";
    let (dir, conv) = tree(
        "sessions-map-taken",
        Some(&format!("{{\"/home/user/acme-shop\": \"{gone}\"}}")),
    );
    conversation_db(&conv, gone, "no uri at all");
    // Claimed: the needle-matching db is what the fallback finds.
    conversation_db(
        &conv,
        "iiiiiiii-0000-0000-0000-000000000009",
        "blob file:///home/user/acme-shop/ blob",
    );
    assert_eq!(
        session_at(
            &conv,
            "/home/user/acme-shop",
            0,
            &BTreeSet::from([gone.to_string()])
        )
        .as_deref(),
        Some("iiiiiiii-0000-0000-0000-000000000009")
    );
    // Unclaimed, the map's id is the answer even though the db
    // holds no URI at all.
    assert_eq!(
        session_at(&conv, "/home/user/acme-shop", 0, &BTreeSet::new()).as_deref(),
        Some(gone)
    );
    drop(dir);

    // Its db from before the role's start is not the role's either,
    // whatever the map says; a db born after the start still is.
    let old = "oooooooo-0000-0000-0000-000000000015";
    let (dir, conv) = tree(
        "sessions-map-old",
        Some(&format!("{{\"/home/user/acme-shop\": \"{old}\"}}")),
    );
    let mapped = conversation_db(&conv, old, "no uri at all");
    std::thread::sleep(std::time::Duration::from_millis(20));
    conversation_db(
        &conv,
        "pppppppp-0000-0000-0000-000000000016",
        "blob file:///home/user/acme-shop/ blob",
    );
    let started = file_time(&mapped) + 1;
    assert_eq!(
        session_at(&conv, "/home/user/acme-shop", started, &BTreeSet::new()).as_deref(),
        Some("pppppppp-0000-0000-0000-000000000016")
    );
    // With no needle match left, the old mapped id stays unused.
    let only = conv.join("pppppppp-0000-0000-0000-000000000016.db");
    fs::write(&only, "no uri either").unwrap();
    assert_eq!(
        session_at(&conv, "/home/user/acme-shop", started, &BTreeSet::new()),
        None
    );
    drop(dir);
}

/// A map that names other cwds only says nothing about the role's
/// own: the needle scan picks it, as on older agy — while an entry
/// for the cwd it does name is that cwd's answer.
#[test]
fn session_at_falls_back_when_the_map_names_other_cwds() {
    let (dir, conv) = tree(
        "sessions-map-other",
        Some(
            "{\"/home/user/other\": \"jjjjjjjj-0000-0000-0000-000000000010\", \
             \"/tmp\": \"kkkkkkkk-0000-0000-0000-000000000011\"}",
        ),
    );
    conversation_db(
        &conv,
        "llllllll-0000-0000-0000-000000000012",
        "blob file:///home/user/acme-shop/blob",
    );
    conversation_db(&conv, "jjjjjjjj-0000-0000-0000-000000000010", "other");
    assert_eq!(
        session_at(&conv, "/home/user/acme-shop", 0, &BTreeSet::new()).as_deref(),
        Some("llllllll-0000-0000-0000-000000000012")
    );
    assert_eq!(
        session_at(&conv, "/home/user/other", 0, &BTreeSet::new()).as_deref(),
        Some("jjjjjjjj-0000-0000-0000-000000000010")
    );
    drop(dir);
}

/// A malformed `last_conversations.json` is no answer from the map —
/// never a panic — and the needle scan still decides; a valid map
/// written in its place is read again.
#[test]
fn session_at_survives_a_malformed_map() {
    for (i, json) in [
        "{not json",
        "[\"an\", \"array\"]",
        "{\"/home/user/acme-shop\": 7}",
    ]
    .into_iter()
    .enumerate()
    {
        let (dir, conv) = tree(&format!("sessions-map-bad{i}"), Some(json));
        conversation_db(
            &conv,
            "mmmmmmmm-0000-0000-0000-000000000013",
            "blob file:///home/user/acme-shop/blob",
        );
        assert_eq!(
            session_at(&conv, "/home/user/acme-shop", 0, &BTreeSet::new()).as_deref(),
            Some("mmmmmmmm-0000-0000-0000-000000000013"),
            "{json}"
        );
        drop(dir);
    }
    // No readable map at all — older agy — is the same story.
    let (dir, conv) = tree("sessions-map-none", None);
    let id = "nnnnnnnn-0000-0000-0000-000000000014";
    conversation_db(&conv, id, "blob file:///home/user/acme-shop/blob");
    assert_eq!(
        session_at(&conv, "/home/user/acme-shop", 0, &BTreeSet::new()).as_deref(),
        Some(id)
    );
    // A valid map written in its place is read: with the URI gone
    // from the db only the map can name the conversation.
    fs::create_dir_all(dir.join("antigravity-cli/cache")).unwrap();
    fs::write(
        dir.join("antigravity-cli/cache/last_conversations.json"),
        format!("{{\"/home/user/acme-shop\": \"{id}\"}}"),
    )
    .unwrap();
    conversation_db(&conv, id, "no uri at all");
    assert_eq!(
        session_at(&conv, "/home/user/acme-shop", 0, &BTreeSet::new()).as_deref(),
        Some(id)
    );
}

/// A map value that is not a plain stem names no conversation: a
/// separator or a `..` segment would join outside `conversations/`,
/// so it is no answer and the needle scan decides. The file a naive
/// join would reach is new, exists and holds no URI, so only the
/// guard can reject it.
#[test]
fn session_at_refuses_a_mapped_id_that_is_a_path() {
    let needle = "ssssssss-0000-0000-0000-000000000018";
    for (i, bad) in [
        "../escape",
        "../../escape",
        "nested/inside",
        "back\\slash",
        "..",
    ]
    .into_iter()
    .enumerate()
    {
        let (dir, conv) = tree(
            &format!("sessions-map-path{i}"),
            Some(&format!("{{\"/home/user/acme-shop\": \"{bad}\"}}")),
        );
        // Exactly the path `conversations.join(format!("{id}.db"))`
        // would reach without the guard.
        let escaped = conv.join(format!("{bad}.db"));
        fs::create_dir_all(escaped.parent().unwrap()).unwrap();
        fs::write(&escaped, "no uri at all").unwrap();
        conversation_db(&conv, needle, "blob file:///home/user/acme-shop/blob");
        assert_eq!(
            session_at(&conv, "/home/user/acme-shop", 0, &BTreeSet::new()).as_deref(),
            Some(needle),
            "{bad} must not be taken"
        );
        drop(dir);
    }
}
