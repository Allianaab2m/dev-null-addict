use std::time::Duration;

/// Follows the shell's output sequence by sequence.
///
/// It knows whether the output so far ends between sequences, and between the
/// bytes of a UTF-8 character. The overlay only takes the screen at such a
/// point, since bytes of its own in the middle of a sequence would change
/// what the sequence means.
///
/// It also picks out complete queries to the terminal, so that they can be
/// answered while the overlay hides the shell, and are not asked again when
/// the output is replayed.
#[derive(Debug, Default)]
pub struct OutputFramer {
    partial: Vec<u8>,
    state: ParseState,
    delivered: usize,
    long: bool,
}

/// A piece of the shell's output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    /// Output, as it is.
    Bytes(Vec<u8>),
    /// A complete query to the terminal.
    Query { kind: QueryKind, raw: Vec<u8> },
}

/// What a query asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueryKind {
    /// Cursor position (`CSI 6n`, `CSI ?6n`).
    CursorPosition,
    /// The setting of a mode (`DECRQM`).
    Mode,
    /// The kitty keyboard protocol flags (`CSI ?u`).
    KeyboardFlags,
    /// Device attributes (`CSI c`, `CSI >c`), the terminal's version
    /// (`XTVERSION`), and the like.
    Terminal,
    /// The default colors or the palette (OSC 4, 10, 11, 12 with `?`).
    Color,
}

impl QueryKind {
    /// Whether the answer depends on the shell's screen and modes, so that
    /// the copy answers it while the overlay is open. The others go to the
    /// outer terminal, whose reply then goes to the shell.
    pub fn is_local(self) -> bool {
        matches!(
            self,
            Self::CursorPosition | Self::Mode | Self::KeyboardFlags
        )
    }
}

impl OutputFramer {
    /// A sequence longer than this, such as a large OSC 52 or DCS, is passed
    /// on in pieces as it comes, and is never a query.
    pub const MAX_SEQUENCE: usize = 4096;

    /// How long the overlay waits for the output to reach a boundary before
    /// it opens anyway. It then cancels the partial sequence on the outer
    /// terminal with CAN.
    pub const BOUNDARY_TIMEOUT: Duration = Duration::from_millis(50);

    pub fn new() -> Self {
        Self::default()
    }

    /// Splits `bytes` into frames, in order.
    ///
    /// With `hold_partial`, a sequence cut off at the end is kept until the
    /// rest arrives, so that a query split across reads is still found.
    /// Without it, everything goes out right away, as it should while the
    /// overlay is closed, and only the parse state is kept.
    pub fn push(&mut self, bytes: &[u8], hold_partial: bool, out: &mut Vec<Frame>) {
        for &byte in bytes {
            self.state.advance(byte);
            self.partial.push(byte);
            if self.at_boundary() || self.partial.len() >= Self::MAX_SEQUENCE {
                let raw = self.partial[self.delivered..].to_vec();
                let kind = (self.delivered == 0 && !self.long && self.at_boundary())
                    .then(|| query_kind(&raw))
                    .flatten();
                if let Some(kind) = kind {
                    out.push(Frame::Query { kind, raw });
                } else {
                    emit(out, &raw);
                }
                self.partial.clear();
                self.delivered = 0;
                self.long = !self.at_boundary();
            }
        }
        if !hold_partial {
            emit(out, &self.partial[self.delivered..]);
            self.delivered = self.partial.len();
        }
    }

    /// Whether the output so far ends between sequences and characters.
    pub fn at_boundary(&self) -> bool {
        self.state == ParseState::Ground
    }

    /// Gives up a held partial sequence as bytes, and forgets the parse
    /// state, as after a CAN.
    pub fn flush(&mut self, out: &mut Vec<Frame>) {
        emit(out, &self.partial[self.delivered..]);
        self.partial.clear();
        self.delivered = 0;
        self.state = ParseState::Ground;
        self.long = false;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ParseState {
    #[default]
    Ground,
    Utf8(u8),
    Escape,
    Intermediate,
    Csi,
    String {
        osc: bool,
        escaped: bool,
    },
}

impl ParseState {
    fn advance(&mut self, byte: u8) {
        if matches!(byte, 0x18 | 0x1a) {
            *self = Self::Ground;
            return;
        }
        *self = match *self {
            Self::Utf8(left) if byte & 0xc0 == 0x80 => {
                if left == 1 {
                    Self::Ground
                } else {
                    Self::Utf8(left - 1)
                }
            }
            Self::Utf8(_) => {
                *self = Self::Ground;
                self.advance(byte);
                return;
            }
            Self::Ground => match byte {
                0x1b => Self::Escape,
                0xc2..=0xdf => Self::Utf8(1),
                0xe0..=0xef => Self::Utf8(2),
                0xf0..=0xf4 => Self::Utf8(3),
                _ => Self::Ground,
            },
            Self::Escape => match byte {
                b'[' => Self::Csi,
                b']' | b'P' | b'_' | b'^' | b'X' => Self::String {
                    osc: byte == b']',
                    escaped: false,
                },
                0x20..=0x2f => Self::Intermediate,
                0x1b => Self::Escape,
                0..=0x1f => Self::Escape,
                _ => Self::Ground,
            },
            Self::Intermediate => match byte {
                0x30..=0x7e => Self::Ground,
                0x1b => Self::Escape,
                _ => Self::Intermediate,
            },
            Self::Csi => match byte {
                0x40..=0x7e => Self::Ground,
                0x1b => Self::Escape,
                _ => Self::Csi,
            },
            Self::String { osc, escaped } => {
                if (osc && byte == 7) || (escaped && byte == b'\\') {
                    Self::Ground
                } else {
                    Self::String {
                        osc,
                        escaped: byte == 0x1b,
                    }
                }
            }
        };
    }
}

fn emit(out: &mut Vec<Frame>, raw: &[u8]) {
    if raw.is_empty() {
        return;
    }
    if let Some(Frame::Bytes(bytes)) = out.last_mut() {
        bytes.extend_from_slice(raw);
    } else {
        out.push(Frame::Bytes(raw.to_vec()));
    }
}

fn query_kind(raw: &[u8]) -> Option<QueryKind> {
    if matches!(raw, b"\x1b[6n" | b"\x1b[?6n") {
        Some(QueryKind::CursorPosition)
    } else if raw.starts_with(b"\x1b[") && raw.ends_with(b"$p") {
        Some(QueryKind::Mode)
    } else if raw == b"\x1b[?u" {
        Some(QueryKind::KeyboardFlags)
    } else if raw.starts_with(b"\x1b[")
        && (raw.ends_with(b"c") || raw == b"\x1b[>q" || raw == b"\x1b[>0q" || raw == b"\x1b[5n")
    {
        Some(QueryKind::Terminal)
    } else if raw.starts_with(b"\x1b]") {
        let text = std::str::from_utf8(&raw[2..]).ok()?;
        let command = text.split(';').next()?;
        (matches!(command, "4" | "10" | "11" | "12") && text.contains('?'))
            .then_some(QueryKind::Color)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(frames: &[Frame]) -> Vec<u8> {
        frames
            .iter()
            .flat_map(|f| match f {
                Frame::Bytes(raw) | Frame::Query { raw, .. } => raw.clone(),
            })
            .collect()
    }

    // No chunk boundary may turn a partial UTF-8 character or control sequence into a safe takeover point.
    #[test]
    fn boundaries_and_utf8() {
        for raw in [
            "界".as_bytes(),
            b"\x1b[31m",
            b"\x1b(0",
            b"\x1b]title\x07",
            b"\x1bPdata\x1b\\",
        ] {
            for hold in [false, true] {
                for split in 1..raw.len() {
                    let mut framer = OutputFramer::new();
                    let mut out = Vec::new();
                    framer.push(&raw[..split], hold, &mut out);
                    assert!(!framer.at_boundary());
                    if hold {
                        assert!(out.is_empty());
                    } else {
                        assert_eq!(bytes(&out), raw[..split]);
                    }
                    framer.push(&raw[split..], hold, &mut out);
                    assert!(framer.at_boundary());
                    assert_eq!(bytes(&out), raw);
                }
            }
        }
    }

    // Split queries must be separated once while hidden and must not replay with ordinary output.
    #[test]
    fn queries_are_framed_in_order() {
        for (raw, kind) in [
            (&b"\x1b[6n"[..], QueryKind::CursorPosition),
            (b"\x1b[?6n", QueryKind::CursorPosition),
            (b"\x1b[?2004$p", QueryKind::Mode),
            (b"\x1b[?u", QueryKind::KeyboardFlags),
            (b"\x1b[c", QueryKind::Terminal),
            (b"\x1b[>0c", QueryKind::Terminal),
            (b"\x1b[>q", QueryKind::Terminal),
            (b"\x1b]10;?\x1b\\", QueryKind::Color),
            (b"\x1b]4;1;?\x07", QueryKind::Color),
        ] {
            for split in 1..raw.len() {
                let mut framer = OutputFramer::new();
                let mut out = Vec::new();
                framer.push(b"before", true, &mut out);
                framer.push(&raw[..split], true, &mut out);
                framer.push(&raw[split..], true, &mut out);
                framer.push(b"after", true, &mut out);
                assert_eq!(
                    out,
                    vec![
                        Frame::Bytes(b"before".to_vec()),
                        Frame::Query {
                            kind,
                            raw: raw.to_vec()
                        },
                        Frame::Bytes(b"after".to_vec())
                    ]
                );
            }
        }
    }

    // Large strings stream with bounded storage but remain non-boundaries until their terminator.
    #[test]
    fn long_sequences_and_flush_never_duplicate() {
        let mut framer = OutputFramer::new();
        let mut out = Vec::new();
        let mut raw = b"\x1b]52;c;".to_vec();
        raw.extend(vec![b'a'; OutputFramer::MAX_SEQUENCE * 3]);
        for part in raw.chunks(997) {
            framer.push(part, true, &mut out);
            assert!(framer.partial.len() < OutputFramer::MAX_SEQUENCE);
        }
        assert!(!framer.at_boundary());
        assert!(!out.is_empty());
        framer.push(b"\x07", true, &mut out);
        raw.push(7);
        assert!(framer.at_boundary());
        assert_eq!(bytes(&out), raw);
        assert!(out.iter().all(|f| matches!(f, Frame::Bytes(_))));
        out.clear();
        framer.push(b"\x1b[", false, &mut out);
        framer.flush(&mut out);
        assert_eq!(bytes(&out), b"\x1b[");
        assert!(framer.at_boundary());
        out.clear();
        framer.push(b"\x1b]unfinished", true, &mut out);
        framer.flush(&mut out);
        assert_eq!(bytes(&out), b"\x1b]unfinished");
    }

    // Cancellation and malformed UTF-8 must recover without hiding the following escape sequence.
    #[test]
    fn cancellation_recovers_boundaries() {
        let mut framer = OutputFramer::new();
        let mut out = Vec::new();
        framer.push(b"\xe7\x1b[", true, &mut out);
        assert!(!framer.at_boundary());
        framer.push(b"\x18", true, &mut out);
        assert!(framer.at_boundary());
        assert_eq!(bytes(&out), b"\xe7\x1b[\x18");
    }
}
