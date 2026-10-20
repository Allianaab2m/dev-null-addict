use std::time::{Duration, Instant};

use super::ToggleKey;

/// Watches the bytes on their way to the shell for the toggle key.
///
/// The key is recognised in every encoding a terminal may use for it,
/// whichever the program in the shell turned on:
///
/// - the control byte (`0x07` for Ctrl+G),
/// - the kitty keyboard protocol and `CSI u` (`CSI 103;5u`), with event
///   types, alternate keys, associated text and lock modifiers (Caps Lock and
///   Num Lock are ignored, any other modifier makes it another key),
/// - xterm's modifyOtherKeys (`CSI 27;5;103~`).
///
/// All other bytes come out untouched and in order. A partial sequence at the
/// end of the input that could still turn out to be the toggle key is held
/// back until more bytes arrive or [`HOLD_TIMEOUT`](Self::HOLD_TIMEOUT)
/// passes, so a key split across reads is still found. Between the start and
/// the end of a bracketed paste nothing is matched.
#[derive(Debug)]
pub struct ToggleScanner {
    key: ToggleKey,
    held: Vec<u8>,
    held_since: Option<Instant>,
    in_paste: bool,
}

/// A piece of the input, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scanned {
    /// Bytes for the shell.
    Forward(Vec<u8>),
    /// The toggle key, with the exact bytes that encoded it.
    Toggle { event: ToggleEvent, raw: Vec<u8> },
}

/// What happened to the toggle key. Only the kitty keyboard protocol reports
/// repeats and releases, and only when the program in the shell asked for
/// them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToggleEvent {
    Press,
    Repeat,
    Release,
}

impl ToggleScanner {
    /// How long a partial sequence is held back before it is passed on as it
    /// is. A lone Esc key press reaches the shell this much later.
    pub const HOLD_TIMEOUT: Duration = Duration::from_millis(10);

    pub fn new(key: ToggleKey) -> Self {
        Self {
            key,
            held: Vec::new(),
            held_since: None,
            in_paste: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8], now: Instant, out: &mut Vec<Scanned>) {
        self.expire(now, out);
        self.held.extend_from_slice(bytes);
        let mut pos = 0;
        while pos < self.held.len() {
            let tail = &self.held[pos..];
            if self.in_paste && !tail.starts_with(b"\x1b[201~") {
                if b"\x1b[201~".starts_with(tail) {
                    break;
                }
                forward(out, &tail[..1]);
                pos += 1;
                continue;
            }
            let len = if tail[0] == 0x1b {
                let Some(len) = super::protocol::sequence_len(tail) else {
                    break;
                };
                len
            } else {
                1
            };
            let raw = &tail[..len];
            if raw == b"\x1b[200~" {
                self.in_paste = true;
                forward(out, raw);
            } else if raw == b"\x1b[201~" {
                self.in_paste = false;
                forward(out, raw);
            } else if !self.in_paste
                && let Some(event) = super::protocol::toggle_event(raw, self.key)
            {
                out.push(Scanned::Toggle {
                    event,
                    raw: raw.to_vec(),
                });
            } else {
                forward(out, raw);
            }
            pos += len;
        }
        self.held.drain(..pos);
        if self.held.is_empty() {
            self.held_since = None;
        } else if pos > 0 || self.held_since.is_none() {
            self.held_since = Some(now);
        }
    }

    /// When the held-back bytes are due, if there are any.
    pub fn deadline(&self) -> Option<Instant> {
        self.held_since.map(|t| t + Self::HOLD_TIMEOUT)
    }

    /// Passes on the held-back bytes once their deadline has passed.
    pub fn expire(&mut self, now: Instant, out: &mut Vec<Scanned>) {
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            forward(out, &std::mem::take(&mut self.held));
            self.held_since = None;
        }
    }

    /// Hands over the held-back bytes, for when the overlay takes the input,
    /// and forgets whether a paste was going on, as the overlay reads the
    /// paste from then on.
    pub fn take_held(&mut self) -> Vec<u8> {
        self.held_since = None;
        self.in_paste = false;
        std::mem::take(&mut self.held)
    }
}

fn forward(out: &mut Vec<Scanned>, bytes: &[u8]) {
    if let Some(Scanned::Forward(previous)) = out.last_mut() {
        previous.extend_from_slice(bytes);
    } else if !bytes.is_empty() {
        out.push(Scanned::Forward(bytes.to_vec()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every split of every toggle encoding must preserve the exact bytes and event ownership.
    #[test]
    fn encodings_survive_every_read_boundary() {
        let now = Instant::now();
        for (raw, event) in [
            (&b"\x07"[..], ToggleEvent::Press),
            (b"\x1b[103;5u", ToggleEvent::Press),
            (b"\x1b[103:71:103;197:1;103u", ToggleEvent::Press),
            (b"\x1b[103;69:2u", ToggleEvent::Repeat),
            (b"\x1b[103;133:3u", ToggleEvent::Release),
            (b"\x1b[27;5;103~", ToggleEvent::Press),
        ] {
            for split in 0..=raw.len() {
                let mut scanner = ToggleScanner::new("ctrl+g".parse().unwrap());
                let mut out = Vec::new();
                scanner.push(&raw[..split], now, &mut out);
                scanner.push(&raw[split..], now + Duration::from_millis(1), &mut out);
                assert_eq!(
                    out,
                    vec![Scanned::Toggle {
                        event,
                        raw: raw.to_vec()
                    }],
                    "split {split}: {raw:?}"
                );
            }
        }
    }

    // Other modifiers and ordinary data must pass unchanged, including repeated scans.
    #[test]
    fn only_exact_control_modifier_toggles() {
        let mut scanner = ToggleScanner::new("ctrl+g".parse().unwrap());
        let mut out = Vec::new();
        let bytes = b"abc\x1b[103;7u\x1b[103;6u\x1b[27;1;103~\x03";
        scanner.push(bytes, Instant::now(), &mut out);
        assert_eq!(out, vec![Scanned::Forward(bytes.to_vec())]);
        scanner.push(b"\x07x\x07", Instant::now(), &mut out);
        assert_eq!(
            out.iter()
                .filter(|e| matches!(e, Scanned::Toggle { .. }))
                .count(),
            2
        );
    }

    // A lone ESC must reach the shell after 10 ms, and taking held bytes must not duplicate them.
    #[test]
    fn holdback_expires_and_transfers_once() {
        let now = Instant::now();
        let mut scanner = ToggleScanner::new("ctrl+g".parse().unwrap());
        let mut out = Vec::new();
        scanner.push(b"\x1b[10", now, &mut out);
        assert!(out.is_empty());
        scanner.expire(now + Duration::from_millis(9), &mut out);
        assert!(out.is_empty());
        scanner.expire(now + ToggleScanner::HOLD_TIMEOUT, &mut out);
        assert_eq!(out, vec![Scanned::Forward(b"\x1b[10".to_vec())]);
        scanner.push(b"\x1b", now, &mut out);
        assert_eq!(scanner.take_held(), b"\x1b");
        assert!(scanner.take_held().is_empty());
    }

    // Paste delimiters may be split anywhere; even escape-looking pasted text is literal.
    #[test]
    fn bracketed_paste_suppresses_matching() {
        let bytes = b"\x1b[200~\x07\x1b[103;5u\x1b]unfinished\x1b[201~\x07";
        for chunk in 1..=bytes.len() {
            let mut scanner = ToggleScanner::new("ctrl+g".parse().unwrap());
            let mut out = Vec::new();
            for part in bytes.chunks(chunk) {
                scanner.push(part, Instant::now(), &mut out);
            }
            assert_eq!(
                out,
                vec![
                    Scanned::Forward(bytes[..bytes.len() - 1].to_vec()),
                    Scanned::Toggle {
                        event: ToggleEvent::Press,
                        raw: vec![7]
                    }
                ]
            );
        }
    }
}
