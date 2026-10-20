use crate::api::CommitEntry;
use crate::terminal::TermSize;

use super::{App, Effect, Request, layout, render, selection::Selection};

impl App {
    pub(crate) fn set_layout(&mut self, layout: layout::OverlayLayout) {
        self.layout = layout;
        self.sync_viewport();
    }

    pub(crate) fn floating(&self) -> bool {
        layout::window(self.layout, self.size).floating
    }

    pub(crate) fn set_background(&mut self, rows: Vec<String>) {
        self.background = rows;
    }

    pub(super) fn viewport(&self) -> &super::viewport::Viewport {
        self.threads.last().map_or_else(
            || &self.timeline.tab(self.timeline.active()).viewport,
            |view| &view.viewport,
        )
    }

    pub(super) fn sync_viewport(&mut self) {
        let area = layout::areas(self, self.size).list;
        if area.is_empty() {
            return;
        }
        let entries = self.entry_heights(area.width);
        let selected = self.selection().id();
        let viewport = if let Some(view) = self.threads.last_mut() {
            &mut view.viewport
        } else {
            &mut self.timeline.tab_mut(self.timeline.active()).viewport
        };
        viewport.reconcile(&entries, selected, area.height);
    }

    fn entry_heights(&self, width: u16) -> Vec<(crate::api::CommitId, usize)> {
        let full = !self.threads.is_empty();
        self.list_entries()
            .into_iter()
            .map(|entry| (entry.id(), render::entry_height(entry, width, full)))
            .collect()
    }

    pub(crate) fn resize(&mut self, size: TermSize) {
        self.size = ratatui::layout::Rect::new(0, 0, size.cols, size.rows);
        self.sync_viewport();
    }

    pub(super) fn list_entries(&self) -> Vec<&CommitEntry> {
        self.threads.last().map_or_else(
            || {
                self.timeline
                    .tab(self.timeline.active())
                    .list
                    .entries()
                    .iter()
                    .collect()
            },
            |view| view.entries().collect(),
        )
    }

    pub(super) fn selection(&self) -> &Selection {
        self.threads.last().map_or_else(
            || &self.timeline.tab(self.timeline.active()).selection,
            |view| view.selection(),
        )
    }

    pub(super) fn move_selection(&mut self, down: bool) -> Vec<Effect> {
        let area = layout::areas(self, self.size).list;
        if area.is_empty() {
            return Vec::new();
        }
        let entries = self.entry_heights(area.width);
        let last = entries.last().map(|(id, _)| *id);
        let request = if let Some(view) = self.threads.last_mut() {
            let (selection, viewport) = view.navigation();
            let navigate = viewport.move_line(selection, &entries, area.height, down);
            if navigate && down && view.selection().id() == last {
                view.replies_mut()
                    .begin_more()
                    .map(|cursor| Request::Replies {
                        id: view.id(),
                        cursor: Some(cursor),
                    })
            } else {
                None
            }
        } else {
            let kind = self.timeline.active();
            let tab = self.timeline.tab_mut(kind);
            let navigate = tab
                .viewport
                .move_line(&mut tab.selection, &entries, area.height, down);
            if navigate && down && tab.selection.id() == last {
                tab.list.begin_more().map(|cursor| Request::Log {
                    kind,
                    cursor: Some(cursor),
                })
            } else {
                None
            }
        };
        self.queue(request.into_iter().collect())
    }
}
