/// What the shell writes while the overlay is open, kept to be replayed when
/// it closes.
///
/// Past [`LIMIT`](Self::LIMIT) bytes the output is dropped. Then there is
/// nothing to replay, and the screen is redrawn from the copy instead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PendingOutput {
    bytes: Vec<u8>,
    overflowed: bool,
}

impl PendingOutput {
    pub const LIMIT: usize = 4 << 20;

    pub fn replayable(&self) -> bool {
        !self.overflowed
    }

    pub fn push(&mut self, bytes: &[u8]) {
        if !self.overflowed {
            if self.bytes.len().saturating_add(bytes.len()) > Self::LIMIT {
                self.bytes.clear();
                self.overflowed = true;
            } else {
                self.bytes.extend_from_slice(bytes);
            }
        }
    }

    /// The output to replay, or `None` if some was dropped. Empties the
    /// buffer either way.
    pub fn take(&mut self) -> Option<Vec<u8>> {
        let bytes = std::mem::take(&mut self.bytes);
        (!std::mem::take(&mut self.overflowed)).then_some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Once any byte was dropped, replay is impossible until take resets the buffer for the next opening.
    #[test]
    fn limit_is_inclusive_and_overflow_resets() {
        let mut pending = PendingOutput::default();
        pending.push(&vec![b'a'; PendingOutput::LIMIT]);
        assert_eq!(pending.take().unwrap().len(), PendingOutput::LIMIT);
        pending.push(&vec![b'b'; PendingOutput::LIMIT]);
        pending.push(b"x");
        pending.push(b"later");
        assert_eq!(pending.take(), None);
        pending.push(b"next");
        assert_eq!(pending.take(), Some(b"next".to_vec()));
    }
}
