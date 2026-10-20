use crate::api::{CommitEntry, CommitId, Page, Thread};

use super::list::CommitList;
use super::selection::Selection;

/// A commit opened with Enter: the commits it replies to, the commit, and its
/// replies oldest first, every one in full.
///
/// The selection runs over all of them and starts on the commit itself.
/// Enter on another commit in it opens that one's thread on top.
#[derive(Debug)]
pub struct ThreadView {
    id: CommitId,
    /// The ancestors, oldest first, then the commit. Empty until the thread
    /// arrives.
    head: Vec<CommitEntry>,
    replies: CommitList,
    selection: Selection,
    pub(super) viewport: super::viewport::Viewport,
}

impl ThreadView {
    /// A thread for `id`, empty until [`set_thread`](Self::set_thread).
    pub fn new(id: CommitId) -> Self {
        Self {
            id,
            head: Vec::new(),
            replies: CommitList::new(super::list::Order::OldestFirst),
            selection: Selection::default(),
            viewport: super::viewport::Viewport::default(),
        }
    }

    pub fn id(&self) -> CommitId {
        self.id
    }

    pub fn is_loaded(&self) -> bool {
        !self.head.is_empty()
    }

    /// Puts in the commit and its ancestors, keeping the replies.
    pub fn set_thread(&mut self, thread: Thread) {
        let was_loaded = self.is_loaded();
        self.head = thread
            .ancestors
            .into_iter()
            .filter(|e| match e {
                CommitEntry::Live(c) => c.is_approved(),
                _ => true,
            })
            .collect();
        if thread.commit.is_approved() {
            self.head.push(CommitEntry::Live(Box::new(thread.commit)));
        }
        self.reanchor();
        if !was_loaded {
            let ids = self.ids();
            let id = self.id;
            self.selection_mut().select(&ids, id);
        }
    }

    pub fn replies(&self) -> &CommitList {
        &self.replies
    }

    /// For refreshing and paging the replies. Call
    /// [`reanchor`](Self::reanchor) after changing them.
    pub fn replies_mut(&mut self) -> &mut CommitList {
        &mut self.replies
    }

    /// The ancestors, the commit and the replies, in display order.
    pub fn entries(&self) -> impl Iterator<Item = &CommitEntry> {
        self.head.iter().chain(self.replies.entries())
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn selection_mut(&mut self) -> &mut Selection {
        &mut self.selection
    }

    /// The ids of [`entries`](Self::entries), for the selection.
    pub fn ids(&self) -> Vec<CommitId> {
        self.entries().map(CommitEntry::id).collect()
    }

    pub fn reanchor(&mut self) {
        self.selection.reanchor(&self.ids());
    }

    /// Puts in a fresh first page of replies and keeps the selection.
    pub fn refresh_replies(&mut self, page: Page<CommitEntry>) {
        self.replies.refresh(page);
        self.reanchor();
    }
}

impl ThreadView {
    pub(super) fn navigation(&mut self) -> (&mut Selection, &mut super::viewport::Viewport) {
        (&mut self.selection, &mut self.viewport)
    }

    pub(super) fn update(&mut self, entry: CommitEntry) {
        if let Some(existing) = self.head.iter_mut().find(|e| e.id() == entry.id()) {
            *existing = entry.clone();
        }
        self.replies.update(entry);
        self.reanchor();
    }
    pub(super) fn remove(&mut self, id: CommitId) {
        self.head.retain(|e| e.id() != id);
        self.replies.remove(id);
        self.reanchor();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{commit, id, page};

    // Ancestors must survive replies refreshes and selection initially belongs to the thread root.
    #[test]
    fn thread_keeps_head_and_reply_selection() {
        let mut view = ThreadView::new(id(2));
        view.set_thread(Thread {
            ancestors: page(&[1], None).items,
            commit: commit(2),
        });
        view.refresh_replies(page(&[3, 4], None));
        assert_eq!(view.selection().id(), Some(id(2)));
        view.selection_mut()
            .select(&[id(1), id(2), id(3), id(4)], id(4));
        view.refresh_replies(page(&[3, 4, 5], None));
        assert_eq!(view.ids(), vec![id(1), id(2), id(3), id(4), id(5)]);
        assert_eq!(view.selection().id(), Some(id(4)));
        view.set_thread(Thread {
            ancestors: page(&[1], None).items,
            commit: commit(2),
        });
        assert_eq!(view.selection().id(), Some(id(4)));
    }
}
