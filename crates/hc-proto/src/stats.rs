//! Transport counters for one connection kind, read by the client's
//! diagnostics snapshot. Relaxed atomics on the transport's read and write
//! calls; nothing here reaches the wire or the screen.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Counters for one connection kind: every counted transport read and
/// write, and every established connection.
#[derive(Debug, Default)]
pub struct ConnStats {
    reads: AtomicU64,
    empty_reads: AtomicU64,
    bytes_in: AtomicU64,
    writes: AtomicU64,
    bytes_out: AtomicU64,
    connects: AtomicU64,
}

/// A point-in-time copy of a [`ConnStats`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConnSnapshot {
    /// Transport read calls that returned, including end of stream.
    pub reads: u64,
    /// Read calls that delivered no bytes (end of stream).
    pub empty_reads: u64,
    /// Bytes received.
    pub bytes_in: u64,
    /// Transport write calls that returned.
    pub writes: u64,
    /// Bytes sent.
    pub bytes_out: u64,
    /// Connections established, a reconnect being any beyond the first.
    pub connects: u64,
    /// Connections established after the first.
    pub reconnects: u64,
}

impl ConnStats {
    /// The current counts.
    pub fn snapshot(&self) -> ConnSnapshot {
        let connects = self.connects.load(Relaxed);
        ConnSnapshot {
            reads: self.reads.load(Relaxed),
            empty_reads: self.empty_reads.load(Relaxed),
            bytes_in: self.bytes_in.load(Relaxed),
            writes: self.writes.load(Relaxed),
            bytes_out: self.bytes_out.load(Relaxed),
            connects,
            reconnects: connects.saturating_sub(1),
        }
    }

    /// One connection of this kind was established.
    pub fn connected(&self) {
        self.connects.fetch_add(1, Relaxed);
    }

    fn read(&self, bytes: usize) {
        self.reads.fetch_add(1, Relaxed);
        if bytes == 0 {
            self.empty_reads.fetch_add(1, Relaxed);
        }
        self.bytes_in.fetch_add(bytes as u64, Relaxed);
    }

    fn wrote(&self, bytes: usize) {
        self.writes.fetch_add(1, Relaxed);
        self.bytes_out.fetch_add(bytes as u64, Relaxed);
    }
}

/// A transport half whose calls are counted into a [`ConnStats`]. The
/// wrapped half is polled exactly as before; only completed calls count.
pub(crate) struct Counted<T> {
    inner: T,
    stats: Arc<ConnStats>,
}

impl<T> Counted<T> {
    pub(crate) fn new(inner: T, stats: Arc<ConnStats>) -> Counted<T> {
        Counted { inner, stats }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for Counted<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let polled = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = polled {
            this.stats.read(buf.filled().len() - before);
        }
        polled
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for Counted<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let polled = Pin::new(&mut this.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(written)) = polled {
            this.stats.wrote(written);
        }
        polled
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}
