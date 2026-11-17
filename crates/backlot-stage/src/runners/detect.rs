//! Where each runner's binary actually lives (U5). The wizard's
//! inherited PATH is only the first place to look: a core started
//! from a non-interactive ssh has `~/.local/bin` but none of the
//! nvm, volta or bun directories a login shell would add, so
//! `installed` alone reports the agents missing and a role launched
//! anyway dies "not found". The search order is the user's intent —
//! inherited PATH, then the login shell's own PATH, then the
//! well-known install directories — and it stops at the first hit,
//! even one whose `--version` then fails.
//!
//! Only `PATH`, `SHELL` and `HOME` are ever read from the
//! environment — `/proc/*/environ` never is — and no other
//! environment value is printed or copied anywhere.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::probe;
use crate::cli::{Env, Fail};

/// The runners `backlot runners` lists and the wizard's detection
/// walks — `fake` ships in `tools/`, never detected.
pub(crate) const RUNNERS: &[&str] = &[
    "claude", "codex", "devin", "cursor", "opencode", "cmd", "cline",
];

/// Every child gets three seconds: a login shell's start-up files
/// and a `--version` alike.
const PROBE: Duration = Duration::from_secs(3);

/// A child that answered inside the timeout.
pub struct RunOut {
    /// Its exit code — a signal reports as the shell's 128+n.
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Environment lookup — `PATH`, `SHELL`, `HOME` are all the
/// detector ever asks for.
type EnvGet<'a> = Box<dyn Fn(&str) -> Option<String> + 'a>;
/// Run `argv`, answer its output — `None` on timeout or a failed
/// spawn.
type Run<'a> = Box<dyn Fn(&[String], Duration) -> Option<RunOut> + 'a>;

/// The outside world the search touches, injected for tests:
/// environment values (`PATH`, `SHELL`, `HOME`) and the timeout
/// command runner. The real scope is [`scope`].
pub struct Scope<'a> {
    pub env: EnvGet<'a>,
    pub run: Run<'a>,
}

/// The real scope over the command's own environment.
pub fn scope<'a>(env: &'a Env) -> Scope<'a> {
    Scope {
        env: Box::new(move |k| env.get(k).map(str::to_string)),
        run: Box::new(run_timed),
    }
}

/// Where the hit came from — only `Inherited` needs nothing
/// written for a launch to find the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// On the wizard's inherited PATH.
    Inherited,
    /// Only on the login shell's PATH.
    Login,
    /// A well-known install directory.
    Known,
}

/// One runner's verdict.
#[derive(Debug)]
pub struct Found {
    /// The binary's absolute path.
    pub path: PathBuf,
    /// `<binary> --version`'s first line, escape sequences and
    /// control characters removed — `None` means the file exists
    /// but does not start ("not working").
    pub version: Option<String>,
    /// Where it was found.
    pub source: Source,
}

impl Found {
    /// A launch with only the inherited PATH cannot reach a
    /// login-shell or known-dir binary.
    pub fn needs_path(&self) -> bool {
        self.source != Source::Inherited
    }
}

/// The runner's binary at its first search hit — `None` when it is
/// nowhere, or the runner names no binary (`fake`, unknown names).
pub fn find(runner: &str, scope: &Scope) -> Option<Found> {
    let bin = probe::launch_program(runner)?;
    let hit = probe::find_on_path(bin, (scope.env)("PATH").as_deref())
        .map(|p| (p, Source::Inherited))
        .or_else(|| {
            login_path(scope)
                .and_then(|path| probe::find_on_path(bin, Some(&path)))
                .map(|p| (p, Source::Login))
        })
        .or_else(|| {
            known_dirs(scope)
                .into_iter()
                .map(|dir| dir.join(bin))
                .find(|f| probe::executable(f))
                .map(|p| (p, Source::Known))
        });
    hit.map(|(path, source)| Found {
        version: version(&path, scope),
        path,
        source,
    })
}

/// The login shell's PATH, printed between two marker lines so
/// profile noise never lands inside it. No `$SHELL` — no step.
fn login_path(scope: &Scope) -> Option<String> {
    let shell = (scope.env)("SHELL").filter(|s| !s.is_empty())?;
    let out = (scope.run)(&[shell, "-lc".to_string(), PATH_SCRIPT.to_string()], PROBE)?;
    path_between(&out.stdout)
}

/// The marker script: `$PATH` alone on its line between the two
/// marker lines.
const PATH_SCRIPT: &str = concat!(
    "printf '%s\\n' '@@BACKLOT_PATH@@'; ",
    "printf '%s\\n' \"$PATH\"; ",
    "printf '%s\\n' '@@BACKLOT_PATH@@'"
);

/// The PATH line between the two markers — `None` when the markers
/// never came (the shell failed, or it is not really a shell).
fn path_between(output: &str) -> Option<String> {
    const MARKER: &str = "@@BACKLOT_PATH@@";
    let mut inside = false;
    for line in output.lines() {
        if line.trim() == MARKER {
            // Adjacent markers mean an empty PATH — as good as none.
            if inside {
                return None;
            }
            inside = true;
            continue;
        }
        if inside {
            return Some(line.trim().to_string());
        }
    }
    None
}

/// The well-known install directories, in the brief's order.
fn known_dirs(scope: &Scope) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = (scope.env)("HOME").filter(|h| !h.is_empty()) {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/bin"));
        dirs.extend(nvm_bins(&home.join(".nvm/versions/node")));
        for d in [
            ".volta/bin",
            ".bun/bin",
            ".cargo/bin",
            ".npm-global/bin",
            ".deno/bin",
            ".local/share/mise/shims",
            ".asdf/shims",
        ] {
            dirs.push(home.join(d));
        }
    }
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs
}

/// `<nvm node dir>/*/bin`, a versioned directory first — newest
/// version ahead. Names that parse no version sort last, kept for
/// completeness.
fn nvm_bins(node: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(node) else {
        return Vec::new();
    };
    let mut dirs: Vec<((u64, u64, u64), PathBuf)> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .map(|p| (version_key(p.file_name()), p))
        .collect();
    dirs.sort_by_key(|a| std::cmp::Reverse(a.0));
    dirs.into_iter().map(|(_, p)| p.join("bin")).collect()
}

/// `v24.19.0` → `(24, 19, 0)` — each dot part's leading digits; a
/// name with none sorts behind every real release.
fn version_key(name: Option<&OsStr>) -> (u64, u64, u64) {
    let mut key = (0, 0, 0);
    let Some(name) = name.and_then(|n| n.to_str()) else {
        return key;
    };
    for (i, part) in name.trim_start_matches('v').split('.').take(3).enumerate() {
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        let n = digits.parse().unwrap_or(0);
        match i {
            0 => key.0 = n,
            1 => key.1 = n,
            _ => key.2 = n,
        }
    }
    key
}

/// `<binary> --version` inside the probe timeout: the first
/// nonempty line of its output — stdout, then stderr — cleaned of
/// escapes. A non-zero exit, a timeout or silence all mean "not
/// working".
fn version(binary: &Path, scope: &Scope) -> Option<String> {
    let out = (scope.run)(
        &[
            binary.to_string_lossy().into_owned(),
            "--version".to_string(),
        ],
        PROBE,
    )?;
    if out.code != 0 {
        return None;
    }
    out.stdout.lines().chain(out.stderr.lines()).find_map(|l| {
        let l = clean(l);
        (!l.is_empty()).then_some(l)
    })
}

/// Escape sequences and control characters out of a version line:
/// CSI `\x1b[…` spans to their final byte, OSC `\x1b]…` to BEL or
/// `\x1b\\`, a stray `\x1bX`, `\r` and the rest just dropped.
fn clean(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            match it.next() {
                Some('[') => {
                    for c in it.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    let mut esc = false;
                    for c in it.by_ref() {
                        if c == '\u{7}' || (esc && c == '\\') {
                            break;
                        }
                        esc = c == '\u{1b}';
                    }
                }
                _ => {}
            }
            continue;
        }
        if !c.is_control() {
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// `argv` with stdin closed and both pipes drained by reader
/// threads — a chatty child never wedges on a full pipe; the child
/// is killed at `timeout` and `None` answered, as on a failed
/// spawn.
pub fn run_timed(argv: &[String], timeout: Duration) -> Option<RunOut> {
    let (prog, args) = argv.split_first()?;
    let mut child = std::process::Command::new(prog)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    fn drain(mut r: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = r.read_to_end(&mut b);
            b
        })
    }
    let t_out = drain(child.stdout.take()?);
    let t_err = drain(child.stderr.take()?);
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = t_out.join();
                let _ = t_err.join();
                return None;
            }
        }
    };
    Some(RunOut {
        code: status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&t_out.join().unwrap_or_default()).into_owned(),
        stderr: String::from_utf8_lossy(&t_err.join().unwrap_or_default()).into_owned(),
    })
}

/// `path` under `home` shown as `~/…` — the table stays narrow.
fn tilde(home: Option<&Path>, path: &Path) -> String {
    if let Some(home) = home
        && let Ok(rest) = path.strip_prefix(home)
    {
        return PathBuf::from("~").join(rest).to_string_lossy().into_owned();
    }
    path.to_string_lossy().into_owned()
}

/// `backlot runners` (U5): every runner with where its binary was
/// found — or that it was not — its `--version` line, and whether
/// a launch needs the `[runner_env]` PATH write. Read-only: the
/// first thing to run when a role says "not found".
pub fn report(env: &Env) -> Result<String, Fail> {
    let scope = scope(env);
    let home = env.get("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    let mut rows = Vec::with_capacity(RUNNERS.len());
    for runner in RUNNERS {
        rows.push(match find(runner, &scope) {
            Some(f) => {
                let needs = if f.needs_path() { "yes" } else { "no" }.to_string();
                [
                    (*runner).to_string(),
                    if f.version.is_some() {
                        "found"
                    } else {
                        "not working"
                    }
                    .to_string(),
                    f.version.unwrap_or_else(|| "-".to_string()),
                    tilde(home.as_deref(), &f.path),
                    needs,
                ]
            }
            None => [
                (*runner).to_string(),
                "missing".to_string(),
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
            ],
        });
    }
    let header = ["runner", "status", "version", "path", "needs PATH"];
    let mut width = [0usize; 5];
    for (i, h) in header.iter().enumerate() {
        width[i] = h.len();
    }
    for row in &rows {
        for (i, c) in row.iter().enumerate() {
            width[i] = width[i].max(c.len());
        }
    }
    let mut out = String::new();
    let mut line = |cells: &[String; 5]| {
        for (i, c) in cells.iter().enumerate() {
            out.push_str(c);
            if i + 1 < cells.len() {
                out.push_str(&" ".repeat(width[i] - c.len() + 2));
            }
        }
        out.push('\n');
    };
    line(&header.map(String::from));
    for row in &rows {
        line(row);
    }
    Ok(out.trim_end().to_string())
}
