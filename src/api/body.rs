use serde::{Deserialize, Serialize};

use super::CommitId;

/// The text of a new commit, as /dev/null will accept it.
///
/// /dev/null normalizes a body before it measures it
/// (`App\Services\CommitBodies::normalize`, then Laravel's `TrimStrings`):
/// line endings become `\n`, outside code three or more line breaks in a row
/// (with only spaces and tabs between them) become two, and leading and
/// trailing whitespace goes. The length is then counted in characters
/// (Unicode scalar values). This does the same, so a body that passes here
/// passes there.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CommitBody(String);

impl CommitBody {
    pub const MIN_CHARS: usize = 3;
    pub const MAX_CHARS: usize = 4000;

    /// Normalizes `text` and checks its length.
    pub fn new(text: &str) -> Result<Self, CommitBodyError> {
        let normalized = normalize(text);
        let count = normalized.chars().count();
        match count {
            0..Self::MIN_CHARS => Err(CommitBodyError::TooShort(count)),
            n if n > Self::MAX_CHARS => Err(CommitBodyError::TooLong(count)),
            _ => Ok(Self(normalized)),
        }
    }

    /// The length /dev/null will measure for `text`, in characters.
    pub fn measure(text: &str) -> usize {
        normalize(text).chars().count()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CommitBody {
    type Error = CommitBodyError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(&text)
    }
}

impl From<CommitBody> for String {
    fn from(body: CommitBody) -> Self {
        body.0
    }
}

/// The length of a body that /dev/null would refuse, in characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
pub enum CommitBodyError {
    #[error("a commit needs at least {min} characters, this has {0}", min = CommitBody::MIN_CHARS)]
    TooShort(usize),
    #[error("a commit can have at most {max} characters, this has {0}", max = CommitBody::MAX_CHARS)]
    TooLong(usize),
}

/// What a new commit answers. A commit replies to one or quotes one, not both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommitTarget {
    #[serde(rename = "parent_id")]
    Reply(CommitId),
    #[serde(rename = "quoted_commit_id")]
    Quote(CommitId),
}

/// The body of `POST /api/v1/commits`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NewCommit {
    pub body: CommitBody,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub target: Option<CommitTarget>,
}

fn normalize(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut normalized = String::new();
    let mut prose = 0;
    let mut pos = 0;
    while pos < text.len() {
        if text.as_bytes()[pos] == b'`'
            && let Some(end) = code_end(&text, pos)
        {
            normalize_prose(&text[prose..pos], &mut normalized);
            normalized.push_str(&text[pos..end]);
            pos = end;
            prose = end;
        } else {
            pos += 1;
        }
    }
    normalize_prose(&text[prose..], &mut normalized);
    // Laravel's mb_trim also removes NUL and the zero-width no-break space.
    normalized
        .trim_matches(|c: char| c.is_whitespace() || c == '\0' || c == '\u{feff}')
        .to_owned()
}

fn code_end(text: &str, start: usize) -> Option<usize> {
    let tail = &text[start..];
    if (start == 0 || text.as_bytes()[start - 1] == b'\n') && tail.starts_with("```") {
        let ticks = tail.bytes().take_while(|b| *b == b'`').count();
        if let Some(newline) = tail.find('\n')
            && !tail[ticks..newline].contains('`')
        {
            let mut offset = start + newline + 1;
            for line in text[offset..].split_inclusive('\n') {
                let content = line.trim_end_matches('\n');
                let fence = content.bytes().take_while(|b| *b == b'`').count();
                if fence >= 3 && content[fence..].bytes().all(|b| b == b' ' || b == b'\t') {
                    return Some(offset + content.len());
                }
                offset += line.len();
            }
        }
    }
    let end = tail[1..].find(['`', '\n'])? + 1;
    (end > 1 && tail.as_bytes()[end] == b'`').then_some(start + end + 1)
}

fn normalize_prose(text: &str, out: &mut String) {
    let bytes = text.as_bytes();
    let mut pos = 0;
    let mut copied = 0;
    while pos < bytes.len() {
        if matches!(bytes[pos], b' ' | b'\t' | b'\n') {
            let start = pos;
            let mut breaks = 0;
            let mut end = start;
            while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t' | b'\n') {
                if bytes[pos] == b'\n' {
                    breaks += 1;
                    end = pos + 1;
                }
                pos += 1;
            }
            if breaks >= 3 {
                out.push_str(&text[copied..start]);
                out.push_str("\n\n");
                copied = end;
            }
        } else {
            pos += 1;
        }
    }
    out.push_str(&text[copied..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    // These cases mirror CommitBodiesTest.php and CodeSpans::PATTERN, with Laravel's final trimming.
    #[test]
    fn upstream_normalize_and_measure() {
        for (raw, expected) in [
            ("a\n\nb", "a\n\nb"),
            ("a\n\n\nb", "a\n\nb"),
            ("a\n\n\n\n\n\nb", "a\n\nb"),
            ("a \n\t\n  \n\nb", "a\n\nb"),
            ("a\r\nb\r\n\r\n\r\n\r\nc", "a\nb\n\nc"),
            ("a\rb\r\r\rc", "a\nb\n\nc"),
            ("a\r\nb\rc\n\n\rd", "a\nb\nc\n\nd"),
            (
                "a\n```py\ndef a():\n    pass\n\n\ndef b():\n    pass\n```\nb",
                "a\n```py\ndef a():\n    pass\n\n\ndef b():\n    pass\n```\nb",
            ),
            (
                "a\n\n\n\n```\ncode\n```\n\n\n\nb",
                "a\n\n```\ncode\n```\n\nb",
            ),
            ("````rs\nx\n\n\ny\n```", "````rs\nx\n\n\ny\n```"),
            ("```\nx\n\n\ny", "```\nx\n\ny"),
            ("see ```\nx\n\n\ny\n```", "see ```\nx\n\ny\n```"),
            (
                "```\nx\n``` nope\n\n\ny\n```",
                "```\nx\n``` nope\n\n\ny\n```",
            ),
            ("  界aé\r\n\n\n  z  ", "界aé\n\n  z"),
            ("`inline`\n\n\nend", "`inline`\n\nend"),
        ] {
            assert_eq!(normalize(raw), expected, "{raw:?}");
            assert_eq!(CommitBody::measure(raw), expected.chars().count());
            assert_eq!(CommitBody::new(raw).unwrap().as_str(), expected);
        }
        assert_eq!(CommitBody::new(" \n a "), Err(CommitBodyError::TooShort(1)));
        assert_eq!(
            CommitBody::new(&"界".repeat(4001)),
            Err(CommitBodyError::TooLong(4001))
        );
        assert!(CommitBody::new(&"界".repeat(4000)).is_ok());
    }

    // Targets are mutually exclusive flat fields in the upstream create payload.
    #[test]
    fn new_commit_json_is_flat() {
        let id = crate::test_support::id(1);
        for (target, field) in [
            (CommitTarget::Reply(id), "parent_id"),
            (CommitTarget::Quote(id), "quoted_commit_id"),
        ] {
            let value = serde_json::to_value(NewCommit {
                body: CommitBody::new("hello").unwrap(),
                target: Some(target),
            })
            .unwrap();
            assert_eq!(value[field], id.to_string());
            assert!(value.get("target").is_none());
        }
    }
}
