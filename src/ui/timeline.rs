use crate::api::LogKind;

use super::list::CommitList;
use super::selection::Selection;

/// The three logs, one tab each. h and l switch tabs and stop at either end.
#[derive(Debug)]
pub struct Timeline {
    active: LogKind,
    following: LogTab,
    latest: LogTab,
    trending: LogTab,
}

/// A log's commits and its own selection.
#[derive(Debug)]
pub struct LogTab {
    pub list: CommitList,
    pub selection: Selection,
    pub(super) viewport: super::viewport::Viewport,
}

impl Timeline {
    pub fn new(active: LogKind) -> Self {
        let tab = |order| LogTab {
            list: CommitList::new(order),
            selection: Selection::default(),
            viewport: super::viewport::Viewport::default(),
        };
        Self {
            active,
            following: tab(super::list::Order::NewestFirst),
            latest: tab(super::list::Order::NewestFirst),
            trending: tab(super::list::Order::Ranked),
        }
    }

    pub fn active(&self) -> LogKind {
        self.active
    }

    /// Switches to `kind`. Returns whether the tab changed.
    pub fn switch(&mut self, kind: LogKind) -> bool {
        let changed = self.active != kind;
        self.active = kind;
        changed
    }

    pub fn tab(&self, kind: LogKind) -> &LogTab {
        match kind {
            LogKind::Following => &self.following,
            LogKind::Latest => &self.latest,
            LogKind::Trending => &self.trending,
        }
    }

    pub fn tab_mut(&mut self, kind: LogKind) -> &mut LogTab {
        match kind {
            LogKind::Following => &mut self.following,
            LogKind::Latest => &mut self.latest,
            LogKind::Trending => &mut self.trending,
        }
    }
}
