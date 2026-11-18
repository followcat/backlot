//! Opt-in real-core measurements for the cached-screen work: what a cold
//! open costs on the wire, what a warm switch costs, how long a 5000-line
//! first paint takes, and what ten cached panes hold in RSS.
//!
//! Ignored by default: `cargo test` never touches a real core. Run
//! explicitly with
//!
//! ```sh
//! HC_TUI_TEST_CORE=/path/to/pinned/herdr-core \
//!     cargo test -p hc-tui --test real_attach_cache -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The RSS case needs no core: it builds the VTs the client would keep.

#![cfg(unix)]

#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use hc_proto::client::{Attach, Client, Output};
use hc_proto::terminal::{AttachParams, RunParams};
use scratch::ScratchCore;

use std::time::{Duration, Instant};

/// Rows the fixture program prints: more than one cold attach can carry.
const LINES: usize = 6000;
/// What `[advanced] attach_history_lines` defaults to.
const HISTORY: u64 = 5000;
/// The pane grid the measurements attach at.
const COLS: u16 = 120;
const ROWS: u16 = 40;

async fn connect(scratch: &ScratchCore) -> Client {
    Client::connect(&scratch.endpoint, None::<&std::ffi::OsStr>)
        .await
        .expect("connect to scratch core")
}

/// A live terminal holding `LINES` numbered rows of scrollback.
async fn fill_history(scratch: &ScratchCore, client: &Client) -> String {
    let name = "cache-fill";
    client
        .terminal_run(RunParams {
            argv: vec![
                "sh".into(),
                "-c".into(),
                format!(
                    "i=0; while [ $i -lt {LINES} ]; do \
                     printf 'line %05d padded padded padded padded padded\\n' $i; \
                     i=$((i+1)); done; exec sleep 600"
                ),
            ],
            name: Some(name.into()),
            cwd: Some(scratch.work.to_string_lossy().into_owned()),
            ..RunParams::default()
        })
        .await
        .expect("terminal.run");
    name.into()
}

/// Bytes the core sends for the first paint of `terminal`, when the first
/// byte arrived, and when the paint completed.
async fn first_paint(
    scratch: &ScratchCore,
    terminal: &str,
    history: u64,
) -> (Vec<u8>, Duration, Duration) {
    let params = AttachParams {
        terminal: terminal.into(),
        observe: true,
        resize_notifications: true,
        quiet_human_release: false,
        history,
        cols: COLS,
        rows: ROWS,
        display: None,
        resumed: false,
        caller: None,
    };
    let started = Instant::now();
    let attach = Attach::connect(&scratch.endpoint, None, &params)
        .await
        .expect("terminal.attach");
    let mut paint = Vec::new();
    let mut first = None;
    let mut complete = started.elapsed();
    // The paint arrives in transport-sized chunks; the fixture program is
    // asleep, so a short quiet period means the paint is complete.
    loop {
        match tokio::time::timeout(Duration::from_millis(300), attach.recv()).await {
            Ok(Some(Output::Data(data))) => {
                let len = data.len();
                paint.extend_from_slice(&data);
                attach.consume(len).expect("credit");
                first.get_or_insert_with(|| started.elapsed());
                complete = started.elapsed();
            }
            Ok(Some(Output::Resize { .. })) => {}
            Ok(Some(_)) | Ok(None) | Err(_) => break,
        }
    }
    (paint, first.unwrap_or(complete), complete)
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn a_cold_open_carries_history_and_a_warm_switch_carries_the_screen() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let terminal = fill_history(&scratch, &client).await;

    // Cold: the configured history.
    let (cold, cold_first, cold_done) = first_paint(&scratch, &terminal, HISTORY).await;
    let cold_bytes = cold.len();
    let cold_lines = cold.iter().filter(|b| **b == b'\n').count();
    // The client's share: feeding that paint into a fresh VT.
    let mut vt = hc_vt::Vt::new(COLS, ROWS, 10_000_000);
    let feed = Instant::now();
    vt.feed(&cold);
    let cold_feed = feed.elapsed();
    let cold_rows = vt.total_rows().unwrap();
    drop(vt);
    // Warm: the same terminal, screen only — what a cached switch costs.
    let (warm, warm_first, warm_done) = first_paint(&scratch, &terminal, 0).await;
    let warm_bytes = warm.len();
    let warm_lines = warm.iter().filter(|b| **b == b'\n').count();

    println!(
        "cold open (history {HISTORY}): {cold_bytes} bytes, ~{cold_lines} newlines, \
         first byte {cold_first:?}, complete {cold_done:?}, VT feed {cold_feed:?}, {cold_rows} rows"
    );
    println!(
        "warm switch (history 0):      {warm_bytes} bytes, ~{warm_lines} newlines, \
         first byte {warm_first:?}, complete {warm_done:?}"
    );
    assert!(
        cold_bytes > warm_bytes * 10,
        "a cold open must dwarf a warm switch on the wire"
    );
    assert!(
        cold_lines > HISTORY as usize / 2,
        "the cold paint must carry most of the requested history"
    );
    assert!(
        warm_lines <= (ROWS as usize) * 4,
        "the warm paint must be about one screen"
    );
}

/// Ten cached panes at the default budget, each holding 5000 rows of
/// 150-column coloured output: the memory the cache costs.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "a measurement, not a gate"]
fn ten_cached_panes_with_5000_coloured_lines() {
    const BUDGET: usize = 10_000_000;
    fn rss_kib() -> u64 {
        std::fs::read_to_string("/proc/self/status")
            .expect("/proc/self/status")
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|kib| kib.parse().ok())
            .expect("VmRSS")
    }
    // The line shape the user described: a wide agent transcript.
    let mut line = Vec::new();
    for column in 0..150 {
        let hue = 31 + column % 6;
        line.extend_from_slice(format!("\x1b[{hue}mx").as_bytes());
    }
    line.extend_from_slice(b"\x1b[0m\r\n");

    let before = rss_kib();
    let mut panes: Vec<hc_vt::Vt> = Vec::new();
    let started = Instant::now();
    for _ in 0..10 {
        let mut vt = hc_vt::Vt::new(150, 40, BUDGET);
        for _ in 0..5000 {
            vt.feed(&line);
        }
        panes.push(vt);
    }
    let took = started.elapsed();
    let after = rss_kib();
    let retained: usize = panes.iter_mut().map(|vt| vt.total_rows().unwrap()).sum();
    println!(
        "10 panes × 5000 lines × 150 coloured columns: RSS {} -> {} MiB (+{} MiB), {retained} rows, {took:?}",
        before / 1024,
        after / 1024,
        (after.saturating_sub(before)) / 1024
    );
    // Ten panes of at most 10 MB of scrollback plus their grids; far past
    // that would mean the configured budget is not being enforced.
    assert!(
        after.saturating_sub(before) < 10 * 20 * 1024,
        "the configured budget must bound ten panes"
    );
}
