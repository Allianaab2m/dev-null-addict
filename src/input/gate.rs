use std::time::{Duration, Instant};

use super::Scanned;

/// Tells a single press of the toggle key, which opens the overlay, from a
/// double press, which sends the key to the shell.
///
/// After a first press nothing happens until a second press or other input
/// arrives, or [`DOUBLE_PRESS_WINDOW`](Self::DOUBLE_PRESS_WINDOW) passes. So
/// the overlay opens that much after the press.
///
/// Repeats and releases of the toggle key go to the shell only after a press
/// that went there, so the program in the shell never sees half a key.
#[derive(Debug, Default)]
pub struct ToggleGate {
    pending_since: Option<Instant>,
    forwarding: bool,
}

/// What to do with the input while the overlay is closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GateAction {
    /// Send these bytes to the shell.
    Forward(Vec<u8>),
    /// Open the overlay, then hand these bytes, possibly none, to it.
    Open(Vec<u8>),
    /// Nothing yet.
    Wait,
}

impl ToggleGate {
    /// How soon after a first press a second one makes a double press.
    pub const DOUBLE_PRESS_WINDOW: Duration = Duration::from_millis(300);

    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&mut self, scanned: Scanned, now: Instant) -> GateAction {
        use super::ToggleEvent;
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            self.pending_since = None;
            self.forwarding = false;
            let bytes = match scanned {
                Scanned::Forward(raw) | Scanned::Toggle { raw, .. } => raw,
            };
            return GateAction::Open(bytes);
        }
        match scanned {
            Scanned::Forward(bytes) => {
                if self.pending_since.take().is_some() {
                    self.forwarding = false;
                    GateAction::Open(bytes)
                } else {
                    GateAction::Forward(bytes)
                }
            }
            Scanned::Toggle {
                event: ToggleEvent::Press,
                raw,
            } => {
                if self.pending_since.take().is_some() {
                    self.forwarding = true;
                    GateAction::Forward(raw)
                } else {
                    self.pending_since = Some(now);
                    self.forwarding = false;
                    GateAction::Wait
                }
            }
            Scanned::Toggle { event, raw } => {
                let forward = self.forwarding;
                if event == ToggleEvent::Release {
                    self.forwarding = false;
                }
                if forward {
                    GateAction::Forward(raw)
                } else {
                    GateAction::Wait
                }
            }
        }
    }

    /// When a pending first press opens the overlay, if one is pending.
    pub fn deadline(&self) -> Option<Instant> {
        self.pending_since.map(|t| t + Self::DOUBLE_PRESS_WINDOW)
    }

    /// Opens the overlay once the window for a second press has passed.
    pub fn expire(&mut self, now: Instant) -> GateAction {
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            self.pending_since = None;
            self.forwarding = false;
            GateAction::Open(Vec::new())
        } else {
            GateAction::Wait
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ToggleEvent;
    fn toggle(event: ToggleEvent) -> Scanned {
        Scanned::Toggle {
            event,
            raw: vec![7],
        }
    }

    // A double press forwards one original key; only that key's repeats and release belong to the shell.
    #[test]
    fn double_press_and_event_ownership() {
        let mut gate = ToggleGate::new();
        let now = Instant::now();
        assert_eq!(
            gate.handle(toggle(ToggleEvent::Press), now),
            GateAction::Wait
        );
        assert_eq!(
            gate.handle(toggle(ToggleEvent::Repeat), now),
            GateAction::Wait
        );
        assert_eq!(
            gate.handle(toggle(ToggleEvent::Release), now),
            GateAction::Wait
        );
        assert_eq!(
            gate.handle(toggle(ToggleEvent::Press), now + Duration::from_millis(299)),
            GateAction::Forward(vec![7])
        );
        assert_eq!(
            gate.handle(toggle(ToggleEvent::Repeat), now),
            GateAction::Forward(vec![7])
        );
        assert_eq!(
            gate.handle(toggle(ToggleEvent::Release), now),
            GateAction::Forward(vec![7])
        );
        assert_eq!(
            gate.handle(toggle(ToggleEvent::Repeat), now),
            GateAction::Wait
        );
    }

    // Other input opens immediately and is delivered to the overlay; expiration must fire only once.
    #[test]
    fn pending_input_and_timeout() {
        let mut gate = ToggleGate::new();
        let now = Instant::now();
        gate.handle(toggle(ToggleEvent::Press), now);
        assert_eq!(
            gate.handle(Scanned::Forward(b"text".to_vec()), now),
            GateAction::Open(b"text".to_vec())
        );
        assert_eq!(gate.expire(now + Duration::from_secs(1)), GateAction::Wait);
        gate.handle(toggle(ToggleEvent::Press), now);
        assert_eq!(
            gate.expire(now + Duration::from_millis(299)),
            GateAction::Wait
        );
        assert_eq!(
            gate.expire(now + ToggleGate::DOUBLE_PRESS_WINDOW),
            GateAction::Open(Vec::new())
        );
        assert_eq!(
            gate.handle(toggle(ToggleEvent::Release), now),
            GateAction::Wait
        );
        gate.handle(toggle(ToggleEvent::Press), now);
        assert_eq!(
            gate.handle(Scanned::Forward(vec![b'x']), now + Duration::from_secs(1)),
            GateAction::Open(vec![b'x'])
        );
    }
}
