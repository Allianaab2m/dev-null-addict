/// A key the overlay acts on.
///
/// Key repeats count as presses and releases are dropped, whatever the
/// encoding. Other Ctrl and Alt combinations are not keys of the overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Delete,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
}

/// Input for the overlay.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum UiInput {
    Key(Key),
    /// The text of a bracketed paste, line endings as `\n`.
    Paste(String),
    /// A press of the toggle key.
    Toggle,
}
