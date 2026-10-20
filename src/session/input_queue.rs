use std::collections::VecDeque;

/// Bytes on their way to the shell, written as fast as the PTY takes them.
///
/// `write_all` is not cancellation-safe inside `select!`, so the loop writes
/// what it can with `write` and keeps the rest here, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputQueue {
    bytes: VecDeque<u8>,
}

impl InputQueue {
    pub fn push(&mut self, bytes: &[u8]) {
        self.bytes.extend(bytes);
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The bytes to write next, as one slice.
    pub fn front(&mut self) -> &[u8] {
        self.bytes.make_contiguous()
    }

    /// Drops the first `written` bytes, after a write took them.
    pub fn consume(&mut self, written: usize) {
        self.bytes.drain(..written);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Partial writes and appended input must retain order without resending a consumed prefix.
    #[test]
    fn partial_writes_keep_order() {
        let mut queue = InputQueue::default();
        queue.push(b"abcdef");
        queue.consume(2);
        queue.push(b"ghi");
        assert_eq!(queue.front(), b"cdefghi");
        queue.consume(5);
        assert_eq!(queue.front(), b"hi");
        queue.consume(2);
        assert!(queue.is_empty());
    }
}
