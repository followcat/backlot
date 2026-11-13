//! A Unix-socket tap between hc-tui and a core for the float e2e.
//!
//! The wire has no request log, so "exactly one `terminal.kill`" is
//! proven by sitting on the socket: each accepted connection is
//! forwarded byte-for-byte both ways while the client→core direction is
//! scanned for JSON-RPC requests. After the one line-mode hello the
//! wire is framed; anything that fails to parse ends the scan for that
//! connection (line-mode connections never carry `terminal.kill`).
//! Byte forwarding itself stays raw — a parse failure can only end the
//! scan, never corrupt the stream.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use hc_proto::frame::{CONTROL_CHANNEL, HEADER_LEN, Header, Kind};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

/// Every `(method, params.terminal)` request the TUI has sent, in order.
type Calls = Arc<Mutex<Vec<(String, String)>>>;

/// The listening tap: accepts, forwards, and records calls.
pub struct Tap {
    pub socket: PathBuf,
    calls: Calls,
    task: tokio::task::JoinHandle<()>,
}

impl Tap {
    /// Listens on `socket`, forwarding every connection to `target`.
    pub fn start(socket: PathBuf, target: PathBuf) -> Tap {
        let listener = UnixListener::bind(&socket).expect("tap bind");
        let calls: Calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let task = tokio::spawn(async move {
            while let Ok((conn, _)) = listener.accept().await {
                match UnixStream::connect(&target).await {
                    Ok(real) => {
                        tokio::spawn(forward(conn, real, seen.clone()));
                    }
                    Err(_) => drop(conn),
                }
            }
        });
        Tap {
            socket,
            calls,
            task,
        }
    }

    /// Requests for `method` naming `terminal` — exact or machine-
    /// qualified (`host-b/t7`) — the TUI has sent so far.
    pub fn count(&self, method: &str, terminal: &str) -> usize {
        let qualified = format!("/{terminal}");
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, t)| m == method && (t == terminal || t.ends_with(&qualified)))
            .count()
    }

    /// The recorded `(method, terminal)` stream, for failure messages.
    pub fn dump(&self) -> Vec<(String, String)> {
        self.calls.lock().unwrap().clone()
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        self.task.abort();
        let _ = fs::remove_file(&self.socket);
    }
}

/// Forwards one tapped connection: core→client is a raw copy;
/// client→core additionally feeds the frame scanner.
async fn forward(conn: UnixStream, real: UnixStream, calls: Calls) {
    let (mut cr, mut cw) = conn.into_split();
    let (mut sr, mut sw) = real.into_split();
    tokio::spawn(async move {
        let _ = tokio::io::copy(&mut sr, &mut cw).await;
    });
    let mut buf: Vec<u8> = Vec::new();
    let mut framed = false;
    let mut scan = true;
    let mut chunk = [0u8; 16384];
    loop {
        let Ok(n) = cr.read(&mut chunk).await else {
            break;
        };
        if n == 0 {
            break;
        }
        if sw.write_all(&chunk[..n]).await.is_err() {
            break;
        }
        if !scan {
            continue;
        }
        buf.extend_from_slice(&chunk[..n]);
        if !framed {
            match buf.iter().position(|&b| b == b'\n') {
                Some(pos) => {
                    buf.drain(..=pos);
                    framed = true;
                }
                // A line longer than any sane hello means this connection
                // is not framed JSON-RPC at all — stop scanning it.
                None if buf.len() > 64 * 1024 => {
                    scan = false;
                    buf.clear();
                }
                None => continue,
            }
        }
        if framed {
            loop {
                if buf.len() < HEADER_LEN {
                    break;
                }
                let Ok(header) = Header::decode(buf[..HEADER_LEN].try_into().unwrap()) else {
                    scan = false;
                    buf.clear();
                    break;
                };
                if buf.len() < HEADER_LEN + header.len {
                    break;
                }
                if header.channel == CONTROL_CHANNEL
                    && header.kind == Kind::Data
                    && let Ok(v) = serde_json::from_slice::<serde_json::Value>(
                        &buf[HEADER_LEN..HEADER_LEN + header.len],
                    )
                    && let Some(method) = v.get("method").and_then(|m| m.as_str())
                {
                    let terminal = v
                        .pointer("/params/terminal")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();
                    calls.lock().unwrap().push((method.to_string(), terminal));
                }
                buf.drain(..HEADER_LEN + header.len);
            }
        }
    }
}
