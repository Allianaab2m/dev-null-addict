use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui_textarea::TextArea;

use crate::api::{CommitTarget, NewCommit, ShortHash, Username};
use crate::input::Key;

/// The draft, and the commit it answers.
///
/// Enter is a line break. The box starts one line high and grows with the
/// draft, up to half the overlay, then scrolls.
#[derive(Debug)]
pub struct Compose {
    editor: TextArea<'static>,
    target: Option<Target>,
}

/// The commit a draft replies to or quotes, as the line above the draft shows
/// it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Target {
    pub target: CommitTarget,
    pub short_hash: ShortHash,
    pub author: Username,
}

impl Compose {
    pub fn new() -> Self {
        let mut editor = TextArea::default();
        editor.set_wrap_mode(ratatui_textarea::WrapMode::Glyph);
        editor.set_cursor_line_style(ratatui::style::Style::default());
        Self {
            editor,
            target: None,
        }
    }

    /// Edits the draft. Backspace on an empty draft drops the target.
    pub fn key(&mut self, key: Key) {
        use ratatui_textarea::{Input, Key as EditorKey};
        if key == Key::Backspace && self.is_empty() {
            self.target = None;
            return;
        }
        let key = match key {
            Key::Char(c) => EditorKey::Char(c),
            Key::Enter => EditorKey::Enter,
            Key::Tab => EditorKey::Tab,
            Key::Backspace => EditorKey::Backspace,
            Key::Delete => EditorKey::Delete,
            Key::Esc => EditorKey::Esc,
            Key::Up => EditorKey::Up,
            Key::Down => EditorKey::Down,
            Key::Left => EditorKey::Left,
            Key::Right => EditorKey::Right,
            Key::Home => EditorKey::Home,
            Key::End => EditorKey::End,
        };
        self.editor.input(Input {
            key,
            ctrl: false,
            alt: false,
            shift: false,
        });
    }

    pub fn paste(&mut self, text: &str) {
        self.editor.insert_str(text);
    }

    /// Replaces the target, keeping the draft.
    pub fn set_target(&mut self, target: Target) {
        self.target = Some(target);
    }

    pub fn target(&self) -> Option<&Target> {
        self.target.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.editor.is_empty()
    }

    /// The length /dev/null will count, for the counter next to Send.
    pub fn char_count(&self) -> usize {
        crate::api::CommitBody::measure(&self.editor.lines().join("\n"))
    }

    /// The commit to send, if /dev/null would accept the draft.
    pub fn to_new_commit(&self) -> Option<NewCommit> {
        Some(NewCommit {
            body: crate::api::CommitBody::new(&self.editor.lines().join("\n")).ok()?,
            target: self.target.as_ref().map(|t| t.target),
        })
    }

    /// Empties the draft and drops the target, once a commit went through.
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Lines the box needs at `width` columns: at least one, at most
    /// `max_height`.
    pub fn height(&self, width: u16, max_height: u16) -> u16 {
        let (lines, _) = super::format::fold(&self.editor.lines().join("\n"), width, usize::MAX);
        (lines.len().min(usize::from(max_height.max(1))) as u16).max(1)
    }

    /// Draws the draft in `area`, wrapped and scrolled to the cursor. When
    /// `focused`, it also puts the terminal's cursor where the draft is
    /// edited, so that an input method shows its candidates there.
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, focused: bool) {
        if area.is_empty() {
            return;
        }
        frame.render_widget(&self.editor, area);
        if focused {
            let cursor = self.editor.screen_cursor();
            let (row, col) = self.editor.scroll_offset();
            let x = cursor
                .col
                .saturating_sub(usize::from(col))
                .min(usize::from(area.width - 1)) as u16;
            let y = cursor
                .row
                .saturating_sub(usize::from(row))
                .min(usize::from(area.height - 1)) as u16;
            frame.set_cursor_position((area.x + x, area.y + y));
        }
    }
}
