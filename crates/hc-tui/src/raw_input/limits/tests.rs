use super::*;
use crate::app::UiEvent;
use crate::raw_events::events_from_framed_chunks;

fn expect_q(chunks: Vec<Vec<u8>>) {
    let events = events_from_framed_chunks(chunks);
    assert!(
        matches!(events.as_slice(), [Decoded::Ui(UiEvent::Key(key))]
                    if key.input.key == hc_vt::Key::Char('q')),
        "{events:?}"
    );
}

#[test]
fn oversized_paste_is_rejected_once_without_executing_its_tail() {
    let mut f = RawInputByteFramer::for_host_input();
    let mut paste = BRACKETED_PASTE_START.to_vec();
    paste.extend(vec![b'a'; MAX_INPUT_BYTES + 1]);
    assert!(f.push(&paste).is_empty());
    assert!(f.take_rejection().is_some());
    assert!(f.take_rejection().is_none());
    for _ in 0..32 {
        assert!(f.push(&[b'x'; 8192]).is_empty());
        assert!(f.buffer.is_empty());
    }
    assert!(f.push(b"\x02q\x1b[20").is_empty());
    expect_q(f.push(b"1~q"));
    assert!(f.buffer.is_empty());
}

#[test]
fn maximum_sized_paste_stays_a_single_literal_event() {
    let mut f = RawInputByteFramer::for_host_input();
    let mut paste = BRACKETED_PASTE_START.to_vec();
    paste.extend(vec![b'a'; MAX_INPUT_BYTES]);
    paste.extend_from_slice(BRACKETED_PASTE_END);
    let events = events_from_framed_chunks(f.push(&paste));
    assert!(
        matches!(events.as_slice(), [Decoded::Ui(UiEvent::Paste(bytes))]
                    if bytes.len() == MAX_INPUT_BYTES)
    );
    assert!(f.take_rejection().is_none());
}

#[test]
fn oversized_control_payload_keeps_only_its_terminator_state() {
    for start in [b"\x1b]52;c;".as_slice(), b"\x1bP"] {
        let mut f = RawInputByteFramer::for_host_input();
        let mut control = start.to_vec();
        control.extend(vec![b'x'; MAX_INPUT_BYTES + 8192]);
        assert!(f.push(&control).is_empty());
        assert!(f.buffer.is_empty());
        assert!(f.take_rejection().is_none());
        assert!(f.push(b"\x02q\x1b").is_empty());
        expect_q(f.push(b"\\q"));
    }
}

#[test]
fn timed_out_control_discard_stays_bounded_during_continuous_input() {
    let mut f = RawInputByteFramer::for_host_input();
    assert!(f.push(b"\x1bPunfinished").is_empty());
    assert!(f.flush_timeout().is_empty());
    assert!(f.discard_until.is_some());
    assert!(f.push(&vec![b'x'; MAX_INPUT_BYTES + 8192]).is_empty());
    assert!(f.buffer.is_empty());
    expect_q(f.push(b"\x1b\\q"));
}

#[test]
fn oversized_csi_does_not_turn_parameter_bytes_into_keys() {
    let mut f = RawInputByteFramer::for_host_input();
    let mut report = b"\x1b[".to_vec();
    report.extend(vec![b'1'; MAX_INPUT_BYTES + 1]);
    assert!(f.push(&report).is_empty());
    assert!(f.take_rejection().is_some());
    assert!(f.buffer.is_empty());
    expect_q(f.push(b"uq"));
}

#[test]
fn paste_and_partial_utf8_wait_without_periodic_flushes() {
    for input in [b"\x1b[200~unfinished".as_slice(), b"\xe2", b"\x1b\xe2"] {
        let mut f = RawInputByteFramer::for_host_input();
        assert!(f.push(input).is_empty());
        assert!(f.has_pending_input());
        assert!(!f.needs_timeout());
    }
}
