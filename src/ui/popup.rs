use std::time::{Duration, Instant};

use crate::api::{CommitId, ShortHash};

/// A box over the overlay. Esc closes it first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Popup {
    /// A message, such as an error or a rejected commit, that goes away by
    /// itself at `until`.
    Toast { message: String, until: Instant },
    /// Asks before deleting a commit of the user. Enter deletes, Esc keeps it.
    ConfirmDelete { id: CommitId, short_hash: ShortHash },
}

impl Popup {
    /// How long a toast stays.
    pub const TOAST_TIME: Duration = Duration::from_secs(5);

    pub fn toast(message: impl Into<String>, now: Instant) -> Self {
        Self::Toast {
            message: message.into(),
            until: now + Self::TOAST_TIME,
        }
    }
}

/// A message in place of the list, until what it says is fixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Notice {
    /// There is no device key. Run `dev-null-addict login`.
    SignedOut,
    /// The key is not, or no longer, an approved device. Run
    /// `dev-null-addict login` again.
    NotApproved,
}
