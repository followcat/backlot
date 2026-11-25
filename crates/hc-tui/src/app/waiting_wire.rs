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

// `ExecResult.stdout` arrives base64; the shared client-side decoder.
use crate::selection::base64_decode;

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
