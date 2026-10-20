//! What the shell has drawn, and the state it left the terminal in.
//!
//! Every byte the shell writes goes through an [`OutputFramer`] and into a
//! [`ScreenModel`], whether the overlay is open or not. The framer knows
//! where sequences end and picks out queries to the terminal. The model
//! keeps a copy of the screen and of the terminal state, so that the overlay
//! can be drawn over the shell, answer queries while it hides the shell, and
//! give the screen back as the shell left it.

mod framer;
mod state;
mod vt100_screen;

pub use framer::{Frame, OutputFramer, QueryKind};
pub use state::Buffer;
pub use vt100_screen::Vt100Screen;

use crate::terminal::TermSize;

/// Bytes that bring a terminal to the state the overlay draws in, and that
/// [`ScreenModel::redraw`] assumes: scrolling region on the whole screen,
/// origin mode off, autowrap on, default colors and attributes.
pub const BASELINE: &[u8] = b"\x1b[r\x1b[?6l\x1b[?7h\x1b[m";

/// What the wrapper needs from a terminal emulator.
///
/// [`Vt100Screen`] is the one in use. Another, such as `alacritty_terminal`,
/// can take its place behind this trait.
pub trait ScreenModel {
    fn new(size: TermSize) -> Self
    where
        Self: Sized;

    /// Feeds output of the shell, in the order it reached, or would have
    /// reached, the outer terminal. Queries are fed too.
    fn process(&mut self, bytes: &[u8]);

    fn resize(&mut self, size: TermSize);

    /// Plain text for each physical row of the current buffer, without colors
    /// or attributes. Wide characters occur once, with their continuation
    /// cells omitted; empty cells before text remain spaces.
    fn text_rows(&self) -> Vec<String>;

    /// The buffer the program in the shell is on.
    fn buffer(&self) -> Buffer;

    /// Bytes that set every mode the program in the shell set, as it left
    /// them, on the buffer the outer terminal is on: cursor visibility and
    /// style, keypad and cursor key modes, bracketed paste, mouse and focus
    /// reporting, kitty keyboard flags, modifyOtherKeys, scrolling margins,
    /// origin mode and autowrap.
    ///
    /// Selecting the buffer is left to the caller, since switching buffers
    /// changes the screen.
    fn restore(&self) -> Vec<u8>;

    /// Answers a query of [`QueryKind::is_local`] the way the terminal would
    /// at this point of the output, from the copy rather than from the outer
    /// terminal, whose cursor and modes the overlay has changed. `None` if
    /// the copy can't answer it.
    fn answer(&self, kind: QueryKind, query: &[u8]) -> Option<Vec<u8>>;

    /// Bytes that draw the current screen of the shell, cursor position
    /// included.
    ///
    /// They assume the outer terminal is on the shell's buffer and at
    /// [`BASELINE`], and leave it there.
    fn redraw(&self) -> Vec<u8>;
}
