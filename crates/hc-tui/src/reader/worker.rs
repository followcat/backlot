// Derived in part from herdr src/raw_input.rs (Apache-2.0).
//! Level-triggered polling preserves stdin flags. Pending decoded input
//! stops further reads until bounded consumers accept the current batch.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use super::control::{APPEARANCE, CELL_SIZE, COLORS, Control, Delivery, MOUSE_CHANGED};
use crate::app::UiEvent;
use crate::host::HostReply;
use crate::raw_events::{Decoded, events_from_framed_chunks};
use crate::raw_input::RawInputByteFramer;

pub(super) struct Settings {
    pub mouse_capture: bool,
    pub queries_pending: bool,
    pub cell_size_pending: bool,
}

pub(super) fn run(
    input: Arc<File>,
    mut wake: UnixStream,
    control: &Control,
    ui: mpsc::Sender<UiEvent>,
    replies: mpsc::Sender<HostReply>,
    settings: Settings,
) -> io::Result<()> {
    let mut framer = RawInputByteFramer::for_host_input();
    // Source confirms disambiguation only from the initial flags reply + DA.
    let mut probed_flags = None;
    let mut primary_attributes = false;
    let mut mouse_capture = settings.mouse_capture;
    if settings.queries_pending {
        framer.host_color_query_sent();
        framer.host_appearance_query_sent();
        framer.enable_host_color_scheme_change_tracking();
        framer.enable_host_appearance_query_on_focus();
    }
    if settings.cell_size_pending {
        framer.host_cell_size_query_sent();
    }
    let mut pending = VecDeque::new();
    let mut resize = None;
    let mut deadline = None;
    let mut chunk = [0u8; 8192];
    let mut eof = false;
    loop {
        if control.stopped() {
            return Ok(());
        }
        let flags = control.take_queries();
        if flags & MOUSE_CHANGED != 0 {
            mouse_capture = control.mouse_capture();
        }
        if flags & COLORS != 0 {
            framer.host_color_query_sent();
        }
        if flags & APPEARANCE != 0 {
            framer.host_appearance_query_sent();
        }
        if flags & CELL_SIZE != 0 {
            framer.host_cell_size_query_sent();
        }
        resize = control.take_resize().or(resize);

        if let Some((cols, rows)) = resize {
            match control.send(&ui, UiEvent::Resize { cols, rows }) {
                Delivery::Sent => resize = None,
                Delivery::Closed => return Ok(()),
                Delivery::Interrupted(_) => continue,
            }
        }
        if let Some(event) = pending.pop_front() {
            let delivery = match event {
                Decoded::Ui(event) => match control.send(&ui, event) {
                    Delivery::Sent => Delivery::Sent,
                    Delivery::Closed => Delivery::Closed,
                    Delivery::Interrupted(event) => Delivery::Interrupted(Decoded::Ui(event)),
                },
                Decoded::Reply(reply) => match control.send(&replies, reply) {
                    Delivery::Sent => Delivery::Sent,
                    Delivery::Closed => Delivery::Closed,
                    Delivery::Interrupted(reply) => Delivery::Interrupted(Decoded::Reply(reply)),
                },
            };
            match delivery {
                Delivery::Sent => {}
                Delivery::Closed => return Ok(()),
                Delivery::Interrupted(event) => pending.push_front(event),
            }
            continue;
        }

        if eof {
            return Ok(());
        }
        let timeout = deadline.map_or(-1, |at: Instant| {
            let remaining = at.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                0
            } else {
                remaining.as_millis().max(1) as i32
            }
        });
        let mut fds = [
            libc::pollfd {
                fd: input.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: these two initialized pollfd values refer to descriptors
        // owned throughout the call; poll writes only their revents fields.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if fds[1].revents != 0 {
            let _ = wake.read(&mut chunk);
            continue; // arm controls before simultaneous stdin bytes
        }
        if ready == 0 {
            pending.extend(events_from_framed_chunks(framer.flush_timeout()));
            deadline = framer.needs_timeout().then(|| {
                Instant::now() + Duration::from_millis(framer.held_input_flush_timeout_ms() as u64)
            });
        } else if fds[0].revents != 0 {
            match (&*input).read(&mut chunk) {
                Ok(0) => {
                    pending.extend(events_from_framed_chunks(framer.finish()));
                    eof = true;
                }
                Ok(count) => {
                    let chunks = framer.push(&chunk[..count]);
                    if let Some(reason) = framer.take_rejection() {
                        pending.push_back(Decoded::Ui(UiEvent::InputRejected(reason)));
                    }
                    let events = events_from_framed_chunks(chunks);
                    for event in &events {
                        match event {
                            Decoded::Reply(HostReply::KeyboardFlags(flags))
                                if !primary_attributes =>
                            {
                                probed_flags = Some(*flags)
                            }
                            Decoded::Reply(HostReply::PrimaryDeviceAttributes) => {
                                primary_attributes = true
                            }
                            _ => {}
                        }
                    }
                    framer.set_host_escape_disambiguation_active(
                        primary_attributes && probed_flags.is_some_and(|flags| flags & 1 != 0),
                    );
                    pending.extend(events);
                    deadline = framer.needs_timeout().then(|| {
                        Instant::now()
                            + Duration::from_millis(
                                framer.idle_flush_timeout_ms(mouse_capture) as u64
                            )
                    });
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                    ) => {}
                Err(error) => return Err(error),
            }
        }
    }
}
