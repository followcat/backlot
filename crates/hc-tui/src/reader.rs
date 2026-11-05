// Derived in part from herdr src/raw_input.rs (Apache-2.0).
//! One input owner with lossless backpressure and cancellable waits.

mod control;
mod worker;

use std::fs::File;
use std::io;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::thread::JoinHandle;

use signal_hook::iterator::Signals;
use tokio::sync::mpsc;

use crate::app::UiEvent;
use crate::host::HostReply;
use control::Control;

/// Drop also stops and joins both workers, even with a full consumer.
pub struct Reader {
    pub ui: mpsc::Receiver<UiEvent>,
    pub replies: mpsc::Receiver<HostReply>,
    control: Arc<Control>,
    input: Option<JoinHandle<()>>,
    signals: Option<JoinHandle<()>>,
}

/// Output controls arm the single reader's reply windows and mouse timing.
#[derive(Clone)]
pub(crate) struct HostControl(Arc<Control>);
impl HostControl {
    pub(crate) fn written(&self, bytes: &[u8]) {
        self.0.written(bytes);
    }
}

impl Reader {
    pub(crate) fn host_control(&self) -> HostControl {
        HostControl(Arc::clone(&self.control))
    }
    pub fn stop(&mut self) {
        self.control.stop();
        for task in [&mut self.input, &mut self.signals] {
            if let Some(task) = task.take() {
                let _ = task.join();
            }
        }
    }

    /// Arm reply windows without stopping the input worker.
    pub fn queries_sent(&self) {
        self.control.queries_sent();
    }

    /// The caller reports an input failure after restoring terminal modes.
    /// Ordinary EOF has no error.
    pub fn failure(&self) -> Option<String> {
        self.control.failure()
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn spawn_reader(
    capacity: usize,
    mouse_capture: bool,
    queries_pending: bool,
    cell_size_pending: bool,
) -> io::Result<Reader> {
    let input = File::from(io::stdin().as_fd().try_clone_to_owned()?);
    spawn_from(
        input,
        capacity,
        mouse_capture,
        queries_pending,
        cell_size_pending,
    )
}

fn spawn_from(
    input: File,
    capacity: usize,
    mouse_capture: bool,
    queries_pending: bool,
    cell_size_pending: bool,
) -> io::Result<Reader> {
    if capacity == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "zero input capacity",
        ));
    }
    let (ui_tx, ui) = mpsc::channel(capacity);
    let (reply_tx, replies) = mpsc::channel(capacity);
    let (wake_read, wake_write) = UnixStream::pair()?;
    wake_read.set_nonblocking(true)?;
    wake_write.set_nonblocking(true)?;
    let mut signals = Signals::new([signal_hook::consts::SIGWINCH])?;
    let control = Arc::new(Control::new(
        wake_write,
        signals.handle(),
        cell_size_pending,
    ));
    let input = Arc::new(input);
    let mut reader = Reader {
        ui,
        replies,
        control,
        input: None,
        signals: None,
    };

    // Install the owner before either spawn: partial setup failure drops it
    // and cancels any worker that already started.
    let worker_control = reader.control.clone();
    let worker_input = input.clone();
    let settings = worker::Settings {
        mouse_capture,
        queries_pending,
        cell_size_pending,
    };
    reader.input = Some(
        std::thread::Builder::new()
            .name("hc-raw-input".into())
            .spawn(move || {
                if let Err(error) = worker::run(
                    worker_input,
                    wake_read,
                    &worker_control,
                    ui_tx,
                    reply_tx,
                    settings,
                ) {
                    worker_control.fail(error);
                }
                worker_control.stop();
            })?,
    );

    let control = reader.control.clone();
    reader.signals = Some(
        std::thread::Builder::new()
            .name("hc-sigwinch".into())
            .spawn(move || {
                for _ in signals.forever() {
                    if let Some(size) = terminal_size(input.as_raw_fd()) {
                        control.resize(size);
                    }
                }
            })?,
    );
    Ok(reader)
}

fn terminal_size(fd: std::os::fd::RawFd) -> Option<(u16, u16)> {
    let mut size = std::mem::MaybeUninit::<libc::winsize>::zeroed();
    // SAFETY: this descriptor stays owned by Arc<File>, and TIOCGWINSZ
    // writes precisely the allocated winsize.
    if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, size.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: the successful ioctl initialized this winsize.
    let size = unsafe { size.assume_init() };
    (size.ws_col > 0 && size.ws_row > 0).then_some((size.ws_col, size.ws_row))
}

#[cfg(test)]
mod tests;
