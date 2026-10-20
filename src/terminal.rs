//! The outer terminal: the one the wrapper itself runs in.

use std::io;

/// Columns and rows of a terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
}

impl TermSize {
    /// The outer terminal's size.
    pub fn current() -> io::Result<Self> {
        let (cols, rows) = ratatui::crossterm::terminal::size()?;
        Ok(Self {
            cols: cols.max(1),
            rows: rows.max(1),
        })
    }
}

/// Keeps the outer terminal in raw mode while it lives.
///
/// The terminal is put back by [`restore`] when this is dropped, and also from
/// a panic hook installed by [`RawTerminal::enter`], before the panic message
/// is printed.
#[derive(Debug)]
pub struct RawTerminal {
    _private: (),
}

impl RawTerminal {
    pub fn enter() -> io::Result<Self> {
        static HOOK: std::sync::Once = std::sync::Once::new();
        HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                restore();
                previous(info);
            }));
        });
        ratatui::crossterm::terminal::enable_raw_mode()?;
        Ok(Self { _private: () })
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        restore();
    }
}

/// Puts the outer terminal back as the wrapper found it: writes the bytes
/// last given to [`set_cleanup`], if any, then leaves raw mode.
///
/// Safe to call more than once and from the panic hook.
pub fn restore() {
    use std::io::Write;
    let bytes = CLEANUP.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(bytes) = bytes {
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(&bytes).and_then(|()| stdout.flush());
    }
    let _ = ratatui::crossterm::terminal::disable_raw_mode();
}

/// Sets the bytes [`restore`] writes before it leaves raw mode, or clears
/// them with `None`.
///
/// The overlay sets them when it takes the screen over, to undo what it did
/// (leave the buffer it entered, show the cursor, turn off what it turned
/// on), and clears them when it gives the screen back.
pub fn set_cleanup(bytes: Option<Vec<u8>>) {
    *CLEANUP.lock().unwrap_or_else(|e| e.into_inner()) = bytes;
}

static CLEANUP: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);
