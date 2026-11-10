//! The matrix cell machinery: a `Ctx` holding the TUI plus the pane
//! table, and the copy/switch/paste helpers every cell composes. Shared
//! by the local and remote matrix tests.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hc_proto::client::Client;

use crate::fixture::prefix;
use crate::pty::Tui;

/// Runs one matrix cell: records pass/fail, then resets the UI. The
/// future may borrow `ctx` — the macro keeps borrows sequential.
#[macro_export]
macro_rules! cell {
    ($ctx:ident, $name:expr, $body:expr) => {{
        let name = $name.to_string();
        let verdict = match ($body).await {
            Ok(()) => "pass".to_string(),
            Err(e) => {
                eprintln!("screen on failure:\n{}", $ctx.tui.screen());
                format!("FAIL: {e}")
            }
        };
        eprintln!("{verdict:>6}  {name}");
        $ctx.results.push((name, verdict));
        $ctx.reset().await;
    }};
}

/// Everything a cell needs: the TUI, the pane table (`space, tab, tag,
/// spec`), where the fakes write, and the owned clipboard dir.
pub struct Ctx {
    pub tui: Tui,
    pub client: Client,
    pub panes: &'static [(&'static str, &'static str, &'static str, &'static str)],
    pub out: PathBuf,
    pub clip: PathBuf,
    pub results: Vec<(String, String)>,
    /// Tag focused after every cell.
    pub home_tag: &'static str,
    /// Flag file the popup program watches; creating it ends the popup.
    pub popup_flag: PathBuf,
}

impl Ctx {
    pub fn out(&self, tag: &str) -> PathBuf {
        self.out.join(format!("{tag}.bin"))
    }

    /// Back to Normal mode on the home pane with the popup closed, so the
    /// next cell starts from a known state.
    pub async fn reset(&mut self) {
        // The popup program exits when its flag file appears.
        if !self.popup_flag.as_os_str().is_empty() {
            let _ = fs::write(&self.popup_flag, b"");
        }
        self.tui.type_keys(b"\x1b");
        tokio::time::sleep(Duration::from_millis(250)).await;
        self.tui.pump();
        let _ = self.focus(self.home_tag).await;
        if !self.popup_flag.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.popup_flag);
        }
        self.tui.clear_raw();
    }

    /// Focuses `tag`'s pane: navigates through its workspace and tab
    /// rows when the marker is off-screen, then clicks the marker and
    /// confirms focus by landing a unique probe in the output file.
    pub async fn focus(&mut self, tag: &str) -> Result<(), String> {
        let (space, tab) = self
            .panes
            .iter()
            .find(|(_, _, t, _)| t == &tag)
            .map(|(s, t, _, _)| (*s, *t))
            .unwrap_or(("", ""));
        for _ in 0..6 {
            if self.tui.find_text(&format!("MARK {tag} ")).is_none() {
                for label in [space, tab] {
                    if label.is_empty() {
                        continue;
                    }
                    if let Some((c, r)) = self.tui.find_text(label) {
                        self.tui.click(c, r);
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        self.tui.pump();
                    }
                    if self.tui.find_text(&format!("MARK {tag} ")).is_some() {
                        break;
                    }
                }
            }
            if let Some((col, row)) = self.tui.find_text(&format!("MARK {tag} ")) {
                self.tui.click(col + 2, row);
                if self.probe_lands(tag, Duration::from_secs(2)).await {
                    return Ok(());
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            self.tui.pump();
        }
        Err(format!("could not focus {tag}"))
    }

    /// Types a unique probe and waits for it in `tag`'s output file.
    async fn probe_lands(&mut self, tag: &str, timeout: Duration) -> bool {
        let out = self.out(tag);
        let probe = format!(
            "probe-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        self.tui.type_keys(format!("{probe}\n").as_bytes());
        file_gains(&mut self.tui, &out, probe.as_bytes(), None, timeout).await
    }

    /// Pastes `text` as the host's bracketed paste, then asserts the
    /// exact wire bytes land in `tag`'s file — wrapped for a `?2004`
    /// program, `\n`→`\r` rewritten raw for one without it (ghostty's
    /// paste encoder; herdr wraps by hand and passes `\n` verbatim —
    /// same effect on canonical readers, a byte difference only for
    /// raw-mode programs without bracketed paste).
    pub async fn assert_paste(&mut self, tag: &str, text: &str, wrap: bool) -> Result<(), String> {
        let out = self.out(tag);
        let mark = fs::read(&out).map(|b| b.len()).unwrap_or(0);
        self.tui.paste_text(text);
        let expect = if wrap {
            format!("\x1b[200~{text}\x1b[201~")
        } else {
            text.replace('\n', "\r")
        };
        if file_gains(
            &mut self.tui,
            &out,
            expect.as_bytes(),
            Some(mark),
            Duration::from_secs(8),
        )
        .await
        {
            return Ok(());
        }
        let got = fs::read(&out).unwrap_or_default();
        Err(format!(
            "expected {expect:?} appended to {tag}; tail was {:?}",
            String::from_utf8_lossy(&got[mark.min(got.len())..])
        ))
    }

    /// The Ctrl+V image-paste path: the bridge uploads the clipboard
    /// image and pastes the returned remote path into the focused pane.
    /// Asserts a `/…/clipboard-…` path lands in `tag`'s file, the file
    /// exists, and it holds `bytes`.
    pub async fn assert_image_paste(&mut self, tag: &str, bytes: &[u8]) -> Result<(), String> {
        let out = self.out(tag);
        let mark = fs::read(&out).map(|b| b.len()).unwrap_or(0);
        crate::fixture::ctrl_v(&self.tui);
        let deadline = std::time::Instant::now() + Duration::from_secs(12);
        loop {
            let got = fs::read(&out).unwrap_or_default();
            let text = String::from_utf8_lossy(&got[mark.min(got.len())..]);
            if let Some(path) = text
                .split(['\x1b', '[', '~', '\r', '\n'])
                .find(|part| part.contains("clipboard-"))
            {
                let path = path.trim();
                if Path::new(path).exists() {
                    let uploaded =
                        fs::read(path).map_err(|e| format!("uploaded {path} unreadable: {e}"))?;
                    return (uploaded == bytes).then_some(()).ok_or_else(|| {
                        format!("uploaded {path} bytes differ from clipboard image")
                    });
                }
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!("no uploaded path reached {tag}; tail was {text:?}"));
            }
            self.tui.pump();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// Waits for `needle` to appear in `path` (optionally only in bytes
/// appended after `mark`), pumping the TUI meanwhile.
pub async fn file_gains(
    tui: &mut Tui,
    path: &Path,
    needle: &[u8],
    mark: Option<usize>,
    timeout: Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let bytes = fs::read(path).unwrap_or_default();
        let from = mark.unwrap_or(0).min(bytes.len());
        if bytes[from..].windows(needle.len()).any(|w| w == needle) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tui.pump();
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

/// Whether a fake wraps pasted input in `200~...201~` markers.
pub fn wrapped(spec: &str) -> bool {
    spec.split(',').any(|s| s == "2004")
}

/// Waits for `MARK <tag> abcdefgh` to be on screen — a focused pane can
/// show "Terminal loading" while its attach reconnects; the content
/// returns when the first paint lands.
async fn mark_at(tui: &mut Tui, tag: &str) -> Option<(u16, u16)> {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(pos) = tui.find_text(&format!("MARK {tag} abcdefgh")) {
            return Some(pos);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
}

/// Drags over the `MARK <tag> abcdefgh` line and returns the OSC 52
/// payload the app wrote to the (test-owned) terminal clipboard.
pub async fn copy_by_mouse(tui: &mut Tui, tag: &str) -> Result<Vec<u8>, String> {
    let (col, row) = mark_at(tui, tag)
        .await
        .ok_or_else(|| format!("no MARK {tag} on screen"))?;
    let end = col + format!("MARK {tag} abcdefgh").len() as u16 - 1;
    tui.take_osc52();
    tui.drag(col, row, end, row);
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    loop {
        let copies = tui.take_osc52();
        if let Some(last) = copies.last() {
            return Ok(last.clone());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("no OSC52 after selecting in {tag}"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Copy mode: `/` search the marker, `V` line-select, `y` copy.
pub async fn copy_by_copy_mode(tui: &mut Tui, tag: &str) -> Result<Vec<u8>, String> {
    prefix(tui, b'[');
    tokio::time::sleep(Duration::from_millis(250)).await;
    tui.pump();
    tui.type_keys(b"/");
    tui.type_keys(format!("MARK {tag} abcdefgh").as_bytes());
    tui.type_keys(b"\r");
    tokio::time::sleep(Duration::from_millis(120)).await;
    tui.type_keys(b"V");
    tui.take_osc52();
    tui.type_keys(b"y");
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    loop {
        let copies = tui.take_osc52();
        if let Some(last) = copies.last() {
            return Ok(last.clone());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("no OSC52 after copy-mode y in {tag}"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Clicks the sidebar row carrying `label`, then waits for the pane that
/// row's tab/workspace should show.
pub async fn switch_via_sidebar(tui: &mut Tui, label: &str, want_tag: &str) -> Result<(), String> {
    let (col, row) = tui
        .find_text(label)
        .ok_or_else(|| format!("no sidebar row {label}"))?;
    tui.click(col, row);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        tui.pump();
        if tui.find_text(&format!("MARK {want_tag} ")).is_some() {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("{label} click did not reveal {want_tag}"));
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

/// Prints the collected matrix table and returns the failure count.
pub fn report(results: &[(String, String)], title: &str) -> usize {
    println!("\n=== {title} ===");
    let mut failures = 0;
    for (name, verdict) in results {
        if verdict != "pass" {
            failures += 1;
        }
        println!("{verdict:>6}  {name}");
    }
    failures
}
