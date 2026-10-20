use crate::terminal::TermSize;

use super::state::ExtraModes;
use super::{Buffer, QueryKind, ScreenModel};

/// [`ScreenModel`] on the `vt100` crate, without scrollback.
///
/// The parser is made with callbacks that track what `vt100` does not keep
/// itself in [`ExtraModes`]. A second parser observes handled sequences whose
/// state is not exposed by vt100, including margins and origin mode.
pub struct Vt100Screen {
    parser: vt100::Parser<ModeCallbacks>,
    handled: vte::Parser,
}

#[derive(Debug, Default)]
struct ModeCallbacks {
    modes: ExtraModes,
}

impl vt100::Callbacks for ModeCallbacks {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        params: &[&[u16]],
        action: char,
    ) {
        let intermediates: Vec<_> = i1.into_iter().chain(i2).collect();
        self.modes.csi(
            if screen.alternate_screen() {
                Buffer::Alternate
            } else {
                Buffer::Main
            },
            &intermediates,
            params,
            action,
        );
    }
}

struct HandledModes<'a> {
    modes: &'a mut ExtraModes,
    buffer: Buffer,
    rows: u16,
    completed: &'a mut bool,
}

// vt100 handles these sequences without callbacks, but does not expose their state.
impl vte::Perform for HandledModes<'_> {
    fn print(&mut self, _: char) {
        *self.completed = true;
    }
    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        if ignore {
            return;
        }
        *self.completed = true;
        let params: Vec<_> = params.iter().collect();
        match (intermediates, action) {
            (b"", 'r') => {
                let top = params
                    .first()
                    .and_then(|p| p.first())
                    .copied()
                    .unwrap_or(1)
                    .max(1);
                let bottom = params
                    .get(1)
                    .and_then(|p| p.first())
                    .copied()
                    .filter(|n| *n > 0)
                    .unwrap_or(self.rows);
                if top == 1 && bottom == self.rows {
                    self.modes.csi(self.buffer, b"", &[], 'r');
                } else if top < bottom && bottom <= self.rows {
                    self.modes.csi(self.buffer, b"", &[&[top], &[bottom]], 'r');
                }
            }
            (b" ", 'q') => self.modes.csi(self.buffer, intermediates, &params, action),
            (b"?", 'h' | 'l') => {
                let handled: Vec<_> = params
                    .into_iter()
                    .filter(|p| p.first() == Some(&6))
                    .collect();
                self.modes.csi(self.buffer, intermediates, &handled, action);
            }
            _ => {}
        }
    }
    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        *self.completed = true;
        if !ignore && intermediates.is_empty() && byte == b'c' {
            *self.modes = ExtraModes::default();
        }
    }
}

impl ScreenModel for Vt100Screen {
    fn text_rows(&self) -> Vec<String> {
        let screen = self.parser.screen();
        screen.rows(0, screen.size().1.get()).collect()
    }

    fn new(size: TermSize) -> Self {
        Self {
            parser: vt100::Parser::new_with_callbacks(
                size.rows.max(1).try_into().expect("positive rows"),
                size.cols.max(1).try_into().expect("positive columns"),
                0,
                ModeCallbacks::default(),
            ),
            handled: vte::Parser::new(),
        }
    }

    fn process(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.parser.process(std::slice::from_ref(byte));
            let buffer = self.buffer();
            let (rows, cols) = self.parser.screen().size();
            let mut completed = false;
            self.handled.advance(
                &mut HandledModes {
                    modes: &mut self.parser.callbacks_mut().modes,
                    buffer,
                    rows: rows.get(),
                    completed: &mut completed,
                },
                std::slice::from_ref(byte),
            );
            if completed
                && self.parser.callbacks().modes.mode(7) == Some(false)
                && self.parser.screen().cursor_position().1 >= cols.get()
            {
                self.parser.process(b"\x08");
            }
        }
    }

    fn resize(&mut self, size: TermSize) {
        self.parser.set_size(
            size.rows.max(1).try_into().expect("positive rows"),
            size.cols.max(1).try_into().expect("positive columns"),
        );
        self.parser.callbacks_mut().modes.resize();
    }

    fn buffer(&self) -> Buffer {
        if self.parser.screen().alternate_screen() {
            Buffer::Alternate
        } else {
            Buffer::Main
        }
    }

    fn restore(&self) -> Vec<u8> {
        let screen = self.parser.screen();
        let mut out = b"\x1b[?9l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l".to_vec();
        out.extend_from_slice(screen.input_mode_formatted().as_bytes());
        out.extend(self.parser.callbacks().modes.restore(self.buffer()));
        out.extend(cursor_restore(screen, &self.parser.callbacks().modes));
        out.extend_from_slice(screen.attributes_formatted().as_bytes());
        out
    }

    fn answer(&self, kind: QueryKind, query: &[u8]) -> Option<Vec<u8>> {
        let screen = self.parser.screen();
        let modes = &self.parser.callbacks().modes;
        match kind {
            QueryKind::CursorPosition => {
                let (row, col) = screen.cursor_position();
                Some(
                    format!(
                        "\x1b[{}{};{}R",
                        if query.starts_with(b"\x1b[?") {
                            "?"
                        } else {
                            ""
                        },
                        modes.cursor_row(row) + 1,
                        col.min(screen.size().1.get() - 1) + 1
                    )
                    .into_bytes(),
                )
            }
            QueryKind::KeyboardFlags => {
                Some(format!("\x1b[?{}u", modes.keyboard_flags(self.buffer())).into_bytes())
            }
            QueryKind::Mode => {
                let text = std::str::from_utf8(query)
                    .ok()?
                    .strip_prefix("\x1b[")?
                    .strip_suffix("$p")?;
                let private = text.starts_with('?');
                let number: u16 = text.trim_start_matches('?').parse().ok()?;
                let value = if private {
                    modes.mode(number).or_else(|| match number {
                        1 => Some(screen.application_cursor()),
                        25 => Some(!screen.hide_cursor()),
                        47 | 1047 | 1049 => Some(screen.alternate_screen()),
                        2004 => Some(screen.bracketed_paste()),
                        9 => Some(screen.mouse_protocol_mode() == vt100::MouseProtocolMode::Press),
                        1000 => Some(
                            screen.mouse_protocol_mode() == vt100::MouseProtocolMode::PressRelease,
                        ),
                        1002 => Some(
                            screen.mouse_protocol_mode() == vt100::MouseProtocolMode::ButtonMotion,
                        ),
                        1003 => Some(
                            screen.mouse_protocol_mode() == vt100::MouseProtocolMode::AnyMotion,
                        ),
                        1005 => Some(
                            screen.mouse_protocol_encoding() == vt100::MouseProtocolEncoding::Utf8,
                        ),
                        1006 => Some(
                            screen.mouse_protocol_encoding() == vt100::MouseProtocolEncoding::Sgr,
                        ),
                        _ => None,
                    })
                } else {
                    None
                };
                Some(
                    format!(
                        "\x1b[{}{number};{}$y",
                        if private { "?" } else { "" },
                        value.map_or(0, |v| if v { 1 } else { 2 })
                    )
                    .into_bytes(),
                )
            }
            _ => None,
        }
    }

    fn redraw(&self) -> Vec<u8> {
        let mut bytes = self.parser.screen().contents_formatted().into_bytes();
        bytes.extend_from_slice(super::BASELINE);
        bytes.extend_from_slice(self.parser.screen().cursor_state_formatted().as_bytes());
        bytes.extend_from_slice(b"\x1b[m");
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn screen() -> Vt100Screen {
        Vt100Screen::new(TermSize { cols: 80, rows: 24 })
    }

    // Queries use the hidden shell's cursor and modes, never the overlay's state.
    #[test]
    fn answers_local_queries() {
        let mut screen = screen();
        screen.process(b"\x1b[2;20r\x1b[?6h\x1b[3;4H\x1b[?7l\x1b[?1004h\x1b[?2004h\x1b[>7u");
        assert_eq!(
            screen.answer(QueryKind::CursorPosition, b"\x1b[6n"),
            Some(b"\x1b[3;4R".to_vec())
        );
        assert_eq!(
            screen.answer(QueryKind::CursorPosition, b"\x1b[?6n"),
            Some(b"\x1b[?3;4R".to_vec())
        );
        assert_eq!(
            screen.answer(QueryKind::KeyboardFlags, b"\x1b[?u"),
            Some(b"\x1b[?7u".to_vec())
        );
        for (query, answer) in [
            (&b"\x1b[?7$p"[..], &b"\x1b[?7;2$y"[..]),
            (b"\x1b[?1004$p", b"\x1b[?1004;1$y"),
            (b"\x1b[?2004$p", b"\x1b[?2004;1$y"),
            (b"\x1b[?9999$p", b"\x1b[?9999;0$y"),
        ] {
            assert_eq!(
                screen.answer(QueryKind::Mode, query).as_deref(),
                Some(answer)
            );
        }
        assert_eq!(screen.answer(QueryKind::Terminal, b"\x1b[c"), None);
    }

    // A redraw plus restore must reproduce contents, input modes and both kitty stack levels.
    #[test]
    fn redraw_and_restore_round_trip() {
        let mut original = screen();
        original.process(b"hello\r\n\x1b[31mworld\x1b[?1h\x1b=\x1b[?2004h\x1b[?1003h\x1b[?1006h\x1b[?1004h\x1b[5 q\x1b[=1u\x1b[>3u\x1b[>4;2m");
        let mut restored = screen();
        restored.process(super::super::BASELINE);
        restored.process(&original.redraw());
        restored.process(&original.restore());
        assert_eq!(
            restored.parser.screen().contents(),
            original.parser.screen().contents()
        );
        assert_eq!(
            restored.parser.screen().cursor_position(),
            original.parser.screen().cursor_position()
        );
        assert_eq!(
            restored.parser.screen().input_mode_formatted(),
            original.parser.screen().input_mode_formatted()
        );
        assert_eq!(
            restored.parser.callbacks().modes,
            original.parser.callbacks().modes
        );
        restored.process(b"\x1b[<u");
        assert_eq!(
            restored.answer(QueryKind::KeyboardFlags, b"\x1b[?u"),
            Some(b"\x1b[?1u".to_vec())
        );
    }

    // Main and alternate screen flags must remain independent across hidden application transitions.
    #[test]
    fn buffer_flags_and_reset() {
        let mut screen = screen();
        screen.process(b"\x1b[=3u\x1b[?1049h\x1b[=7u");
        assert_eq!(screen.buffer(), Buffer::Alternate);
        screen.process(b"\x1b[?1049l");
        assert_eq!(screen.buffer(), Buffer::Main);
        assert_eq!(
            screen.answer(QueryKind::KeyboardFlags, b"\x1b[?u"),
            Some(b"\x1b[?3u".to_vec())
        );
        screen.process(b"\x1bc");
        assert_eq!(screen.parser.callbacks().modes, ExtraModes::default());
    }
}

fn cursor_restore(screen: &vt100::Screen, modes: &ExtraModes) -> Vec<u8> {
    let cursor = screen.cursor_state_formatted();
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < cursor.len() {
        let rest = &cursor.as_bytes()[pos..];
        if rest.starts_with(b"\x1b[")
            && let Some(end) = rest[2..]
                .iter()
                .position(|b| (0x40..=0x7e).contains(b))
                .map(|n| n + 2)
        {
            if rest[end] == b'H' {
                let params = &cursor[pos + 2..pos + end];
                let mut params = params.split(';');
                let row = params
                    .next()
                    .and_then(|p| p.parse::<u16>().ok())
                    .unwrap_or(1)
                    .max(1);
                let col = params
                    .next()
                    .and_then(|p| p.parse::<u16>().ok())
                    .unwrap_or(1)
                    .max(1);
                out.extend_from_slice(
                    format!("\x1b[{};{col}H", modes.cursor_row(row - 1) + 1).as_bytes(),
                );
            } else {
                out.extend_from_slice(&rest[..=end]);
            }
            pos += end + 1;
        } else {
            out.push(rest[0]);
            pos += 1;
        }
    }
    out
}

#[cfg(test)]
mod cursor_tests {
    use super::*;

    // The delayed wrap flag cannot be restored with CUP alone; the next character must wrap identically.
    #[test]
    fn restore_preserves_delayed_wrap_with_origin_mode() {
        let size = TermSize { cols: 5, rows: 8 };
        for setup in [&b""[..], b"\x1b[2;7r\x1b[?6h"] {
            let mut original = Vt100Screen::new(size);
            original.process(setup);
            original.process(b"\x1b[1;1Habcde");
            assert_eq!(
                original.answer(QueryKind::CursorPosition, b"\x1b[6n"),
                Some(b"\x1b[1;5R".to_vec())
            );
            let mut restored = Vt100Screen::new(size);
            restored.process(super::super::BASELINE);
            restored.process(&original.redraw());
            restored.process(&original.restore());
            original.process(b"x");
            restored.process(b"x");
            assert_eq!(
                original.parser.screen().contents(),
                restored.parser.screen().contents()
            );
            assert_eq!(
                original.parser.screen().cursor_position(),
                restored.parser.screen().cursor_position()
            );
        }
    }
}

#[cfg(test)]
mod margin_tests {
    use super::*;

    // Omitted bottom margins mean the final row, and disabled autowrap overwrites the last cell.
    #[test]
    fn default_bottom_margin_and_disabled_wrap() {
        let mut screen = Vt100Screen::new(TermSize { cols: 5, rows: 8 });
        screen.process(b"\x1b[2r\x1b[?6h\x1b[?7l\x1b[1;1Habcdef");
        assert_eq!(
            screen.parser.screen().contents(),
            "\nabcd f".replace(' ', "")
        );
        assert_eq!(
            screen.answer(QueryKind::CursorPosition, b"\x1b[6n"),
            Some(b"\x1b[1;5R".to_vec())
        );
        assert!(
            String::from_utf8(screen.restore())
                .unwrap()
                .contains("\x1b[2;8r")
        );
    }
}

#[cfg(test)]
mod text_tests {
    use super::*;

    // Background rows keep physical wrapping, gaps, wide and combining characters, but strip all terminal attributes.
    #[test]
    fn plain_rows_preserve_cell_positions_and_current_buffer() {
        let mut screen = Vt100Screen::new(TermSize { cols: 10, rows: 4 });
        screen.process("\x1b[31;44;1;4mA界B e\u{301}\x1b[2;4HZ\x1b[3;1H0123456789XY".as_bytes());
        assert_eq!(
            screen.text_rows(),
            ["A界B e\u{301}", "   Z", "0123456789", "XY"]
        );
        screen.process(b"\x1b[?1049h\x1b[2J\x1b[HALT");
        assert_eq!(screen.text_rows(), ["ALT", "", "", ""]);
        screen.process(b"\x1b[?1049l");
        assert_eq!(screen.text_rows()[0], "A界B e\u{301}");
        screen.resize(TermSize { cols: 12, rows: 5 });
        assert_eq!(screen.text_rows().len(), 5);
        assert_eq!(screen.text_rows()[0], "A界B e\u{301}");
    }
}
