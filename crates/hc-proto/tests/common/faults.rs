//! Stream-half wrappers that inject one-direction failures: a split
//! duplex pair cannot isolate "the peer's writer died but its reader
//! lives", so the client side wraps its own half instead.
#![allow(dead_code)]

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// A write half that lets `pass` writes through, then fails — models the
/// backend's pipe dying on our write side while our read side stays open.
pub struct FailWrites<W> {
    pub inner: W,
    pub pass: usize,
}

impl<W: AsyncWrite + Unpin> AsyncWrite for FailWrites<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.pass == 0 {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "backend stopped accepting writes",
            )));
        }
        self.pass -= 1;
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// A read half that reports EOF the first time the inner stream has no
/// data ready — after any real byte has arrived, so the hello reply always
/// gets through. Models the peer's writer dying while its reader stays
/// open.
pub struct EofReads<R> {
    pub inner: R,
    pub seen: bool,
    pub spent: bool,
}

impl<R: AsyncRead + Unpin> EofReads<R> {
    /// Wraps `inner` fresh: EOF after the first idle poll past real data.
    pub fn new(inner: R) -> Self {
        EofReads {
            inner,
            seen: false,
            spent: false,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for EofReads<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.spent {
            return Poll::Ready(Ok(()));
        }
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Pending if self.seen => {
                self.spent = true;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Ok(())) => {
                if buf.remaining() > 0 {
                    self.seen = true;
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}
