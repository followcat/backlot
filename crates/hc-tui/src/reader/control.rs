//! Coalesced controls wake both OS polling and channel-capacity waits.

use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use tokio::sync::{Notify, mpsc};

pub(super) const COLORS: u8 = 1;
pub(super) const APPEARANCE: u8 = 2;
pub(super) const CELL_SIZE: u8 = 4;
pub(super) const MOUSE_CHANGED: u8 = 8;

pub(super) struct Control {
    stopped: AtomicBool,
    queries: AtomicU8,
    mouse_capture: AtomicBool,
    cell_query: bool,
    resize: Mutex<Option<(u16, u16)>>,
    failure: Mutex<Option<String>>,
    wake: UnixStream,
    signals: signal_hook::iterator::Handle,
    changed: Notify,
}

impl Control {
    pub(super) fn new(
        wake: UnixStream,
        signals: signal_hook::iterator::Handle,
        cell_query: bool,
    ) -> Self {
        Self {
            stopped: AtomicBool::new(false),
            queries: AtomicU8::new(0),
            mouse_capture: AtomicBool::new(false),
            cell_query,
            resize: Mutex::new(None),
            failure: Mutex::new(None),
            wake,
            signals,
            changed: Notify::new(),
        }
    }

    fn wake(&self) {
        // A full socket already carries a wake. The actual controls are
        // retained separately, so coalescing the wake cannot lose them.
        let _ = (&self.wake).write(&[1]);
        self.changed.notify_one();
    }

    pub(super) fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.signals.close();
        self.wake();
    }

    pub(super) fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    pub(super) fn queries_sent(&self) {
        let flags = COLORS | APPEARANCE | if self.cell_query { CELL_SIZE } else { 0 };
        self.queries.fetch_or(flags, Ordering::AcqRel);
        self.wake();
    }

    pub(super) fn written(&self, bytes: &[u8]) {
        let contains = |sequence: &[u8]| bytes.windows(sequence.len()).any(|part| part == sequence);
        let mut flags = 0;
        if contains(b"\x1b]10;?") || contains(b"\x1b]11;?") {
            flags |= COLORS;
        }
        if contains(crate::host::SCHEME_QUERY.as_bytes()) {
            flags |= APPEARANCE;
        }
        if contains(b"\x1b[16t") {
            flags |= CELL_SIZE;
        }
        if contains(b"\x1b[?1003h") || contains(b"\x1b[?1003l") {
            self.mouse_capture
                .store(contains(b"\x1b[?1003h"), Ordering::Release);
            flags |= MOUSE_CHANGED;
        }
        if flags != 0 {
            self.queries.fetch_or(flags, Ordering::AcqRel);
            self.wake();
        }
    }

    pub(super) fn mouse_capture(&self) -> bool {
        self.mouse_capture.load(Ordering::Acquire)
    }

    pub(super) fn take_queries(&self) -> u8 {
        self.queries.swap(0, Ordering::AcqRel)
    }

    pub(super) fn resize(&self, size: (u16, u16)) {
        *self.resize.lock().unwrap_or_else(|e| e.into_inner()) = Some(size);
        self.wake();
    }

    pub(super) fn take_resize(&self) -> Option<(u16, u16)> {
        self.resize.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    pub(super) fn fail(&self, error: io::Error) {
        *self.failure.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.to_string());
    }

    pub(super) fn failure(&self) -> Option<String> {
        self.failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Retain an unsent value when a control interrupts a capacity wait.
    pub(super) fn send<T>(&self, sender: &mpsc::Sender<T>, value: T) -> Delivery<T> {
        match sender.try_send(value) {
            Ok(()) => Delivery::Sent,
            Err(mpsc::error::TrySendError::Closed(_)) => Delivery::Closed,
            Err(mpsc::error::TrySendError::Full(value)) => {
                let permit = futures_lite::future::block_on(async {
                    tokio::select! {
                        biased;
                        _ = self.changed.notified() => None,
                        permit = sender.reserve() => Some(permit),
                    }
                });
                match permit {
                    Some(Ok(permit)) => {
                        permit.send(value);
                        Delivery::Sent
                    }
                    Some(Err(_)) => Delivery::Closed,
                    None => Delivery::Interrupted(value),
                }
            }
        }
    }
}

pub(super) enum Delivery<T> {
    Sent,
    Closed,
    Interrupted(T),
}
