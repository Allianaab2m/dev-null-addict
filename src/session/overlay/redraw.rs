use std::time::{Duration, Instant};

/// UI invalidations are immediate; shell output shares a fixed background refresh deadline.
#[derive(Debug, Default)]
pub(super) struct Redraw {
    ui: bool,
    background: bool,
    next_background: Option<Instant>,
}

impl Redraw {
    pub(super) const INTERVAL: Duration = Duration::from_millis(33);

    pub(super) fn opened(&mut self, now: Instant) {
        self.background_refreshed(now);
    }

    pub(super) fn ui_changed(&mut self) {
        self.ui = true;
    }

    pub(super) fn background_changed(&mut self) {
        self.background = true;
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.next_background.filter(|_| self.background)
    }

    pub(super) fn background_due(&self, now: Instant) -> bool {
        self.deadline().is_some_and(|deadline| now >= deadline)
    }

    pub(super) fn background_refreshed(&mut self, now: Instant) {
        self.background = false;
        self.next_background = Some(now + Self::INTERVAL);
        self.ui = true;
    }

    pub(super) fn needs_draw(&self) -> bool {
        self.ui
    }

    pub(super) fn drawn(&mut self) {
        self.ui = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Output arriving continuously must share a fixed deadline, while UI work cannot be delayed or reset that deadline.
    #[test]
    fn output_is_throttled_and_input_remains_immediate() {
        let start = Instant::now();
        let mut redraw = Redraw::default();
        redraw.opened(start);
        assert!(redraw.needs_draw());
        redraw.drawn();
        for millis in 1..=100 {
            let now = start + Duration::from_millis(millis);
            redraw.background_changed();
            if millis == 10 || millis == 40 {
                let deadline = redraw.deadline();
                redraw.ui_changed();
                assert!(redraw.needs_draw());
                redraw.drawn();
                assert_eq!(redraw.deadline(), deadline);
            }
            assert_eq!(redraw.background_due(now), millis % 33 == 0);
            if redraw.background_due(now) {
                redraw.background_refreshed(now);
                assert!(redraw.needs_draw());
                redraw.drawn();
            }
            assert!(
                !redraw.needs_draw(),
                "output alone cannot bypass the throttle"
            );
        }
        assert_eq!(redraw.deadline(), Some(start + Duration::from_millis(132)));
        redraw.background_refreshed(start + Duration::from_millis(132));
        redraw.drawn();
        assert_eq!(
            redraw.deadline(),
            None,
            "idle shells must not keep waking the loop"
        );
    }
}
