use std::fmt;
use std::str::FromStr;

/// The key that opens and closes the overlay: Ctrl and a letter, written
/// `ctrl+g`.
///
/// Ctrl+H, Ctrl+I and Ctrl+M are refused. Without a keyboard protocol,
/// terminals send them as Backspace (on some terminals), Tab and Enter, so
/// pressing those keys would toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ToggleKey(u8);

impl ToggleKey {
    /// The letter, lowercase.
    pub fn letter(self) -> char {
        self.0 as char
    }

    /// The byte a terminal sends for it without a keyboard protocol: `0x07`
    /// for Ctrl+G.
    pub fn control_byte(self) -> u8 {
        self.0 - b'a' + 1
    }
}

/// Parses `ctrl+<letter>`, ignoring ASCII case.
impl FromStr for ToggleKey {
    type Err = ToggleKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_ascii_lowercase();
        let bytes = lower
            .strip_prefix("ctrl+")
            .ok_or(ToggleKeyError::Syntax)?
            .as_bytes();
        let [letter @ b'a'..=b'z'] = bytes else {
            return Err(ToggleKeyError::Syntax);
        };
        let sent_as = match letter {
            b'h' => Some("Backspace"),
            b'i' => Some("Tab"),
            b'm' => Some("Enter"),
            _ => None,
        };
        if let Some(sent_as) = sent_as {
            return Err(ToggleKeyError::Reserved {
                letter: *letter as char,
                sent_as,
            });
        }
        Ok(Self(*letter))
    }
}

/// Writes `ctrl+<letter>`.
impl fmt::Display for ToggleKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ctrl+{}", self.letter())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
pub enum ToggleKeyError {
    #[error("expected ctrl and a letter from a to z, such as ctrl+g")]
    Syntax,
    #[error("ctrl+{letter} can't be used: terminals send it as {sent_as}")]
    Reserved { letter: char, sent_as: &'static str },
}

#[cfg(test)]
mod tests {
    use super::*;

    // Legacy control bytes collide with three ordinary keys, which must never toggle.
    #[test]
    fn parses_letters_and_rejects_ambiguous_or_malformed_keys() {
        for letter in b'a'..=b'z' {
            let parsed = format!("CTRL+{}", letter as char).parse::<ToggleKey>();
            if matches!(letter, b'h' | b'i' | b'm') {
                assert!(matches!(parsed, Err(ToggleKeyError::Reserved { .. })));
            } else {
                let key = parsed.unwrap();
                assert_eq!(key.control_byte(), letter - b'a' + 1);
                assert_eq!(key.to_string(), format!("ctrl+{}", letter as char));
            }
        }
        for bad in ["", "ctrl+gg", "ctrl+1", "alt+g", " ctrl+g", "ctrl+é"] {
            assert_eq!(bad.parse::<ToggleKey>(), Err(ToggleKeyError::Syntax));
        }
    }
}
