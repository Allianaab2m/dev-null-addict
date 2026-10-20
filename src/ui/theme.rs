use ratatui::style::{Color, Modifier, Style};

pub(super) fn accent() -> Style {
    Style::default().fg(Color::Cyan)
}

pub(super) fn muted() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub(super) fn quote() -> Style {
    Style::default().fg(Color::Gray)
}

pub(super) fn selected_header() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

pub(super) fn selection_bar(focused: bool) -> Style {
    if focused { accent() } else { muted() }
}

pub(super) fn stars(starred: bool) -> Style {
    if starred {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    }
}

pub(super) fn active_tab() -> Style {
    accent().add_modifier(Modifier::BOLD)
}

/// Filled, unlike the list's selection: reversed accent, so the label takes
/// the terminal's own background color and stays readable on any theme.
pub(super) fn focused_button() -> Style {
    accent().add_modifier(Modifier::BOLD | Modifier::REVERSED)
}

pub(super) fn background() -> Style {
    Style::default().fg(Color::DarkGray)
}
