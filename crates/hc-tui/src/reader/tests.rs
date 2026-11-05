use super::*;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::fd::{FromRawFd, OwnedFd};
use std::time::{Duration, Instant};

fn pair(capacity: usize) -> (Reader, UnixStream) {
    let (input, host) = UnixStream::pair().unwrap();
    let input = File::from(OwnedFd::from(input));
    (
        spawn_from(input, capacity, false, false, false).unwrap(),
        host,
    )
}

async fn next(reader: &mut Reader) -> Option<UiEvent> {
    tokio::time::timeout(Duration::from_secs(2), reader.ui.recv())
        .await
        .expect("input delivery deadline")
}

#[tokio::test]
async fn legacy_escape_uses_live_mouse_timing_without_a_fake_kitty_confirmation() {
    let (mut reader, mut host) = pair(8);
    let sequence = crate::terminal_modes::mouse_capture_sequence(true, false).unwrap();
    reader.host_control().written(&sequence);
    host.write_all(b"\x1b").unwrap();
    let event = tokio::time::timeout(Duration::from_millis(400), reader.ui.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event,UiEvent::Key(key) if key.input.key == hc_vt::Key::Escape));
    reader.stop();
}

#[tokio::test]
async fn confirmed_host_preserves_a_slow_mouse_tail_after_capture_changes() {
    let (mut reader, mut host) = pair(8);
    host.write_all(b"\x1b[?7u\x1b[?62;22c").unwrap();
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(1), reader.replies.recv())
            .await
            .unwrap()
            .unwrap();
    }
    reader
        .host_control()
        .written(&crate::terminal_modes::mouse_capture_sequence(true, false).unwrap());
    host.write_all(b"\x1b").unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(350), reader.ui.recv())
            .await
            .is_err()
    );
    host.write_all(b"[<0;32;5M").unwrap();
    assert!(matches!(next(&mut reader).await, Some(UiEvent::Mouse(_))));
    reader.stop();
}

fn character(event: UiEvent) -> char {
    let UiEvent::Key(key) = event else {
        panic!("expected a key")
    };
    let hc_vt::Key::Char(c) = key.input.key else {
        panic!("expected a character")
    };
    c
}

#[tokio::test]
async fn slow_consumer_keeps_every_key_in_order_and_eof_closes_channels() {
    let (mut reader, mut host) = pair(1);
    let expected: Vec<u8> = (0..500).map(|i| b'a' + (i % 26) as u8).collect();
    host.write_all(&expected).unwrap();
    host.shutdown(Shutdown::Write).unwrap();
    // Longer than the former send retry limit: this must apply
    // backpressure, rather than silently discard keys every100ms.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let mut actual = Vec::new();
    while let Some(event) = next(&mut reader).await {
        actual.push(character(event) as u8);
    }
    assert_eq!(actual, expected);
    assert!(reader.replies.recv().await.is_none());
    assert!(reader.failure().is_none());
    reader.stop();
}

#[tokio::test]
async fn stopping_a_full_input_queue_is_prompt_and_releases_its_descriptor() {
    let (mut reader, mut host) = pair(1);
    host.write_all(&[b'a'; 256]).unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    let began = Instant::now();
    reader.stop();
    assert!(began.elapsed() < Duration::from_millis(500));
    host.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    assert_eq!(host.read(&mut [0]).unwrap(), 0);
    reader.stop(); // idempotent, including after workers have exited
}

#[test]
fn dropping_an_idle_reader_closes_its_input_and_signal_workers() {
    let (reader, mut host) = pair(1);
    drop(reader);
    host.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    assert_eq!(host.read(&mut [0]).unwrap(), 0);
}

#[tokio::test]
async fn query_wakes_arm_replies_without_stopping_or_consuming_user_input() {
    let (mut reader, mut host) = pair(2);
    reader.queries_sent();
    host.write_all(b"\x1b]10;#abcdef\x07z").unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(2), reader.replies.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reply,
        HostReply::Foreground(hc_vt::Rgb {
            r: 0xab,
            g: 0xcd,
            b: 0xef
        })
    );
    assert_eq!(character(next(&mut reader).await.unwrap()), 'z');
    reader.queries_sent();
    host.write_all(b"y").unwrap();
    assert_eq!(character(next(&mut reader).await.unwrap()), 'y');
    reader.stop();
}

#[tokio::test]
async fn final_coalesced_resize_survives_a_full_channel() {
    let (mut reader, mut host) = pair(1);
    host.write_all(b"a").unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    for size in [(80, 24), (200, 50), (93, 39)] {
        reader.control.resize(size);
    }
    assert_eq!(character(next(&mut reader).await.unwrap()), 'a');
    let event = next(&mut reader).await.unwrap();
    assert!(
        matches!(event, UiEvent::Resize { cols: 93, rows: 39 }),
        "{event:?}"
    );
    reader.stop();
}

#[tokio::test]
async fn sigwinch_reads_the_owned_input_descriptor() {
    let mut master = -1;
    let mut slave = -1;
    let size = libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: openpty initializes these two descriptors on success; no
    // name or termios pointers are supplied, and winsize is initialized.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                &size,
            )
        },
        0
    );
    // SAFETY: each freshly returned descriptor has exactly one owner.
    let master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    let mut reader = spawn_from(slave, 1, false, false, false).unwrap();
    let size = libc::winsize {
        ws_row: 39,
        ws_col: 93,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: master remains open and ioctl reads the initialized winsize.
    assert_eq!(
        unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &size) },
        0
    );
    signal_hook::low_level::raise(signal_hook::consts::SIGWINCH).unwrap();
    assert!(matches!(
        next(&mut reader).await,
        Some(UiEvent::Resize { cols: 93, rows: 39 })
    ));
    reader.stop();
}

#[test]
fn zero_capacity_is_a_setup_error() {
    let (input, _) = UnixStream::pair().unwrap();
    let result = spawn_from(File::from(OwnedFd::from(input)), 0, false, false, false);
    assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::InvalidInput));
}

#[tokio::test]
async fn eof_resolves_a_trailing_escape_before_closing_input() {
    let (mut reader, mut host) = pair(1);
    host.write_all(b"\x1b").unwrap();
    host.shutdown(Shutdown::Write).unwrap();
    assert!(matches!(next(&mut reader).await, Some(UiEvent::Key(key))
                    if key.input.key == hc_vt::Key::Escape));
    assert!(next(&mut reader).await.is_none());
    reader.stop();
}

#[tokio::test]
async fn rejected_paste_tail_never_becomes_keys_on_the_real_reader() {
    let (mut reader, mut host) = pair(1);
    let mut input = b"\x1b[200~".to_vec();
    input.extend(vec![b'x'; crate::app::input::MAX_INPUT_BYTES + 1]);
    input.extend_from_slice(b"\x02q\x1b[201~q");
    host.write_all(&input).unwrap();
    host.shutdown(Shutdown::Write).unwrap();
    assert!(matches!(
        next(&mut reader).await,
        Some(UiEvent::InputRejected(_))
    ));
    assert_eq!(character(next(&mut reader).await.unwrap()), 'q');
    assert!(next(&mut reader).await.is_none());
    reader.stop();
}
