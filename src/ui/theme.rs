use ratatui::style::{Color, Modifier, Style};

pub(super) fn accent() -> Style {
    Style::default().fg(Color::Cyan)
}

/// A username, in the lane color its user picked, or the accent when there
/// is none or it is not `#rrggbb`.
pub(super) fn user(lane: Option<&str>) -> Style {
    let rgb = lane
        .and_then(|lane| lane.strip_prefix('#'))
        .filter(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .and_then(|hex| u32::from_str_radix(hex, 16).ok());
    match rgb {
        Some(rgb) => Style::default()
            .fg(Color::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
            .add_modifier(Modifier::BOLD),
        None => accent(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    // Only `#rrggbb` lanes color a username; anything else falls back to the accent.
    #[test]
    fn user_lane_colors() {
        assert_eq!(
            user(Some("#1A2b3c")),
            Style::default()
                .fg(Color::Rgb(0x1a, 0x2b, 0x3c))
                .add_modifier(Modifier::BOLD)
        );
        for lane in [
            None,
            Some(""),
            Some("1a2b3c"),
            Some("#1a2b3"),
            Some("#+1a2b3"),
            Some("#1a2b3g"),
        ] {
            assert_eq!(user(lane), accent());
        }
    }
}
