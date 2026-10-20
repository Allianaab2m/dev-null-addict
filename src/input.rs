//! Keyboard input from the outer terminal.
//!
//! While the overlay is closed, [`ToggleScanner`] and [`ToggleGate`] pass the
//! input to the shell and only take out the toggle key. While it is open,
//! [`KeyDecoder`] turns the input into keys for the overlay.

mod decoder;
mod gate;
mod key;
mod protocol;
mod scanner;
mod toggle_key;

pub use decoder::{Decoded, KeyDecoder};
pub use gate::{GateAction, ToggleGate};
pub use key::{Key, UiInput};
pub use scanner::{Scanned, ToggleEvent, ToggleScanner};
pub use toggle_key::{ToggleKey, ToggleKeyError};
