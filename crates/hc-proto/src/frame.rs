// Derived from herdr-core schema/README.md and crates/herdr-core/src/wire.rs (Apache-2.0).

//! Framed-mode frames: `[channel u32 LE][kind u8][len u32 LE][payload]`.
//! The format follows herdr-core's `schema/README.md` (protocol generation 1,
//! pinned commit `ec8adcdf0da38e539e8702f7c37801647da035f3`).
//!
//! Channel 0 carries JSON-RPC messages as DATA; each attach stream and each
//! framed event subscription owns a higher channel. Frame kinds not yet known
//! decode as [`Kind::Unknown`] so newer cores stay usable.

/// Bytes in a frame header: channel, kind, length.
pub const HEADER_LEN: usize = 9;
/// Largest payload a frame may carry: 16 MiB.
pub const MAX_PAYLOAD: usize = 16 << 20;
/// Channel carrying JSON-RPC in both directions.
pub const CONTROL_CHANNEL: u32 = 0;
/// Credit a stream starts with before the viewer acknowledges bytes.
pub const INITIAL_CREDIT: u32 = 256 * 1024;

/// The `kind` byte. Unknown values decode instead of failing, so a newer core
/// can add kinds without breaking this client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Channel 0: one JSON-RPC message. Streams: terminal bytes.
    Data,
    /// u32 LE: bytes the receiver consumed; the sender may send more.
    Credit,
    /// The channel is over; JSON reason from the core, empty from a client.
    Close,
    /// u16 LE cols, u16 LE rows: viewer to core, or core to observer.
    Resize,
    /// One byte, viewer to core: 1 gained focus, 0 lost it.
    Focus,
    /// JSON display facts (`Display` in the schema), viewer to core.
    Facts,
    /// Terminal-generated reply bytes, not human activity, viewer to core.
    AutomaticInput,
    /// A semantically tagged human release, accepted only after negotiation.
    QuietHumanRelease,
    /// A kind this protocol version does not define; payload passes through.
    Unknown(u8),
}

impl Kind {
    /// Maps a wire byte to a kind; undefined bytes become [`Kind::Unknown`].
    pub fn from_u8(b: u8) -> Kind {
        match b {
            0 => Kind::Data,
            1 => Kind::Credit,
            2 => Kind::Close,
            3 => Kind::Resize,
            4 => Kind::Focus,
            5 => Kind::Facts,
            6 => Kind::AutomaticInput,
            7 => Kind::QuietHumanRelease,
            other => Kind::Unknown(other),
        }
    }

    /// The wire byte for this kind.
    pub fn to_u8(self) -> u8 {
        match self {
            Kind::Data => 0,
            Kind::Credit => 1,
            Kind::Close => 2,
            Kind::Resize => 3,
            Kind::Focus => 4,
            Kind::Facts => 5,
            Kind::AutomaticInput => 6,
            Kind::QuietHumanRelease => 7,
            Kind::Unknown(b) => b,
        }
    }

    /// The fixed payload size of a control kind, when it has one. `None`
    /// means the payload is variable (bounded by [`MAX_PAYLOAD`] only).
    fn fixed_len(self) -> Option<usize> {
        match self {
            Kind::Credit | Kind::Resize => Some(4),
            Kind::Focus => Some(1),
            _ => None,
        }
    }
}

/// Why a header or payload is not a legal frame.
#[derive(Debug, Clone, PartialEq)]
pub enum FormatError {
    /// `len` exceeds [`MAX_PAYLOAD`]; rejected before allocating.
    OversizedPayload {
        /// The claimed payload length.
        len: u32,
    },
    /// A fixed-size control frame arrived with a different length.
    BadControlLen {
        /// The frame's kind byte.
        kind: u8,
        /// The size this kind requires.
        expected: usize,
        /// The size the header claimed.
        actual: usize,
    },
    /// A line grew past the limit without a newline.
    OversizedLine {
        /// The configured limit in bytes.
        limit: usize,
    },
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::OversizedPayload { len } => {
                write!(f, "frame payload of {len} bytes exceeds {MAX_PAYLOAD}")
            }
            FormatError::BadControlLen {
                kind,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "control kind {kind} needs {expected} bytes, got {actual}"
                )
            }
            FormatError::OversizedLine { limit } => write!(f, "line exceeds {limit} bytes"),
        }
    }
}

impl std::error::Error for FormatError {}

/// A decoded frame header: which channel, which kind, how much payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// The channel this frame belongs to.
    pub channel: u32,
    /// The frame kind.
    pub kind: Kind,
    /// Payload length in bytes, already checked against [`MAX_PAYLOAD`] and
    /// any fixed control size.
    pub len: usize,
}

impl Header {
    /// Decodes and validates a raw header. Fails before any payload buffer is
    /// allocated, so an oversized `len` costs nine bytes, not 16 MiB.
    pub fn decode(raw: &[u8; HEADER_LEN]) -> Result<Header, FormatError> {
        let channel = u32::from_le_bytes(raw[0..4].try_into().expect("4 bytes"));
        let kind = Kind::from_u8(raw[4]);
        let len_u32 = u32::from_le_bytes(raw[5..9].try_into().expect("4 bytes"));
        if len_u32 as usize > MAX_PAYLOAD {
            return Err(FormatError::OversizedPayload { len: len_u32 });
        }
        let len = len_u32 as usize;
        if let Some(expected) = kind.fixed_len()
            && len != expected
        {
            return Err(FormatError::BadControlLen {
                kind: raw[4],
                expected,
                actual: len,
            });
        }
        Ok(Header { channel, kind, len })
    }
}

/// One frame: a channel, a kind, and its payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The channel this frame belongs to.
    pub channel: u32,
    /// The frame kind.
    pub kind: Kind,
    /// The payload bytes.
    pub payload: Vec<u8>,
}

/// A typed view of a frame's payload, borrowing from the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control<'a> {
    /// Raw bytes: JSON-RPC on channel 0, terminal bytes elsewhere.
    Data(&'a [u8]),
    /// Bytes the receiver consumed.
    Credit(u32),
    /// Channel end; JSON reason from the core, empty from a client.
    Close(&'a [u8]),
    /// Terminal size change.
    Resize {
        /// Columns.
        cols: u16,
        /// Rows.
        rows: u16,
    },
    /// Whether the viewer gained (`true`) or lost (`false`) focus.
    Focus(bool),
    /// JSON display facts.
    Facts(&'a [u8]),
    /// Terminal-generated reply bytes, not human activity.
    AutomaticInput(&'a [u8]),
    /// Source-tagged human release bytes, gated by attach negotiation.
    QuietHumanRelease(&'a [u8]),
    /// A kind this protocol version does not define.
    Unknown {
        /// The kind byte.
        kind: u8,
        /// The untouched payload.
        payload: &'a [u8],
    },
}

impl Frame {
    /// Wraps a payload with a kind and channel.
    pub fn new(channel: u32, kind: Kind, payload: impl Into<Vec<u8>>) -> Frame {
        Frame {
            channel,
            kind,
            payload: payload.into(),
        }
    }

    /// A DATA frame (JSON-RPC on channel 0, stream bytes elsewhere).
    pub fn data(channel: u32, payload: impl Into<Vec<u8>>) -> Frame {
        Frame::new(channel, Kind::Data, payload)
    }

    /// A CREDIT frame acknowledging `n` consumed bytes.
    pub fn credit(channel: u32, n: u32) -> Frame {
        Frame::new(channel, Kind::Credit, n.to_le_bytes().to_vec())
    }

    /// A CLOSE frame; the core sends a JSON reason, a client sends empty.
    pub fn close(channel: u32, payload: impl Into<Vec<u8>>) -> Frame {
        Frame::new(channel, Kind::Close, payload)
    }

    /// A RESIZE frame with the new grid size.
    pub fn resize(channel: u32, cols: u16, rows: u16) -> Frame {
        let mut payload = Vec::with_capacity(4);
        payload.extend_from_slice(&cols.to_le_bytes());
        payload.extend_from_slice(&rows.to_le_bytes());
        Frame::new(channel, Kind::Resize, payload)
    }

    /// A FOCUS frame: `true` gained focus, `false` lost it.
    pub fn focus(channel: u32, focused: bool) -> Frame {
        Frame::new(channel, Kind::Focus, vec![u8::from(focused)])
    }

    /// A FACTS frame carrying JSON display facts.
    pub fn facts(channel: u32, payload: impl Into<Vec<u8>>) -> Frame {
        Frame::new(channel, Kind::Facts, payload)
    }

    /// An AUTOMATIC_INPUT frame with terminal-generated reply bytes. Send it
    /// only when hello and attach both confirmed the capability.
    pub fn automatic_input(channel: u32, payload: impl Into<Vec<u8>>) -> Frame {
        Frame::new(channel, Kind::AutomaticInput, payload)
    }

    /// The whole frame on the wire: header plus payload. Panics when the
    /// payload exceeds [`MAX_PAYLOAD`] — that is a bug in the caller.
    pub fn encode(&self) -> Vec<u8> {
        assert!(
            self.payload.len() <= MAX_PAYLOAD,
            "payload exceeds {MAX_PAYLOAD} bytes"
        );
        let mut out = Vec::with_capacity(HEADER_LEN + self.payload.len());
        out.extend_from_slice(&self.channel.to_le_bytes());
        out.push(self.kind.to_u8());
        out.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }

    /// Decodes `payload` per the frame kind, checking fixed control sizes.
    /// Unknown kinds pass through untouched.
    pub fn control(&self) -> Result<Control<'_>, FormatError> {
        let len = self.payload.len();
        let bad = |expected| FormatError::BadControlLen {
            kind: self.kind.to_u8(),
            expected,
            actual: len,
        };
        match self.kind {
            Kind::Data => Ok(Control::Data(&self.payload)),
            Kind::Credit => {
                if len != 4 {
                    return Err(bad(4));
                }
                Ok(Control::Credit(u32::from_le_bytes(
                    self.payload[..4].try_into().expect("4 bytes"),
                )))
            }
            Kind::Close => Ok(Control::Close(&self.payload)),
            Kind::Resize => {
                if len != 4 {
                    return Err(bad(4));
                }
                Ok(Control::Resize {
                    cols: u16::from_le_bytes(self.payload[0..2].try_into().expect("2 bytes")),
                    rows: u16::from_le_bytes(self.payload[2..4].try_into().expect("2 bytes")),
                })
            }
            Kind::Focus => {
                if len != 1 {
                    return Err(bad(1));
                }
                Ok(Control::Focus(self.payload[0] != 0))
            }
            Kind::Facts => Ok(Control::Facts(&self.payload)),
            Kind::AutomaticInput => Ok(Control::AutomaticInput(&self.payload)),
            Kind::QuietHumanRelease => Ok(Control::QuietHumanRelease(&self.payload)),
            Kind::Unknown(kind) => Ok(Control::Unknown {
                kind,
                payload: &self.payload,
            }),
        }
    }

    /// For a CLOSE frame, the core's JSON reason, when it sent one.
    pub fn close_reason(&self) -> Option<serde_json::Value> {
        if self.kind != Kind::Close || self.payload.is_empty() {
            return None;
        }
        serde_json::from_slice(&self.payload).ok()
    }
}
