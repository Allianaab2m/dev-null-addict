use std::io::{self, Stdout};

use ratatui::backend::CrosstermBackend;
use ratatui::{Frame, Terminal};

use crate::terminal::TermSize;

/// The outer terminal's output. Everything the wrapper writes there goes
/// through here: the shell's output while the overlay is closed, and the
/// overlay's drawing.
///
/// The ratatui terminal has a fixed viewport, so it never asks the terminal
/// for its size or cursor position by itself; the answer would come on
/// stdin, which the input thread reads. Its buffers are reset with `resize`
/// when the overlay takes the screen over. It is not resized while the
/// overlay is closed, since resizing clears the screen.
pub struct Display {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    size: TermSize,
}

impl Display {
    pub fn new(size: TermSize) -> io::Result<Self> {
        let terminal = Terminal::with_options(
            CrosstermBackend::new(std::io::stdout()),
            ratatui::TerminalOptions {
                viewport: ratatui::Viewport::Fixed(ratatui::layout::Rect::new(
                    0, 0, size.cols, size.rows,
                )),
            },
        )?;
        Ok(Self { terminal, size })
    }

    pub(super) fn size(&self) -> TermSize {
        self.size
    }

    /// Writes bytes as they are, and flushes.
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        self.terminal.backend_mut().write_all(bytes)?;
        self.terminal.backend_mut().flush()
    }

    /// Takes note of a new size of the outer terminal. While the overlay is
    /// open, the ratatui terminal follows it at once.
    pub fn resize(&mut self, size: TermSize, overlay_open: bool) -> io::Result<()> {
        self.size = size;
        if overlay_open {
            self.take_over()?;
        }
        Ok(())
    }

    /// Resets ratatui's buffers to the whole screen at the current size, so
    /// that the next draw draws everything.
    pub fn take_over(&mut self) -> io::Result<()> {
        self.terminal.resize(ratatui::layout::Rect::new(
            0,
            0,
            self.size.cols,
            self.size.rows,
        ))
    }

    pub fn draw(&mut self, render: impl FnOnce(&mut Frame<'_>)) -> io::Result<()> {
        self.terminal.draw(render).map(|_| ())
    }
}
