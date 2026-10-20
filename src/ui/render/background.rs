use ratatui::{Frame, layout::Rect, text::Line, widgets::Paragraph};
use unicode_width::UnicodeWidthChar;

use crate::ui::theme;

pub(super) fn draw(rows: &[String], frame: &mut Frame<'_>, window: Rect) {
    let area = frame.area();
    let lines: Vec<_> = rows
        .iter()
        .take(usize::from(area.height))
        .enumerate()
        .map(|(row, text)| {
            let y = area.y + row as u16;
            let text = if (window.y..window.bottom()).contains(&y) {
                clip_edges(text, area.x, window)
            } else {
                text.clone()
            };
            Line::raw(text)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).style(theme::background()), area);
}

// Erase both halves of a wide character crossing the border before drawing the window over it.
fn clip_edges(text: &str, start: u16, window: Rect) -> String {
    let mut x = usize::from(start);
    let mut clipped = false;
    text.chars().fold(String::new(), |mut line, ch| {
        let width = ch.width().unwrap_or(0);
        if width > 0 {
            clipped = [window.x, window.right()]
                .into_iter()
                .any(|edge| x < usize::from(edge) && x + width > usize::from(edge));
        }
        if clipped {
            line.extend(std::iter::repeat_n(' ', width));
        } else {
            line.push(ch);
        }
        x += width;
        line
    })
}
