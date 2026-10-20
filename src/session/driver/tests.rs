use super::*;
use crate::api::LogKind;
use crate::screen::QueryKind;
use std::time::Duration;

fn driver() -> Driver {
    let size = TermSize { cols: 80, rows: 24 };
    let toggle = "ctrl+g".parse().unwrap();
    let (requests, _) = Requests::new(
        "/nonexistent-test-key".into(),
        crate::api::BaseUrl::default(),
    );
    Driver {
        display: Display::captured(size),
        queue: InputQueue::default(),
        requests,
        overlay: Overlay::new(
            App::new(toggle, LogKind::Following, Duration::from_secs(5)),
            KeyDecoder::new(toggle),
        ),
        screen: Vt100Screen::new(size),
        framer: OutputFramer::new(),
        scanner: ToggleScanner::new(toggle),
        gate: ToggleGate::new(),
        opening: None,
        closing: None,
    }
}

// Every release split at the double-press deadline must reach the decoder before later draft text.
#[test]
fn timer_open_transfers_every_release_prefix() {
    let release = b"\x1b[103;5:3u";
    for split in 1..release.len() {
        let mut driver = driver();
        let now = Instant::now();
        driver.input(b"\x1b[103;5u", now).unwrap();
        driver
            .input(&release[..split], now + Duration::from_millis(299))
            .unwrap();
        driver.tick(now + Duration::from_millis(300)).unwrap();
        driver
            .input(
                &[&release[split..], b"X"].concat(),
                now + Duration::from_millis(301),
            )
            .unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                driver
                    .overlay
                    .app_mut()
                    .render(frame, jiff::Timestamp::UNIX_EPOCH)
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("1 / 4000"), "split {split}: {text}");
        assert!(driver.queue.is_empty());
        assert!(driver.scanner.take_held().is_empty());
    }
}

// Closing between any CSI, OSC or UTF-8 bytes must produce the same terminal as uninterrupted output.
#[test]
fn replay_close_preserves_partial_output() {
    for raw in [
        &b"\x1b[31mRED"[..],
        b"\x1b]0;title\x07RED",
        "界RED".as_bytes(),
    ] {
        for split in 1..raw.len() {
            let mut driver = driver();
            let now = Instant::now();
            let mut outer = Vt100Screen::new(TermSize { cols: 80, rows: 24 });
            driver.input(b"\x07", now).unwrap();
            driver.tick(now + Duration::from_millis(300)).unwrap();
            outer.process(&driver.display.take_bytes());
            driver.output(&raw[..split], now).unwrap();
            driver.close().unwrap();
            let mut closing = driver.display.take_bytes();
            driver.output(&raw[split..], now).unwrap();
            closing.extend(driver.display.take_bytes());
            assert!(
                closing.ends_with(raw),
                "replay bytes changed at split {split}"
            );
            outer.process(&closing);
            let mut expected = Vt100Screen::new(TermSize { cols: 80, rows: 24 });
            expected.process(raw);
            assert_eq!(
                outer.text_rows(),
                expected.text_rows(),
                "split {split}: {raw:?}"
            );
            assert_eq!(
                outer.answer(QueryKind::CursorPosition, b"\x1b[6n"),
                expected.answer(QueryKind::CursorPosition, b"\x1b[6n")
            );
        }
    }
}

// Redraw close waits for each fragmented output boundary and preserves input order during that wait.
#[test]
fn redraw_close_waits_for_output_boundary() {
    for raw in [
        &b"\x1b[31mRED"[..],
        b"\x1b]0;title\x07RED",
        "界RED".as_bytes(),
    ] {
        for split in 1..raw.len() {
            let mut driver = driver();
            let now = Instant::now();
            let mut outer = Vt100Screen::new(TermSize { cols: 80, rows: 24 });
            driver.output(b"\x1b[?1049h", now).unwrap();
            outer.process(&driver.display.take_bytes());
            driver.input(b"\x07", now).unwrap();
            driver.tick(now + Duration::from_millis(300)).unwrap();
            outer.process(&driver.display.take_bytes());
            driver.output(&raw[..split], now).unwrap();
            let partial = !driver.framer.at_boundary();
            driver.input(b"\x07first", now).unwrap();
            if partial {
                assert!(driver.overlay.is_open());
                driver
                    .input(b"second", now + Duration::from_millis(1))
                    .unwrap();
            }
            driver
                .output(&raw[split..], now + Duration::from_millis(2))
                .unwrap();
            assert!(!driver.overlay.is_open());
            outer.process(&driver.display.take_bytes());
            let mut expected = Vt100Screen::new(TermSize { cols: 80, rows: 24 });
            expected.process(b"\x1b[?1049h");
            expected.process(raw);
            assert_eq!(outer.text_rows(), expected.text_rows(), "split {split}");
            let mut input = Vec::new();
            while !driver.queue.is_empty() {
                let bytes = driver.queue.front();
                let len = bytes.len();
                input.extend_from_slice(bytes);
                driver.queue.consume(len);
            }
            assert_eq!(
                input,
                if partial {
                    b"firstsecond".as_slice()
                } else {
                    b"first"
                }
            );
        }
    }
}

// The startup CPR seeds background rows without emitting a clear or moving the physical cursor.
#[test]
fn startup_cursor_seeds_nonzero_row_without_touching_display() {
    let mut driver = driver();
    driver.seed_cursor(Some((9, 6)));
    assert!(driver.display.take_bytes().is_empty());
    driver.output(b"PROMPT", Instant::now()).unwrap();
    assert_eq!(driver.screen.text_rows()[9], "      PROMPT");
    assert_eq!(
        driver.screen.answer(QueryKind::CursorPosition, b"\x1b[6n"),
        Some(b"\x1b[10;13R".to_vec())
    );
    assert_eq!(driver.display.take_bytes(), b"PROMPT");
}

// Overflow and resize force redraw too, and an unfinished sequence must wait until the bounded close deadline.
#[test]
fn redraw_close_timeout_is_bounded_after_overflow_or_resize() {
    for overflow in [false, true] {
        let mut driver = driver();
        let now = Instant::now();
        driver.input(b"\x07", now).unwrap();
        driver.tick(now + Duration::from_millis(300)).unwrap();
        if overflow {
            driver
                .overlay
                .hold(&vec![b' '; super::super::pending::PendingOutput::LIMIT + 1]);
        } else {
            driver.resize(TermSize { cols: 70, rows: 20 }).unwrap();
        }
        driver.output(b"\x1b[31", now).unwrap();
        driver.input(b"\x07", now).unwrap();
        assert!(driver.overlay.is_open());
        driver
            .tick(now + OutputFramer::BOUNDARY_TIMEOUT - Duration::from_millis(1))
            .unwrap();
        assert!(driver.overlay.is_open());
        driver.tick(now + OutputFramer::BOUNDARY_TIMEOUT).unwrap();
        assert!(!driver.overlay.is_open());
        assert!(driver.framer.at_boundary());
        driver.output(b"\r\nAFTER", now).unwrap();
        assert!(
            driver
                .screen
                .text_rows()
                .iter()
                .any(|row| row.contains("AFTER"))
        );
    }
}
