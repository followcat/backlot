// Derived from herdr-core schema/README.md and crates/herdr-core/src/server.rs (Apache-2.0).

//! The two wire codecs over one buffered stream.
//!
//! A connection starts in plain mode — one JSON-RPC message per line — and a
//! `hello` reply with `framed: true` switches it to binary frames. [`Reader`]
//! holds the read buffer for both modes, so bytes that arrived with the hello
//! reply (a peer may send the first frame in the same packet) are not lost
//! when the codec switches. [`Writer`] is separate because some transports —
//! an `ssh ... hc connect` child — give read and write halves that do not
//! share one object.

use crate::frame::{FormatError, Frame, HEADER_LEN, Header};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Largest line accepted in plain mode: 64 MiB. A reply that big belongs on
/// channel 0 in framed mode; a peer streaming more is cut off.
pub const MAX_LINE: usize = 64 << 20;

const READ_CHUNK: usize = 8 * 1024;
/// How much consumed prefix may accumulate before it is compacted away.
const COMPACT_AT: usize = 64 * 1024;

/// A codec failure on the read path.
#[derive(Debug)]
pub enum Error {
    /// The underlying stream failed.
    Io(std::io::Error),
    /// The stream ended inside a message (a partial line, header or payload).
    /// A clean end at a message boundary is `Ok(None)`, not an error.
    Truncated(&'static str),
    /// The bytes violate the wire format.
    Format(FormatError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io: {e}"),
            Error::Truncated(what) => write!(f, "stream ended inside a {what}"),
            Error::Format(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}

impl From<FormatError> for Error {
    fn from(e: FormatError) -> Error {
        Error::Format(e)
    }
}

/// Buffered reader for both wire modes. Read ahead bytes stay buffered until
/// a matching `read_*` call consumes them, so a framed hello reply and the
/// first frame can share one packet.
pub struct Reader<R> {
    read: R,
    buf: Vec<u8>,
    start: usize,
    /// Offset into `buffered()` a newline search already covered, so a long
    /// fragmented line is not rescanned after every fill.
    scanned: usize,
    /// Longest accepted line in [`read_line`](Self::read_line).
    max_line: usize,
}

impl<R: AsyncRead + Unpin> Reader<R> {
    /// Wraps a byte stream, bounding plain-mode lines at [`MAX_LINE`].
    pub fn new(read: R) -> Reader<R> {
        Reader::with_line_limit(read, MAX_LINE)
    }

    /// Wraps a byte stream with a custom line limit. Tests use it to keep
    /// oversized-line fixtures small.
    pub fn with_line_limit(read: R, max_line: usize) -> Reader<R> {
        Reader {
            read,
            buf: Vec::new(),
            start: 0,
            scanned: 0,
            max_line,
        }
    }

    /// Bytes received but not yet consumed by `read_line`/`read_frame`.
    pub fn buffered(&self) -> &[u8] {
        &self.buf[self.start..]
    }

    /// Gives back the stream and any unread buffered bytes.
    pub fn into_inner(self) -> (R, Vec<u8>) {
        (self.read, self.buf[self.start..].to_vec())
    }

    /// Reads more from the stream into the buffer. Returns bytes read; `0`
    /// means the stream ended.
    async fn fill(&mut self) -> std::io::Result<usize> {
        if self.start > 0 && (self.start == self.buf.len() || self.start >= COMPACT_AT) {
            self.buf.drain(..self.start);
            self.start = 0;
        }
        let mut chunk = [0u8; READ_CHUNK];
        let n = self.read.read(&mut chunk).await?;
        self.buf.extend_from_slice(&chunk[..n]);
        Ok(n)
    }

    fn consume(&mut self, n: usize) {
        self.start += n;
        self.scanned = self.scanned.saturating_sub(n);
        if self.start == self.buf.len() {
            self.buf.clear();
            self.start = 0;
        }
    }

    /// Reads one plain-mode line, without its newline. `Ok(None)` is a clean
    /// end of stream between messages; an end inside a line is
    /// [`Error::Truncated`]. Lines longer than the configured limit
    /// ([`MAX_LINE`] by default) fail, whether or not the newline arrived.
    pub async fn read_line(&mut self) -> Result<Option<Vec<u8>>, Error> {
        loop {
            let buffered = self.buffered();
            if let Some(at) = buffered[self.scanned..].iter().position(|b| *b == b'\n') {
                let at = self.scanned + at;
                if at > self.max_line {
                    return Err(FormatError::OversizedLine {
                        limit: self.max_line,
                    }
                    .into());
                }
                let mut line = buffered[..at].to_vec();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                self.consume(at + 1);
                return Ok(Some(line));
            }
            let len = buffered.len();
            self.scanned = len;
            if len > self.max_line {
                return Err(FormatError::OversizedLine {
                    limit: self.max_line,
                }
                .into());
            }
            if self.fill().await? == 0 {
                return if self.buffered().is_empty() {
                    Ok(None)
                } else {
                    Err(Error::Truncated("line"))
                };
            }
        }
    }

    /// Reads one framed-mode frame. `Ok(None)` is a clean end of stream at a
    /// frame boundary; a partial header or payload is [`Error::Truncated`].
    /// Header limits are checked before the payload buffer is allocated.
    pub async fn read_frame(&mut self) -> Result<Option<Frame>, Error> {
        loop {
            let buffered = self.buffered();
            if buffered.len() >= HEADER_LEN {
                let header = Header::decode(buffered[..HEADER_LEN].try_into().expect("header"))?;
                let total = HEADER_LEN + header.len;
                if buffered.len() >= total {
                    let payload = buffered[HEADER_LEN..total].to_vec();
                    self.consume(total);
                    return Ok(Some(Frame::new(header.channel, header.kind, payload)));
                }
            }
            if self.fill().await? == 0 {
                return if self.buffered().is_empty() {
                    Ok(None)
                } else {
                    Err(Error::Truncated("frame"))
                };
            }
        }
    }
}

/// The write half of a wire: lines in plain mode, frames after the switch.
pub struct Writer<W> {
    write: W,
}

impl<W: AsyncWrite + Unpin> Writer<W> {
    /// Wraps a byte sink.
    pub fn new(write: W) -> Writer<W> {
        Writer { write }
    }

    /// Writes `line` plus a newline. `line` is one JSON-RPC message.
    pub async fn write_line(&mut self, line: &[u8]) -> std::io::Result<()> {
        self.write.write_all(line).await?;
        self.write.write_all(b"\n").await
    }

    /// Writes one frame: header plus payload.
    pub async fn write_frame(&mut self, frame: &Frame) -> std::io::Result<()> {
        self.write.write_all(&frame.encode()).await
    }

    /// Flushes the underlying writer.
    pub async fn flush(&mut self) -> std::io::Result<()> {
        self.write.flush().await
    }
}
