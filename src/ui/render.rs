mod background;
mod commit;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::{
    App, Focus, help, layout,
    popup::{Notice, Popup},
    theme,
};
use crate::api::{CommitEntry, CommitTarget};

/// The name of a log on its tab, as /dev/null shows it.
fn tab_label(kind: crate::api::LogKind) -> &'static str {
    match kind {
        crate::api::LogKind::Following => "Following",
        crate::api::LogKind::Latest => "Latest",
        crate::api::LogKind::Trending => "Trending",
    }
}

/// Draws the overlay.
///
/// From top to bottom: the target line (when replying or quoting), the
/// composer, a line with the counter and Send, the log tabs, and the
/// timeline or the innermost thread. With at least
/// [`help::MIN_WIDTH`](super::help::MIN_WIDTH) columns the keys go down the
/// right edge. Popups go over everything. The cursor is put where the draft
/// is edited, so that an input method shows its candidates there.
///
/// Each timeline commit shows the author, the relative time and the short
/// hash, then at most [`FOLDED_LINES`](super::format::FOLDED_LINES) lines of
/// its body and how many more there are. Threads show bodies in full.
pub fn draw(app: &App, frame: &mut Frame<'_>, now: jiff::Timestamp) {
    let area = frame.area();
    let areas = layout::areas(app, area);
    if areas.window.floating {
        background::draw(&app.background, frame, areas.window.outer);
        frame.render_widget(Clear, areas.window.outer);
        frame.render_widget(Block::default().borders(Borders::ALL), areas.window.outer);
    }
    draw_composer(app, frame, &areas);
    draw_tabs(app, frame, areas.tabs);
    draw_list(app, frame, areas.list, now);
    if areas.help.width > 0 {
        draw_help(app, frame, areas.help);
    }
    draw_popup(app, frame, areas.window.inner);
}

fn draw_composer(app: &App, frame: &mut Frame<'_>, areas: &layout::Areas) {
    if let Some(target) = app.compose.target() {
        let verb = match target.target {
            CommitTarget::Reply(_) => "Reply to",
            CommitTarget::Quote(_) => "Quote",
        };
        frame.render_widget(
            Paragraph::new(format!("{verb} @{} {}", target.author, target.short_hash))
                .style(theme::accent()),
            areas.target,
        );
    }
    let composer = Block::default()
        .borders(Borders::ALL)
        .title(" /dev/null ")
        .border_style(if app.focus == Focus::Compose {
            theme::accent()
        } else {
            Style::default()
        });
    let inner = composer.inner(areas.composer);
    frame.render_widget(composer, areas.composer);
    app.compose.render(
        frame,
        inner,
        app.focus == Focus::Compose && app.popup.is_none(),
    );
    let send_style = if app.focus == Focus::Send {
        theme::focused_button()
    } else if app.compose.to_new_commit().is_none() {
        theme::muted()
    } else {
        theme::accent()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(format!("{} / 4000  ", app.compose.char_count())),
            Span::styled(
                if app.sending {
                    " Sending... "
                } else {
                    " Send "
                },
                send_style,
            ),
        ])),
        areas.send,
    );
}

fn draw_tabs(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let tabs: Vec<_> = crate::api::LogKind::ALL
        .into_iter()
        .map(|kind| {
            Span::styled(
                format!(" {} ", tab_label(kind)),
                if kind == app.timeline.active() {
                    theme::active_tab()
                } else {
                    Style::default()
                },
            )
        })
        .collect();
    frame.render_widget(Paragraph::new(Line::from(tabs)), area);
}

pub(super) fn entry_height(entry: &CommitEntry, width: u16, full: bool) -> usize {
    commit::lines(
        entry,
        width.saturating_sub(2),
        full,
        jiff::Timestamp::UNIX_EPOCH,
    )
    .len()
}

fn draw_list(app: &App, frame: &mut Frame<'_>, area: Rect, now: jiff::Timestamp) {
    if let Some(notice) = app.notice {
        let message = match notice {
            Notice::SignedOut => {
                "Not signed in. Run `dev-null-addict login` in another shell, then return here."
            }
            Notice::NotApproved => {
                "This device is not approved. Run `dev-null-addict login` again."
            }
        };
        frame.render_widget(Paragraph::new(message).wrap(Wrap { trim: false }), area);
        return;
    }
    let entries = app.list_entries();
    let selection = app.selection();
    let full = !app.threads.is_empty();
    if entries.is_empty() {
        let loaded = app.threads.last().map_or_else(
            || app.timeline.tab(app.timeline.active()).list.is_loaded(),
            |view| view.is_loaded() && view.replies().is_loaded(),
        );
        frame.render_widget(
            Paragraph::new(if loaded {
                "No commits yet."
            } else {
                "Loading..."
            }),
            area,
        );
        return;
    }
    let visible: Vec<_> = entries
        .iter()
        .flat_map(|entry| {
            let selected = Some(entry.id()) == selection.id();
            let mut lines = commit::lines(entry, area.width.saturating_sub(2), full, now);
            if selected && let Some(header) = lines.first_mut() {
                header.style = header.style.patch(theme::selected_header());
            }
            lines.into_iter().map(move |mut line| {
                // Keep the marker's style separate from the header and body.
                let style = line.style;
                line.spans
                    .iter_mut()
                    .for_each(|span| span.style = style.patch(span.style));
                line.style = Style::default();
                line.spans.insert(
                    0,
                    if selected {
                        Span::styled("│ ", theme::selection_bar(app.focus == Focus::List))
                    } else {
                        Span::raw("  ")
                    },
                );
                line
            })
        })
        .skip(app.viewport().top())
        .take(usize::from(area.height))
        .collect();
    frame.render_widget(Paragraph::new(visible), area);
}

fn draw_help(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let context = if matches!(app.popup, Some(Popup::ConfirmDelete { .. })) {
        help::HelpContext::ConfirmDelete
    } else {
        match app.focus {
            Focus::Compose => help::HelpContext::Compose,
            Focus::Send => help::HelpContext::Send,
            Focus::List if app.threads.is_empty() => help::HelpContext::Timeline,
            Focus::List => help::HelpContext::Thread,
        }
    };
    let lines: Vec<_> = help::entries(context, app.toggle).into_iter().filter(|(key, _)| key != "d" || matches!(app.selected(), Some(CommitEntry::Live(commit)) if app.me.as_ref() == Some(&commit.author.username))).map(|(key, action)| Line::from(vec![Span::styled(format!("{key:<10}"), theme::accent()), Span::raw(action)])).collect();
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::LEFT).title(" Keys "))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_popup(app: &App, frame: &mut Frame<'_>, area: Rect) {
    if let Some(popup) = &app.popup {
        let (title, text) = match popup {
            Popup::Toast { message, .. } => (" Notice ", message.clone()),
            Popup::ConfirmDelete { short_hash, .. } => (
                " Delete commit? ",
                format!("Delete {short_hash}?\nEnter deletes. Esc keeps it."),
            ),
        };
        let width = area.width.saturating_sub(4).clamp(1, 70).min(area.width);
        let height = (super::format::fold(&text, width.saturating_sub(2), usize::MAX)
            .0
            .len() as u16
            + 2)
        .min(area.height);
        let popup_area = ratatui::layout::Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, popup_area);
        frame.render_widget(
            Paragraph::new(text)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .border_style(theme::accent()),
                )
                .wrap(Wrap { trim: false }),
            popup_area,
        );
    }
}
