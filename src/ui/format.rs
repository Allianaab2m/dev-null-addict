use jiff::Timestamp;

/// Lines of a commit's body the timeline shows before it folds the rest.
pub const FOLDED_LINES: usize = 5;

/// How long ago `at` was: `now`, `42s`, `5m`, `3h`, `6d`, and the date
/// (`2026-10-20`, local time) from a week on.
pub fn relative_time(at: Timestamp, now: Timestamp) -> String {
    let seconds = now.as_second().saturating_sub(at.as_second());
    match seconds {
        ..=0 => "now".to_owned(),
        1..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        3600..86400 => format!("{}h", seconds / 3600),
        86400..604800 => format!("{}d", seconds / 86400),
        _ => at.to_zoned(jiff::tz::TimeZone::system()).date().to_string(),
    }
}

/// Wraps `text` at `width` columns, counting wide characters as two, and
/// keeps the first `max_lines` lines. Returns them and how many lines were
/// left out.
pub fn fold(text: &str, width: u16, max_lines: usize) -> (Vec<String>, usize) {
    use unicode_width::UnicodeWidthChar;
    let width = usize::from(width.max(1));
    let mut lines = Vec::new();
    for logical in text.split('\n') {
        let mut line = String::new();
        let mut columns = 0;
        for c in logical.chars() {
            let size = c.width().unwrap_or(0);
            if columns + size > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                columns = 0;
            }
            line.push(c);
            columns += size;
        }
        lines.push(line);
    }
    let omitted = lines.len().saturating_sub(max_lines);
    lines.truncate(max_lines);
    (lines, omitted)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Boundary values avoid off-by-one changes of unit and future timestamps never become negative.
    #[test]
    fn relative_time_boundaries() {
        let now: Timestamp = "2026-10-21T12:00:00Z".parse().unwrap();
        for (seconds, expected) in [
            (-1, "now"),
            (0, "now"),
            (1, "1s"),
            (59, "59s"),
            (60, "1m"),
            (3599, "59m"),
            (3600, "1h"),
            (86399, "23h"),
            (86400, "1d"),
            (604799, "6d"),
        ] {
            let at = Timestamp::from_second(now.as_second() - seconds).unwrap();
            assert_eq!(relative_time(at, now), expected);
        }
        let at: Timestamp = "2026-10-01T12:00:00Z".parse().unwrap();
        assert_eq!(
            relative_time(at, now),
            at.to_zoned(jiff::tz::TimeZone::system()).date().to_string()
        );
    }

    // Folding counts display columns and all omitted wrapped lines, including explicit blank lines.
    #[test]
    fn fold_wide_combining_and_blank_lines() {
        assert_eq!(
            fold("ab界cd\n\nx\ny\nz", 4, 3),
            (vec!["ab界".to_owned(), "cd".to_owned(), "".to_owned()], 3)
        );
        assert_eq!(
            fold("e\u{301}界x", 3, 5),
            (vec!["e\u{301}界".to_owned(), "x".to_owned()], 0)
        );
        assert_eq!(fold("\n", 0, 5), (vec![String::new(), String::new()], 0));
        assert_eq!(fold("abcdef", 2, 0), (vec![], 3));
    }
}
