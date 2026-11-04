//! Wire codec tests over scripted in-memory peers (`tokio::io::duplex`).
//! No sockets, no hc sessions.

use hc_proto::codec::{Error, Reader, Writer};
use hc_proto::frame::{Control, FormatError, Frame, HEADER_LEN, Kind, MAX_PAYLOAD};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex, split};

/// Our half of a wire plus the scripted peer's end.
fn wire() -> (
    Reader<impl tokio::io::AsyncRead>,
    Writer<impl tokio::io::AsyncWrite>,
    DuplexStream,
) {
    let (ours, peer) = duplex(4096);
    let (read_half, write_half) = split(ours);
    (Reader::new(read_half), Writer::new(write_half), peer)
}

#[tokio::test]
async fn lines_round_trip_adjacent_messages() {
    let (mut reader, _writer, mut peer) = wire();
    peer.write_all(b"{\"id\":1}\n{\"id\":2}\n").await.unwrap();
    assert_eq!(reader.read_line().await.unwrap().unwrap(), b"{\"id\":1}");
    assert_eq!(reader.read_line().await.unwrap().unwrap(), b"{\"id\":2}");
}

#[tokio::test]
async fn line_arrives_truly_fragmented() {
    // Capacity 1 forces one fill per byte; the writer yields on each.
    let (ours, mut peer) = duplex(1);
    let (read_half, _w) = split(ours);
    let mut reader = Reader::new(read_half);
    let bytes = b"{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n".to_vec();
    let writer = tokio::spawn(async move {
        for b in &bytes {
            peer.write_all(&[*b]).await.unwrap();
        }
    });
    let line = reader.read_line().await.unwrap().unwrap();
    assert_eq!(
        line,
        b"{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}"
    );
    writer.await.unwrap();
}

#[tokio::test]
async fn frame_arrives_truly_fragmented() {
    // The header itself is split across fills, byte by byte.
    let (ours, mut peer) = duplex(1);
    let (read_half, _w) = split(ours);
    let mut reader = Reader::new(read_half);
    let frame = Frame::resize(3, 80, 24);
    let bytes = frame.encode();
    let writer = tokio::spawn(async move {
        for b in &bytes {
            peer.write_all(&[*b]).await.unwrap();
        }
    });
    let got = reader.read_frame().await.unwrap().unwrap();
    assert_eq!(got, frame);
    writer.await.unwrap();
}

#[tokio::test]
async fn hello_reply_and_first_frame_share_a_read() {
    let (mut reader, _writer, mut peer) = wire();
    let reply = b"{\"jsonrpc\":\"2.0\",\"id\":0,\"result\":{\"generation\":1}}\n";
    let frame = Frame::data(7, b"first paint bytes");
    let mut blob = reply.to_vec();
    blob.extend_from_slice(&frame.encode());
    // One write: the read-ahead must survive the plain -> framed switch.
    peer.write_all(&blob).await.unwrap();

    let line = reader.read_line().await.unwrap().unwrap();
    assert_eq!(line, &reply[..reply.len() - 1]);
    let got = reader.read_frame().await.unwrap().unwrap();
    assert_eq!(got, frame);
}

#[tokio::test]
async fn inbound_frames_all_kinds_decode() {
    let (mut reader, _writer, mut peer) = wire();
    let frames = [
        Frame::data(0, b"{\"id\":1}"),
        Frame::credit(3, 258),
        Frame::close(3, br#"{"reason":"exited","exit_code":0}"#),
        Frame::resize(3, 80, 24),
        Frame::focus(3, true),
        Frame::facts(3, b"{\"fg\":\"#aabbcc\"}"),
        Frame::automatic_input(3, b"\x1b[?1;2c"),
    ];
    let mut blob = Vec::new();
    for frame in &frames {
        blob.extend_from_slice(&frame.encode());
    }
    peer.write_all(&blob).await.unwrap();

    for expected in &frames {
        let got = reader.read_frame().await.unwrap().unwrap();
        assert_eq!(&got, expected);
    }
    assert_eq!(frames[1].control().unwrap(), Control::Credit(258));
    assert_eq!(
        frames[3].control().unwrap(),
        Control::Resize { cols: 80, rows: 24 }
    );
    assert_eq!(frames[4].control().unwrap(), Control::Focus(true));
}

#[tokio::test]
async fn outbound_frames_are_little_endian_on_the_wire() {
    let (_reader, mut writer, mut peer) = wire();
    let frames = [
        Frame::data(0, b"{\"id\":1}"),
        Frame::credit(3, 258),
        Frame::close(3, br#"{"reason":"exited"}"#),
        Frame::resize(3, 80, 24),
        Frame::focus(3, true),
        Frame::facts(3, b"{\"fg\":\"#aabbcc\"}"),
        Frame::automatic_input(3, b"\x1b[?1;2c"),
    ];
    for frame in &frames {
        writer.write_frame(frame).await.unwrap();
        let mut raw = vec![0u8; frame.encode().len()];
        peer.read_exact(&mut raw).await.unwrap();
        assert_eq!(raw, frame.encode());
    }

    // Byte-level check of one little-endian control on the wire.
    writer
        .write_frame(&Frame::resize(9, 132, 43))
        .await
        .unwrap();
    let mut raw = [0u8; HEADER_LEN + 4];
    peer.read_exact(&mut raw).await.unwrap();
    assert_eq!(&raw[0..4], &9u32.to_le_bytes());
    assert_eq!(raw[4], 3);
    assert_eq!(&raw[5..9], &4u32.to_le_bytes());
    assert_eq!(&raw[9..], &[132, 0, 43, 0]);
}

#[tokio::test]
async fn adjacent_frames_from_one_write() {
    let (mut reader, _writer, mut peer) = wire();
    let mut blob = Frame::data(1, b"one").encode();
    blob.extend_from_slice(&Frame::data(1, b"two").encode());
    blob.extend_from_slice(&Frame::data(1, b"three").encode());
    peer.write_all(&blob).await.unwrap();
    for text in ["one", "two", "three"] {
        let got = reader.read_frame().await.unwrap().unwrap();
        assert_eq!(got.payload, text.as_bytes());
    }
}

#[tokio::test]
async fn zero_length_data_and_close_are_legal() {
    let (mut reader, _writer, mut peer) = wire();
    let mut blob = Frame::data(4, Vec::new()).encode();
    blob.extend_from_slice(&Frame::close(4, Vec::new()).encode());
    peer.write_all(&blob).await.unwrap();

    let data = reader.read_frame().await.unwrap().unwrap();
    assert!(data.payload.is_empty());
    let close = reader.read_frame().await.unwrap().unwrap();
    assert_eq!(close.control().unwrap(), Control::Close(&[][..]));
}

#[tokio::test]
async fn unknown_kind_passes_through() {
    let (mut reader, _writer, mut peer) = wire();
    let frame = Frame::new(2, Kind::Unknown(42), b"future kind payload");
    peer.write_all(&frame.encode()).await.unwrap();
    let got = reader.read_frame().await.unwrap().unwrap();
    assert_eq!(got.kind, Kind::Unknown(42));
    assert_eq!(
        got.control().unwrap(),
        Control::Unknown {
            kind: 42,
            payload: b"future kind payload"
        }
    );
}

#[tokio::test]
async fn oversized_header_rejected_without_payload() {
    for len in [u32::MAX, MAX_PAYLOAD as u32 + 1] {
        let (mut reader, _writer, mut peer) = wire();
        let mut header = [0u8; HEADER_LEN];
        header[5..9].copy_from_slice(&len.to_le_bytes());
        peer.write_all(&header).await.unwrap();
        let err = reader.read_frame().await.unwrap_err();
        assert!(
            matches!(err, Error::Format(FormatError::OversizedPayload { len: got }) if got == len),
            "len {len}: {err:?}"
        );
    }
}

#[tokio::test]
async fn max_payload_header_then_eof_is_truncated() {
    // The boundary size is accepted by the header; the stream then ends
    // mid-payload, which is Truncated — no 16 MiB fixture needed.
    let (mut reader, _writer, peer) = wire();
    let mut header = [0u8; HEADER_LEN];
    header[4] = 0; // DATA
    header[5..9].copy_from_slice(&(MAX_PAYLOAD as u32).to_le_bytes());
    let mut peer = peer;
    peer.write_all(&header).await.unwrap();
    drop(peer);
    assert!(matches!(
        reader.read_frame().await.unwrap_err(),
        Error::Truncated("frame")
    ));
}

#[tokio::test]
async fn fixed_size_controls_reject_wrong_lengths() {
    // (kind, claimed len): CREDIT=4, RESIZE=4, FOCUS=1.
    for (kind_byte, len) in [(1u8, 3u32), (3, 2), (4, 4)] {
        let (mut reader, _writer, mut peer) = wire();
        let mut header = [0u8; HEADER_LEN];
        header[4] = kind_byte;
        header[5..9].copy_from_slice(&len.to_le_bytes());
        peer.write_all(&header).await.unwrap();
        let err = reader.read_frame().await.unwrap_err();
        assert!(
            matches!(err, Error::Format(FormatError::BadControlLen { .. })),
            "kind {kind_byte} len {len}: {err:?}"
        );
    }
}

#[tokio::test]
async fn eof_at_frame_boundary_is_clean() {
    let (mut reader, _writer, peer) = wire();
    drop(peer);
    assert_eq!(reader.read_frame().await.unwrap(), None);
    assert_eq!(reader.read_line().await.unwrap(), None);
}

#[tokio::test]
async fn eof_mid_header_and_mid_payload_are_truncated() {
    let (mut reader, _writer, mut peer) = wire();
    peer.write_all(&[1, 2, 3, 4, 0]).await.unwrap(); // 5 of 9 header bytes
    drop(peer);
    assert!(matches!(
        reader.read_frame().await.unwrap_err(),
        Error::Truncated("frame")
    ));

    let (mut reader, _writer, mut peer) = wire();
    let frame = Frame::data(1, b"ten bytes!").encode();
    peer.write_all(&frame[..frame.len() - 3]).await.unwrap();
    drop(peer);
    assert!(matches!(
        reader.read_frame().await.unwrap_err(),
        Error::Truncated("frame")
    ));
}

#[tokio::test]
async fn eof_mid_line_is_truncated() {
    let (mut reader, _writer, mut peer) = wire();
    peer.write_all(b"{\"id\":1").await.unwrap();
    drop(peer);
    assert!(matches!(
        reader.read_line().await.unwrap_err(),
        Error::Truncated("line")
    ));
}

#[tokio::test]
async fn line_at_exact_limit_is_accepted() {
    let (ours, mut peer) = duplex(4096);
    let (read_half, _w) = split(ours);
    let mut reader = Reader::with_line_limit(read_half, 64);
    let mut line = vec![b'x'; 64];
    line.push(b'\n');
    peer.write_all(&line).await.unwrap();
    assert_eq!(reader.read_line().await.unwrap().unwrap().len(), 64);
}

#[tokio::test]
async fn line_over_limit_with_newline_in_same_chunk_fails() {
    // Regression: the limit must also apply when the delimiter already
    // sits in the buffer.
    let (ours, mut peer) = duplex(4096);
    let (read_half, _w) = split(ours);
    let mut reader = Reader::with_line_limit(read_half, 64);
    let mut line = vec![b'x'; 65];
    line.push(b'\n');
    peer.write_all(&line).await.unwrap();
    assert!(matches!(
        reader.read_line().await.unwrap_err(),
        Error::Format(FormatError::OversizedLine { limit: 64 })
    ));
}

#[tokio::test]
async fn line_over_limit_fragmented_fails() {
    // Capacity 1: the overflow arrives byte by byte, not in one chunk.
    let (ours, mut peer) = duplex(1);
    let (read_half, _w) = split(ours);
    let mut reader = Reader::with_line_limit(read_half, 64);
    let writer = tokio::spawn(async move {
        for _ in 0..200 {
            if peer.write_all(b"x").await.is_err() {
                break;
            }
        }
    });
    let err = reader.read_line().await.unwrap_err();
    assert!(matches!(
        err,
        Error::Format(FormatError::OversizedLine { limit: 64 })
    ));
    writer.abort();
}

#[tokio::test]
async fn line_over_limit_unterminated_fails_before_eof() {
    let (ours, mut peer) = duplex(4096);
    let (read_half, _w) = split(ours);
    let mut reader = Reader::with_line_limit(read_half, 64);
    peer.write_all(&[b'y'; 100]).await.unwrap();
    // Peer still open: the limit alone must end the read.
    assert!(matches!(
        reader.read_line().await.unwrap_err(),
        Error::Format(FormatError::OversizedLine { limit: 64 })
    ));
}
