//! Endpoint discovery and remote-command tests: a table of injected
//! environments against a fake filesystem. Nothing here reads the real
//! environment, config or runtime dir, and nothing connects.

use hc_proto::endpoint::{Endpoint, MAX_SOCKET_PATH, Platform, remote, resolve, session};
use hc_proto::jsonrpc::RpcError;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

/// The filesystem discovery may ask about: which paths are directories, what
/// `config.toml` contains. Everything else is `NotFound`.
#[derive(Default)]
struct Fake {
    dirs: Vec<PathBuf>,
    files: BTreeMap<PathBuf, String>,
}

impl Platform for Fake {
    fn run_user_dir(&self) -> Option<PathBuf> {
        let path = PathBuf::from("/run/user/4242");
        self.dirs.contains(&path).then_some(path)
    }
    fn read_file(&self, path: &Path) -> io::Result<String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }
}

fn mk_env(pairs: &[(&str, &str)]) -> hc_proto::endpoint::Env {
    pairs
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect()
}

fn resolve_local(
    env: &hc_proto::endpoint::Env,
    arg: Option<&OsStr>,
    platform: &dyn Platform,
) -> PathBuf {
    match resolve(env, arg, None, platform).unwrap() {
        Endpoint::Local(l) => l.socket,
        Endpoint::Remote(_) => panic!("expected local"),
    }
}

#[test]
fn session_trim_default_and_override() {
    let empty = mk_env(&[]);
    assert_eq!(session(&empty, None).unwrap(), "default");
    assert_eq!(
        session(&mk_env(&[("HC_SESSION", "  ")]), None).unwrap(),
        "default"
    );
    assert_eq!(
        session(&mk_env(&[("HC_SESSION", "work")]), None).unwrap(),
        "work"
    );
    assert_eq!(
        session(&mk_env(&[("HC_SESSION", "smoke ")]), None).unwrap(),
        "smoke",
        "cmd's set keeps the space"
    );
    assert_eq!(
        session(&mk_env(&[("HC_SESSION", "MyTest")]), None).unwrap(),
        "MyTest"
    );
    // --session wins over HC_SESSION.
    assert_eq!(
        session(
            &mk_env(&[("HC_SESSION", "env-session")]),
            Some(OsStr::new("arg-session"))
        )
        .unwrap(),
        "arg-session"
    );
    // Names a path can't hold are refused.
    for bad in ["a/b", r"a\b", "a:b", "..", ".", "a\tb"] {
        assert!(session(&empty, Some(OsStr::new(bad))).is_err(), "{bad:?}");
    }
}

#[test]
fn hc_socket_wins_over_everything() {
    let platform = Fake {
        dirs: vec![PathBuf::from("/run/user/4242")],
        ..Fake::default()
    };
    let e = mk_env(&[
        ("HOME", "/home/user"),
        ("XDG_RUNTIME_DIR", "/run/user/4242"),
        ("HC_SOCKET", "/tmp/custom.sock"),
    ]);
    assert_eq!(
        resolve_local(&e, None, &platform),
        PathBuf::from("/tmp/custom.sock")
    );
    // An empty HC_SOCKET is ignored, like core.
    let e = mk_env(&[("XDG_RUNTIME_DIR", "/run/user/4242"), ("HC_SOCKET", "")]);
    assert_eq!(
        resolve_local(&e, None, &platform),
        PathBuf::from("/run/user/4242/herdr-core/default.sock")
    );
}

#[test]
fn config_socket_beats_runtime_and_broken_config_falls_back() {
    let dirs = vec![PathBuf::from("/run/user/4242")];
    let config_dir = PathBuf::from("/home/user/.config/herdr-core");
    // A valid socket key wins over the runtime dir.
    let platform = Fake {
        dirs: dirs.clone(),
        files: [(
            config_dir.join("config.toml"),
            "socket = \"/var/run/hc.sock\"\n".into(),
        )]
        .into(),
    };
    let e = mk_env(&[
        ("HOME", "/home/user"),
        ("XDG_RUNTIME_DIR", "/run/user/4242"),
    ]);
    assert_eq!(
        resolve_local(&e, None, &platform),
        PathBuf::from("/var/run/hc.sock")
    );
    // Garbage TOML is ignored for endpoint purposes.
    let platform = Fake {
        dirs,
        files: [(config_dir.join("config.toml"), "not [[toml".into())].into(),
    };
    assert_eq!(
        resolve_local(&e, None, &platform),
        PathBuf::from("/run/user/4242/herdr-core/default.sock")
    );
}

#[test]
fn semantically_invalid_config_drops_the_socket_like_core() {
    let dirs = vec![PathBuf::from("/run/user/4242")];
    let config_dir = PathBuf::from("/home/user/.config/herdr-core");
    let e = mk_env(&[
        ("HOME", "/home/user"),
        ("XDG_RUNTIME_DIR", "/run/user/4242"),
    ]);
    let fallback = PathBuf::from("/run/user/4242/herdr-core/default.sock");
    // A known field with the wrong type fails core's Config::load, so core
    // ignores the socket override too — we must parse the same shape.
    for config in [
        "notes = \"oops\"\nsocket = \"/var/run/hc.sock\"\n",
        "shell = \"nu\"\nsocket = \"/var/run/hc.sock\"\n",
    ] {
        let platform = Fake {
            dirs: dirs.clone(),
            files: [(config_dir.join("config.toml"), config.into())].into(),
        };
        assert_eq!(
            resolve_local(&e, None, &platform),
            fallback,
            "{config:?} must fail validation and drop the socket"
        );
    }
    // A key core does not know is tolerated, as core tolerates it.
    let platform = Fake {
        dirs,
        files: [(
            config_dir.join("config.toml"),
            "future_key = true\nsocket = \"/var/run/hc.sock\"\n".into(),
        )]
        .into(),
    };
    assert_eq!(
        resolve_local(&e, None, &platform),
        PathBuf::from("/var/run/hc.sock")
    );
}

#[test]
fn runtime_fallbacks_in_order() {
    let home_env = mk_env(&[("HOME", "/home/user")]);
    // 1. XDG_RUNTIME_DIR/herdr-core
    let socket = resolve_local(
        &mk_env(&[("XDG_RUNTIME_DIR", "/tmp/xrt"), ("HOME", "/home/user")]),
        None,
        &Fake::default(),
    );
    assert_eq!(socket, PathBuf::from("/tmp/xrt/herdr-core/default.sock"));
    // 2. /run/user/<uid>/herdr-core when that dir exists
    let platform = Fake {
        dirs: vec![PathBuf::from("/run/user/4242")],
        ..Fake::default()
    };
    assert_eq!(
        resolve_local(&home_env, None, &platform),
        PathBuf::from("/run/user/4242/herdr-core/default.sock")
    );
    // 3. the session state dir under XDG_STATE_HOME
    let e = mk_env(&[("XDG_STATE_HOME", "/tmp/state"), ("HOME", "/home/user")]);
    assert_eq!(
        resolve_local(&e, None, &Fake::default()),
        PathBuf::from("/tmp/state/herdr-core/default/default.sock")
    );
    // 4. HOME/.local/state as the last resort; missing HOME reads as "/"
    assert_eq!(
        resolve_local(&home_env, None, &Fake::default()),
        PathBuf::from("/home/user/.local/state/herdr-core/default/default.sock")
    );
    assert_eq!(
        resolve_local(&mk_env(&[]), None, &Fake::default()),
        PathBuf::from("/.local/state/herdr-core/default/default.sock")
    );
    // The session name reaches the socket name.
    assert_eq!(
        resolve_local(
            &mk_env(&[("XDG_RUNTIME_DIR", "/tmp/xrt")]),
            Some(OsStr::new("acme")),
            &Fake::default()
        ),
        PathBuf::from("/tmp/xrt/herdr-core/acme.sock")
    );
}

#[test]
fn holder_socket_and_path_length() {
    // Holder sits beside the chosen socket; the bound uses its longest name.
    let platform = Fake::default();
    let e = mk_env(&[("HC_SOCKET", "/tmp/s.sock")]);
    let Endpoint::Local(l) = resolve(&e, None, None, &platform).unwrap() else {
        panic!("expected local");
    };
    assert_eq!(l.holder_socket, PathBuf::from("/tmp/s.holder.sock"));
    // A path that leaves no room for the holder id fails as usage.
    let long = "a".repeat(MAX_SOCKET_PATH - 4);
    let e = mk_env(&[("XDG_RUNTIME_DIR", "/"), ("HC_SESSION", long.as_str())]);
    let err = resolve(&e, None, None, &platform).unwrap_err();
    assert_eq!(err.kind(), Some("usage"));
}

#[cfg(unix)]
#[test]
fn non_utf8_env_values_are_preserved() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let mut env = mk_env(&[]);
    env.insert(
        OsString::from("HC_SOCKET"),
        OsString::from_vec(b"/tmp/bad\xff.sock".to_vec()),
    );
    let Endpoint::Local(l) = resolve(&env, None, None, &Fake::default()).unwrap() else {
        panic!("expected local");
    };
    assert_eq!(l.socket.as_os_str().as_bytes(), b"/tmp/bad\xff.sock");
}

#[test]
fn remote_target_validation() {
    for good in [
        "host-b",
        "alice@host-b",
        "alice@host-b:2222",
        "ssh://alice@host-b:2222",
        "[2001:db8::1]",
        "alice@[2001:db8::1]",
        "my-alias",
    ] {
        assert!(remote(good, "default").is_ok(), "{good:?}");
    }
    for bad in [
        "",
        "-oProxyCommand=evil",
        "-J jump",
        "two words",
        "tab\ttarget",
        "new\nline",
    ] {
        assert!(remote(bad, "default").is_err(), "{bad:?}");
    }
    // A remote endpoint skips all local rules.
    let e = mk_env(&[
        ("HC_SOCKET", "/tmp/ignored.sock"),
        ("XDG_RUNTIME_DIR", "/tmp/ignored"),
    ]);
    let Endpoint::Remote(r) = resolve(&e, None, Some("host-b"), &Fake::default()).unwrap() else {
        panic!("expected remote");
    };
    assert_eq!(r.target, "host-b");
}

#[test]
fn remote_line_and_quoting() {
    // The normalized session always reaches the remote command, so a remote
    // HC_SESSION can never silently pick a different core.
    assert_eq!(
        remote("host-b", "default").unwrap().line(),
        "hc connect --session 'default'"
    );
    assert_eq!(
        remote("host-b", "work").unwrap().line(),
        "hc connect --session 'work'"
    );
    assert_eq!(
        remote("host-b", "my work").unwrap().line(),
        "hc connect --session 'my work'",
        "spaces stay inside one quoted word"
    );
    assert_eq!(
        remote("host-b", "o'clock").unwrap().line(),
        r"hc connect --session 'o'\''clock'",
        "an apostrophe is closed, escaped, reopened"
    );
    assert_eq!(
        remote("host-b", "a$(id)").unwrap().line(),
        "hc connect --session 'a$(id)'",
        "metacharacters stay literal inside quotes"
    );
}

#[test]
fn explicit_default_session_survives_remote_resolution() {
    // An explicit `--session default` must pin the remote session too:
    // the remote environment is outside this layer's control.
    for arg in ["default"].into_iter().map(Some).chain([None]) {
        let Endpoint::Remote(r) = resolve(
            &mk_env(&[]),
            arg.map(OsStr::new),
            Some("host-b"),
            &Fake::default(),
        )
        .unwrap() else {
            panic!("expected remote");
        };
        assert_eq!(r.line(), "hc connect --session 'default'");
    }
}

#[test]
fn remote_argv_is_batch_piped_and_unshellable() {
    let argv = remote("alice@host-b", "work").unwrap().argv();
    let argv: Vec<&OsStr> = argv.iter().map(OsString::as_os_str).collect();
    assert_eq!(
        argv,
        [
            OsStr::new("-T"),
            OsStr::new("-o"),
            OsStr::new("BatchMode=yes"),
            OsStr::new("-o"),
            OsStr::new("ConnectTimeout=10"),
            OsStr::new("-o"),
            OsStr::new("ServerAliveInterval=15"),
            OsStr::new("-o"),
            OsStr::new("ServerAliveCountMax=4"),
            OsStr::new("alice@host-b"),
            OsStr::new("hc connect --session 'work'"),
        ]
    );
    // The remote line is one argument; no local shell ever sees it.
}

#[test]
fn resolve_errors_keep_kind_and_hint() {
    let e = mk_env(&[]);
    let err: RpcError = resolve(&e, Some(OsStr::new("a/b")), None, &Fake::default()).unwrap_err();
    assert_eq!(err.kind(), Some("usage"));
    assert!(!err.hint().unwrap_or_default().is_empty());
}
