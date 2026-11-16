//! The Waiting worker's wire calls: `machine.exec` stdout arrives
//! base64, `file.read` returns plain `text`, and a relative
//! `file.read` resolves against the core's working directory — so the
//! machine's home is probed once per connection for the sessions
//! path. Every call carries a per-probe deadline and returns `None`
//! on any failure: a failed exec or read is not evidence either way.

use hc_proto::client::Client;
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Duration;

/// Per-call deadline for a probe: a stalled exec or read must not
/// hold the cycle open.
pub(super) const OP: Duration = Duration::from_secs(5);

/// Standard-alphabet base64 decode — `ExecResult.stdout` arrives
/// base64; kept local for the same reason `base64_encode` is.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a' + 26)),
            b'0'..=b'9' => Some(u32::from(byte - b'0' + 52)),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|b| **b == b'=').count();
        let data_len = chunk.len() - pad;
        // Two symbols make a byte; padding only ever ends the stream.
        if data_len < 2 || pad > 2 || (pad > 0 && chunk.len() != 4) {
            return None;
        }
        let n: u32 = chunk[..data_len]
            .iter()
            .try_fold(0u32, |acc, b| value(*b).map(|v| (acc << 6) | v))?;
        let shifted = n << (6 * (4 - data_len));
        out.extend(shifted.to_be_bytes()[1..data_len].iter());
    }
    Some(out)
}

/// `ExecResult.stdout` as text, `None` on any failure.
pub(super) async fn exec_stdout(
    conn: &Client,
    machine: &Option<String>,
    argv: &[&str],
) -> Option<String> {
    let result = conn
        .call_timeout(
            "machine.exec",
            Some(json!({"argv": argv, "on": machine, "timeout": OP.as_secs_f64()})),
            OP,
        )
        .await
        .ok()?;
    String::from_utf8(base64_decode(result["stdout"].as_str()?)?).ok()
}

/// `FileReadResult.text`, `None` on any failure — a missing sessions
/// file simply means this process has none.
pub(super) async fn read_file(
    conn: &Client,
    machine: &Option<String>,
    path: String,
) -> Option<String> {
    conn.call_timeout(
        "file.read",
        Some(json!({"path": path, "on": machine, "limit": 8192})),
        OP,
    )
    .await
    .ok()?
    .get("text")
    .and_then(|text| text.as_str())
    .map(str::to_owned)
}

/// The machine's home directory, probed once per connection.
pub(super) async fn home_of<'a>(
    conn: &Client,
    machine: &'a Option<String>,
    homes: &'a mut BTreeMap<Option<String>, Option<String>>,
) -> Option<&'a str> {
    if !homes.contains_key(machine) {
        let home = exec_stdout(conn, machine, &["sh", "-c", "printf %s \"$HOME\""])
            .await
            .map(|out| out.trim().to_owned())
            .filter(|out| !out.is_empty());
        homes.insert(machine.clone(), home);
    }
    homes.get(machine)?.as_deref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_through_the_local_encoder() {
        for bytes in [b"ps listing\n".as_slice(), b"", &[0u8; 7], &[255u8; 5]] {
            let encoded = crate::selection::base64_encode(bytes);
            assert_eq!(base64_decode(&encoded).as_deref(), Some(bytes), "{encoded}");
        }
        assert_eq!(base64_decode("Zg"), Some(b"f".to_vec()));
        assert_eq!(base64_decode("Z m 8 ="), Some(b"fo".to_vec()));
        assert_eq!(base64_decode("!bad"), None);
    }
}
