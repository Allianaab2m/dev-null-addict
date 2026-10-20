use super::*;
use crate::input::Key;
use crate::test_support::{commit, id, page};

fn app() -> App {
    let mut app = App::new(
        "ctrl+g".parse().unwrap(),
        LogKind::Following,
        Duration::from_secs(5),
    );
    app.set_layout(layout::OverlayLayout::FullScreen);
    app
}
fn ready(app: &mut App, now: Instant) {
    app.open(now);
    app.answer(
        Request::Me,
        Ok(Response::User(User {
            username: "someone".into(),
            name: None,
        })),
        now,
    );
    app.answer(
        Request::Log {
            kind: LogKind::Following,
            cursor: None,
        },
        Ok(Response::Entries(page(&[1, 2], Some("next")))),
        now,
    );
}

// Send is skipped for short drafts, and successful sends reset the composer while moderation remains tracked.
#[test]
fn compose_focus_send_and_moderation() {
    let mut app = app();
    let now = Instant::now();
    ready(&mut app, now);
    app.input(UiInput::Key(Key::Tab), now);
    assert_eq!(app.focus, Focus::List);
    app.input(UiInput::Key(Key::Char('r')), now);
    assert_eq!(app.focus, Focus::Compose);
    assert!(app.compose.target().is_some());
    app.input(UiInput::Key(Key::Backspace), now);
    assert!(app.compose.target().is_none());
    app.input(UiInput::Paste("hello\nworld".to_owned()), now);
    app.input(UiInput::Key(Key::Tab), now);
    assert_eq!(app.focus, Focus::Send);
    let effects = app.input(UiInput::Key(Key::Enter), now);
    let Effect::Send(request @ Request::Create(_)) = effects[0].clone() else {
        panic!("create effect")
    };
    assert!(app.sending);
    assert!(app.input(UiInput::Key(Key::Enter), now).is_empty());
    let mut pending = commit(3);
    pending.status = crate::api::CommitStatus::Pending;
    app.answer(request, Ok(Response::Commit(pending)), now);
    assert!(app.compose.is_empty());
    assert_eq!(app.focus, Focus::Compose);
    assert_eq!(app.pending, vec![id(3)]);
    let effects = app.tick(now + Duration::from_secs(5));
    assert!(effects.contains(&Effect::Send(Request::Moderation(id(3)))));
    let mut rejected = commit(3);
    rejected.status = crate::api::CommitStatus::Rejected;
    app.answer(
        Request::Moderation(id(3)),
        Ok(Response::Thread(Thread {
            ancestors: Vec::new(),
            commit: rejected,
        })),
        now,
    );
    assert!(app.pending.is_empty());
    assert!(matches!(&app.popup, Some(Popup::Toast {message, ..}) if message.contains("rejected")));
    app.tick(now + Popup::TOAST_TIME);
    assert!(app.popup.is_none());
}

// Esc unwinds confirmation and nested threads; the toggle always closes and preserves draft and navigation.
#[test]
fn navigation_and_delete_confirmation() {
    let mut app = app();
    let now = Instant::now();
    ready(&mut app, now);
    app.input(UiInput::Key(Key::Tab), now);
    assert_eq!(
        app.input(UiInput::Key(Key::Enter), now),
        vec![
            Effect::Send(Request::Thread(id(1))),
            Effect::Send(Request::Replies {
                id: id(1),
                cursor: None
            })
        ]
    );
    app.answer(
        Request::Thread(id(1)),
        Ok(Response::Thread(Thread {
            ancestors: Vec::new(),
            commit: commit(1),
        })),
        now,
    );
    app.input(UiInput::Key(Key::Char('d')), now);
    assert!(matches!(app.popup, Some(Popup::ConfirmDelete { .. })));
    app.input(UiInput::Key(Key::Esc), now);
    assert!(app.popup.is_none());
    assert_eq!(app.threads.len(), 1);
    app.input(UiInput::Key(Key::Char('d')), now);
    assert_eq!(
        app.input(UiInput::Key(Key::Enter), now),
        vec![Effect::Send(Request::Delete(id(1)))]
    );
    app.input(UiInput::Toggle, now);
    assert_eq!(app.threads.len(), 1);
    assert!(app.tick(now + Duration::from_secs(60)).is_empty());
    app.open(now);
    assert_eq!(app.focus, Focus::Compose);
    app.input(UiInput::Key(Key::Esc), now);
    assert!(app.threads.is_empty());
    assert_eq!(app.input(UiInput::Key(Key::Esc), now), vec![Effect::Close]);
}

// Persistent login notices differ from transient key errors, and automatic requests obey Retry-After.
#[test]
fn failures_backoff_and_closed_answers() {
    let mut app = app();
    let now = Instant::now();
    app.open(now);
    app.answer(Request::Me, Err(Failure::SignedOut), now);
    assert_eq!(app.notice, Some(Notice::SignedOut));
    let request = Request::Log {
        kind: LogKind::Following,
        cursor: None,
    };
    app.answer(
        request.clone(),
        Err(Failure::Api(ApiError::Status {
            status: reqwest::StatusCode::TOO_MANY_REQUESTS,
            code: Some(crate::api::ErrorCode::RateLimited),
            message: "slow down".into(),
            retry_after: Some(Duration::from_secs(30)),
        })),
        now,
    );
    assert!(app.tick(now + Duration::from_secs(29)).is_empty());
    assert!(!app.tick(now + Duration::from_secs(30)).is_empty());
    app.input(UiInput::Toggle, now);
    app.pending.push(id(3));
    assert!(
        app.answer(
            Request::Moderation(id(3)),
            Ok(Response::Thread(Thread {
                ancestors: Vec::new(),
                commit: commit(3)
            })),
            now
        )
        .is_empty()
    );
    assert!(matches!(
        Failure::from(KeyError::Missing("missing".into())),
        Failure::SignedOut
    ));
    assert!(matches!(Failure::from(KeyError::NoHome), Failure::Key(_)));
}

// Fixed-size offscreen rendering verifies help, popups and the IME cursor without terminal I/O.
#[test]
fn renders_help_popups_and_cursor() {
    let mut app = app();
    let now = Instant::now();
    ready(&mut app, now);
    app.input(UiInput::Paste("hello world".into()), now);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| app.render(frame, "2026-10-21T12:00:00Z".parse().unwrap()))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("Keys"));
    assert!(text.contains("hello world"));
    assert!(text.contains("someone"));
    let cursor = terminal.get_cursor_position().unwrap();
    assert!(cursor.x > 1 && cursor.x < 96 && cursor.y < 15);
    app.popup = Some(Popup::ConfirmDelete {
        id: id(1),
        short_hash: id(1).into(),
    });
    terminal
        .draw(|frame| app.render(frame, jiff::Timestamp::now()))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("Delete commit?"));
    for (cols, rows) in [(1, 1), (5, 3), (80, 24)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(cols, rows)).unwrap();
        terminal
            .draw(|frame| app.render(frame, jiff::Timestamp::now()))
            .unwrap();
    }
}

fn scrolling_app(full: bool, cols: u16, rows: u16) -> App {
    let mut app = app();
    app.resize(crate::terminal::TermSize { cols, rows });
    let now = Instant::now();
    ready(&mut app, now);
    let entries: Vec<_> = [1, 2]
        .into_iter()
        .map(|n| {
            let mut commit = commit(n);
            commit.images.clear();
            commit.body = (0..10)
                .map(|line| format!("line {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            commit
        })
        .collect();
    if full {
        let mut view = ThreadView::new(id(1));
        view.set_thread(Thread {
            ancestors: Vec::new(),
            commit: entries[0].clone(),
        });
        view.refresh_replies(Page {
            items: vec![CommitEntry::Live(Box::new(entries[1].clone()))],
            next_cursor: Some("more".to_owned().into()),
        });
        app.threads.push(view);
    } else {
        app.timeline.tab_mut(LogKind::Following).list.refresh(Page {
            items: entries
                .into_iter()
                .map(|c| CommitEntry::Live(Box::new(c)))
                .collect(),
            next_cursor: Some("more".to_owned().into()),
        });
    }
    app.focus = Focus::List;
    app.sync_viewport();
    app
}

fn selected_offset(app: &App) -> usize {
    let area = layout::areas(app, app.size).list;
    let start: usize = app
        .list_entries()
        .iter()
        .take_while(|entry| Some(entry.id()) != app.selection().id())
        .map(|entry| render::entry_height(entry, area.width, !app.threads.is_empty()))
        .sum();
    app.viewport().top().saturating_sub(start)
}

fn drawn_list(app: &App) -> Vec<String> {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(
        app.size.width,
        app.size.height,
    ))
    .unwrap();
    let selection = *app.selection();
    let viewport = *app.viewport();
    terminal
        .draw(|frame| app.render(frame, jiff::Timestamp::UNIX_EPOCH))
        .unwrap();
    assert_eq!(
        *app.selection(),
        selection,
        "render must not change scrolling"
    );
    assert_eq!(
        *app.viewport(),
        viewport,
        "render must not change scrolling"
    );
    let area = layout::areas(app, app.size).list;
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect()
        })
        .collect()
}

// Both list types scroll one line before changing commits; moving upward enters the previous commit at its bottom.
#[test]
fn tall_commits_scroll_before_selection_moves() {
    let now = Instant::now();
    for full in [false, true] {
        let mut app = scrolling_app(full, 80, 9);
        assert_eq!(layout::areas(&app, app.size).list.height, 4);
        let limit = if full { 9 } else { 5 };
        assert!(drawn_list(&app)[0].contains("@someone"));
        for offset in 1..=limit {
            let key = if offset % 2 == 0 {
                Key::Down
            } else {
                Key::Char('j')
            };
            assert!(app.input(UiInput::Key(key), now).is_empty());
            assert_eq!(app.selection().id(), Some(id(1)));
            assert_eq!(selected_offset(&app), offset);
        }
        let bottom = drawn_list(&app);
        assert!(
            bottom
                .iter()
                .any(|line| line.contains("1 stars  0 replies"))
        );
        assert!(bottom.iter().all(|line| line.starts_with("│ ")));
        let effects = app.input(UiInput::Key(Key::Down), now);
        assert_eq!(app.selection().id(), Some(id(2)));
        assert_eq!(selected_offset(&app), 0);
        assert_eq!(
            effects.len(),
            1,
            "navigation to the last commit requests the next page"
        );
        assert!(drawn_list(&app)[0].contains("@someone"));
        app.input(UiInput::Key(Key::Char('k')), now);
        assert_eq!(app.selection().id(), Some(id(1)));
        assert_eq!(selected_offset(&app), limit);
        assert_eq!(drawn_list(&app), bottom);
        for offset in (0..limit).rev() {
            app.input(
                UiInput::Key(if offset % 2 == 0 {
                    Key::Up
                } else {
                    Key::Char('k')
                }),
                now,
            );
            assert_eq!(app.selection().id(), Some(id(1)));
            assert_eq!(selected_offset(&app), offset);
        }
        assert!(drawn_list(&app)[0].contains("@someone"));
        app.input(UiInput::Key(Key::Up), now);
        assert_eq!(app.selection().id(), Some(id(1)));
        assert_eq!(selected_offset(&app), 0);
    }
}

// Input uses current geometry before any draw, including the help column, composer growth, and the reply target.
#[test]
fn scrolling_uses_resizes_and_composer_layout_without_rendering() {
    let now = Instant::now();
    let mut app = scrolling_app(true, 100, 12);
    let mut long = commit(1);
    long.body = "x".repeat(600);
    long.images.clear();
    app.update_commit(CommitEntry::Live(Box::new(long)));
    let area = layout::areas(&app, app.size).list;
    assert_eq!((area.width, area.height), (76, 7));
    for _ in 0..5 {
        app.input(UiInput::Key(Key::Down), now);
    }
    assert_eq!(app.selection().id(), Some(id(1)));
    assert_eq!(selected_offset(&app), 5);

    app.resize(crate::terminal::TermSize { cols: 99, rows: 12 });
    assert_eq!(layout::areas(&app, app.size).list.width, 99);
    app.input(UiInput::Key(Key::Up), now);
    assert_eq!(
        selected_offset(&app),
        2,
        "resize clamps to 3 before scrolling up"
    );
    app.input(UiInput::Key(Key::Char('r')), now);
    app.input(UiInput::Paste("a\nb\nc".into()), now);
    assert_eq!(layout::areas(&app, app.size).list.height, 4);
    app.input(UiInput::Key(Key::Tab), now);
    app.input(UiInput::Key(Key::Tab), now);
    for offset in 3..=6 {
        app.input(UiInput::Key(Key::Down), now);
        assert_eq!(app.selection().id(), Some(id(1)));
        assert_eq!(selected_offset(&app), offset);
    }
    assert!(drawn_list(&app).iter().any(|line| line.contains("stars")));

    app.resize(crate::terminal::TermSize { cols: 1, rows: 1 });
    let selection = *app.selection();
    app.input(UiInput::Key(Key::Down), now);
    assert_eq!(*app.selection(), selection);
    app.resize(crate::terminal::TermSize { cols: 99, rows: 30 });
    app.input(UiInput::Key(Key::Down), now);
    assert_eq!(
        app.selection().id(),
        Some(id(2)),
        "a now-short commit moves directly"
    );
}

// Selected commits preserve foreground readability; only starred counts are yellow, and every line has a focus-aware bar.
#[test]
fn selection_and_counts_use_semantic_ansi_styles() {
    use ratatui::style::{Color, Modifier};
    let now = Instant::now();
    for full in [false, true] {
        for focus in [Focus::List, Focus::Compose] {
            for starred in [None, Some(false), Some(true)] {
                let mut app = scrolling_app(full, 80, 30);
                app.focus = focus;
                let mut selected = commit(1);
                selected.images.clear();
                selected.starred = starred;
                selected.body = "plain body".into();
                app.update_commit(CommitEntry::Live(Box::new(selected)));
                let mut terminal =
                    ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 30)).unwrap();
                terminal
                    .draw(|frame| app.render(frame, jiff::Timestamp::UNIX_EPOCH))
                    .unwrap();
                let area = layout::areas(&app, app.size).list;
                let buffer = terminal.backend().buffer();
                for row in area.y..area.y + 4 {
                    assert_eq!(buffer[(0, row)].symbol(), "│");
                    assert_eq!(
                        buffer[(0, row)].fg,
                        if focus == Focus::List {
                            Color::Cyan
                        } else {
                            Color::DarkGray
                        }
                    );
                    for col in 0..area.width {
                        let cell = &buffer[(col, row)];
                        assert_eq!(cell.bg, Color::Reset);
                        assert!(!cell.modifier.contains(Modifier::REVERSED));
                        assert!(!matches!(cell.fg, Color::Rgb(..) | Color::Indexed(..)));
                    }
                }
                assert!(buffer[(2, area.y)].modifier.contains(Modifier::BOLD));
                assert_eq!(buffer[(2, area.y + 1)].fg, Color::Reset);
                let counts: String = (2..area.width)
                    .map(|x| buffer[(x, area.y + 2)].symbol())
                    .collect();
                let expected = if starred == Some(true) {
                    "* 1 stars  0 replies"
                } else {
                    "1 stars  0 replies"
                };
                assert!(counts.starts_with(expected));
                let stars_end = expected.find("  ").unwrap();
                for x in 2..2 + stars_end as u16 {
                    assert_eq!(
                        buffer[(x, area.y + 2)].fg,
                        if starred == Some(true) {
                            Color::Yellow
                        } else {
                            Color::Reset
                        }
                    );
                }
                for x in 2 + stars_end as u16..2 + expected.len() as u16 {
                    assert_eq!(buffer[(x, area.y + 2)].fg, Color::Reset);
                }
                // A body that fits moves immediately, independently of prior rendering.
                app.focus = Focus::List;
                app.input(UiInput::Key(Key::Down), now);
                assert_eq!(app.selection().id(), Some(id(2)));
            }
        }
    }
}

mod floating;
mod scrolling;
