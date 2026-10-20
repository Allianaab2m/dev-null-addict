use super::*;
use crate::api::LogKind;
use crate::screen::Vt100Screen;
use crate::terminal::TermSize;
use std::time::Duration;

fn overlay(layout: crate::OverlayLayout, now: Instant, screen: &Vt100Screen) -> Overlay {
    let toggle = "ctrl+g".parse().unwrap();
    let mut app = App::new(toggle, LogKind::Following, Duration::from_secs(5));
    app.set_layout(layout);
    app.resize(TermSize {
        cols: 100,
        rows: 30,
    });
    app.set_background(screen.text_rows());
    let mut overlay = Overlay::new(app, KeyDecoder::new(toggle));
    // Model the completed takeover without writing to the test runner's terminal.
    overlay.takeover = Some(Takeover {
        shell_buffer: Buffer::Main,
        entered_alternate: true,
        shell_modes: Vec::new(),
        resized: false,
    });
    overlay.redraw.opened(now);
    overlay.redraw.drawn();
    overlay
}

fn first_row(overlay: &Overlay) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| overlay.app.render(frame, jiff::Timestamp::UNIX_EPOCH))
        .unwrap();
    (0..100)
        .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
        .collect()
}

// The overlay deadline must refresh a dirty background even after output stops, retaining replay bytes unchanged.
#[test]
fn background_deadline_updates_the_cached_screen() {
    let now = Instant::now();
    let mut screen = Vt100Screen::new(TermSize {
        cols: 100,
        rows: 30,
    });
    screen.process(b"OLD");
    let mut overlay = overlay(crate::OverlayLayout::Float, now, &screen);
    assert_eq!(overlay.deadline(), None);
    let raw = b"\rNEW";
    screen.process(raw);
    overlay.hold(raw);
    assert_eq!(overlay.deadline(), Some(now + Duration::from_millis(33)));
    for millis in 1..33 {
        overlay.update_background(&screen, now + Duration::from_millis(millis));
        assert!(!overlay.redraw.needs_draw());
        assert!(first_row(&overlay).starts_with("OLD"));
    }
    overlay.app_mut().input(
        crate::input::UiInput::Paste("typed immediately".into()),
        now,
    );
    assert!(overlay.redraw.needs_draw());
    overlay.redraw.drawn();
    assert!(first_row(&overlay).starts_with("OLD"));
    overlay.update_background(&screen, now + Duration::from_millis(33));
    assert!(overlay.redraw.needs_draw());
    assert!(first_row(&overlay).starts_with("NEW"));
    overlay.redraw.drawn();
    assert_eq!(overlay.deadline(), None);
    assert_eq!(overlay.pending.take(), Some(raw.to_vec()));
}

// Full-screen output has no visible background and must neither schedule redraws nor discard replay bytes.
#[test]
fn full_screen_output_does_not_wake_drawing() {
    let now = Instant::now();
    let screen = Vt100Screen::new(TermSize {
        cols: 100,
        rows: 30,
    });
    let mut overlay = overlay(crate::OverlayLayout::FullScreen, now, &screen);
    overlay.hold(b"output");
    overlay.update_background(&screen, now + Duration::from_secs(1));
    assert!(!overlay.redraw.needs_draw());
    assert_eq!(overlay.deadline(), None);
    assert_eq!(overlay.pending.take(), Some(b"output".to_vec()));
}
