use std::time::{Duration, Instant};

use super::{StartupInput, find_cpr};

/// Owns the first CPR after an unanswered startup query, before either input
/// decoder sees it. Terminals answer in order, even if the shell also queries.
#[derive(Default)]
pub(in crate::session) struct LateReply {
    deadline: Option<Instant>,
    held: Vec<u8>,
}

impl LateReply {
    pub(in crate::session) fn expect(&mut self, now: Instant) {
        self.deadline = Some(now + Duration::from_secs(2));
    }

    pub(in crate::session) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub(in crate::session) fn filter(&mut self, bytes: &[u8], now: Instant) -> StartupInput {
        let mut held = std::mem::take(&mut self.held);
        held.extend_from_slice(bytes);
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.deadline = None;
        }
        let mut position = None;
        if self.deadline.is_some() {
            // Shift+F3 and similar xterm keys have the same shape (ESC[1;2R).
            // Eating one such key during this bounded window is an accepted trade-off.
            if let Some((start, end, cursor)) = find_cpr(&held) {
                held.drain(start..end);
                position = Some(cursor);
                self.deadline = None;
            } else if let Some(start) = held.iter().rposition(|byte| *byte == 0x1b)
                && cpr_prefix(&held[start..])
            {
                self.held = held.split_off(start);
            }
        }
        StartupInput { position, held }
    }
}

fn cpr_prefix(bytes: &[u8]) -> bool {
    if bytes == b"\x1b" {
        return true;
    }
    let Some(tail) = bytes.strip_prefix(b"\x1b[") else {
        return false;
    };
    let mut parts = tail.split(|byte| *byte == b';');
    let row = parts.next().expect("split always has a first part");
    let column = parts.next();
    let number = |part: &[u8]| {
        part.iter().all(u8::is_ascii_digit)
            && (part.is_empty()
                || std::str::from_utf8(part)
                    .ok()
                    .and_then(|text| text.parse::<u16>().ok())
                    .is_some())
    };
    parts.next().is_none()
        && number(row)
        && column.is_none_or(|col| {
            !row.is_empty() && row.iter().any(|byte| *byte != b'0') && number(col)
        })
}
