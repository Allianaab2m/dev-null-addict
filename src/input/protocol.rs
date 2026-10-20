use super::{ToggleEvent, ToggleKey};

pub(super) fn sequence_len(bytes: &[u8]) -> Option<usize> {
    let first = *bytes.first()?;
    if first != 0x1b {
        let len = match first {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 1,
        };
        return (bytes.len() >= len).then_some(len);
    }
    match *bytes.get(1)? {
        b'[' => bytes[2..]
            .iter()
            .position(|b| (0x40..=0x7e).contains(b))
            .map(|n| n + 3),
        b'O' => (bytes.len() >= 3).then_some(3),
        b']' | b'P' | b'_' | b'^' => bytes[2..]
            .iter()
            .enumerate()
            .find_map(|(n, b)| (*b == 7 || (*b == b'\\' && bytes[n + 1] == 0x1b)).then_some(n + 3)),
        _ => sequence_len(&bytes[1..]).map(|n| n + 1),
    }
}

pub(super) fn enhanced(bytes: &[u8]) -> Option<(u32, u32, ToggleEvent)> {
    let text = std::str::from_utf8(bytes).ok()?.strip_prefix("\x1b[")?;
    if let Some(params) = text.strip_suffix('u') {
        let mut parts = params.split(';');
        let code = parts.next()?.split(':').next()?.parse().ok()?;
        let mut modifiers = parts.next().unwrap_or("1").split(':');
        let mods: u32 = modifiers.next()?.parse().ok()?;
        let event = match modifiers.next().unwrap_or("1") {
            "1" => ToggleEvent::Press,
            "2" => ToggleEvent::Repeat,
            "3" => ToggleEvent::Release,
            _ => return None,
        };
        Some((code, mods.checked_sub(1)? & !192, event))
    } else {
        let params = text.strip_suffix('~')?;
        let mut parts = params.split(';');
        if parts.next()? != "27" {
            return None;
        }
        let mods: u32 = parts.next()?.parse().ok()?;
        let code = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some((code, mods.checked_sub(1)?, ToggleEvent::Press))
    }
}

pub(super) fn toggle_event(bytes: &[u8], key: ToggleKey) -> Option<ToggleEvent> {
    if bytes == [key.control_byte()] {
        return Some(ToggleEvent::Press);
    }
    let (code, mods, event) = enhanced(bytes)?;
    (code == key.letter() as u32 && mods == 4).then_some(event)
}

pub(super) fn is_reply(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"\x1b]") || bytes.starts_with(b"\x1bP") {
        return true;
    }
    let Some(body) = bytes.strip_prefix(b"\x1b[") else {
        return false;
    };
    matches!(body.last(), Some(b'R' | b'c' | b'n'))
        || (body.starts_with(b"?") && body.ends_with(b"u"))
        || body.ends_with(b"$y")
}
