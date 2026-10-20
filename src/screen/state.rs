/// The screen buffer a terminal shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Buffer {
    #[default]
    Main,
    Alternate,
}

/// Terminal state the program in the shell set that `vt100` does not expose:
/// kitty keyboard flags, modifyOtherKeys, scrolling margins, origin mode,
/// autowrap, cursor style and focus reporting.
///
/// Callbacks track unhandled sequences; a supplemental parser observes handled
/// sequences whose state is not exposed by vt100.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExtraModes {
    /// The kitty keyboard flags stacks, one for each buffer as kitty keeps
    /// them, current flags last.
    keyboard_flags: [Vec<u16>; 2],
    modify_other_keys: u8,
    /// Top and bottom rows of the scrolling region, zero-based and
    /// inclusive. `None` for the whole screen.
    margins: Option<(u16, u16)>,
    origin_mode: bool,
    autowrap: bool,
    cursor_style: u8,
    focus_reporting: bool,
}

impl Default for ExtraModes {
    /// As a terminal starts: autowrap on, everything else off.
    fn default() -> Self {
        Self {
            keyboard_flags: [vec![0], vec![0]],
            modify_other_keys: 0,
            margins: None,
            origin_mode: false,
            autowrap: true,
            cursor_style: 0,
            focus_reporting: false,
        }
    }
}

impl ExtraModes {
    /// Updates the tracked state from a CSI sequence.
    pub fn csi(&mut self, buffer: Buffer, intermediates: &[u8], params: &[&[u16]], action: char) {
        let param = |i: usize, default| {
            params
                .get(i)
                .and_then(|p| p.first())
                .copied()
                .unwrap_or(default)
        };
        let stack = &mut self.keyboard_flags[usize::from(buffer == Buffer::Alternate)];
        match (intermediates, action) {
            (b">", 'u') => {
                stack.push(param(0, 0));
            }
            (b"<", 'u') => {
                let len = stack
                    .len()
                    .saturating_sub(usize::from(param(0, 1).max(1)))
                    .max(1);
                stack.truncate(len);
            }
            (b"=", 'u') => {
                let flags = param(0, 0);
                let current = stack.last_mut().expect("base flags");
                *current = match param(1, 1) {
                    2 => *current | flags,
                    3 => *current & !flags,
                    _ => flags,
                };
            }
            (b">", 'm') if param(0, 0) == 4 => self.modify_other_keys = param(1, 0).min(2) as u8,
            (b"", 'r') => {
                let top = param(0, 1).max(1);
                let bottom = param(1, 0);
                self.margins = (bottom > top).then_some((top - 1, bottom.saturating_sub(1)));
            }
            (b"?", 'h' | 'l') => {
                for p in params {
                    match p.first() {
                        Some(6) => self.origin_mode = action == 'h',
                        Some(7) => self.autowrap = action == 'h',
                        Some(1004) => self.focus_reporting = action == 'h',
                        _ => {}
                    }
                }
            }
            (b" ", 'q') => self.cursor_style = param(0, 0).min(6) as u8,
            _ => {}
        }
    }

    /// The current kitty keyboard flags on `buffer`.
    pub fn keyboard_flags(&self, buffer: Buffer) -> u16 {
        *self.keyboard_flags[usize::from(buffer == Buffer::Alternate)]
            .last()
            .expect("base flags")
    }

    /// Bytes that set these modes as tracked, for `buffer`.
    pub fn restore(&self, buffer: Buffer) -> Vec<u8> {
        let stack = &self.keyboard_flags[usize::from(buffer == Buffer::Alternate)];
        let mut out = format!("\x1b[<65535u\x1b[={}u", stack[0]);
        for flags in &stack[1..] {
            out.push_str(&format!("\x1b[>{flags}u"));
        }
        out.push_str(&format!("\x1b[>4;{}m", self.modify_other_keys));
        if let Some((top, bottom)) = self.margins {
            out.push_str(&format!("\x1b[{};{}r", top + 1, bottom + 1));
        } else {
            out.push_str("\x1b[r");
        }
        for (mode, enabled) in [
            (6, self.origin_mode),
            (7, self.autowrap),
            (1004, self.focus_reporting),
        ] {
            out.push_str(&format!("\x1b[?{mode}{}", if enabled { 'h' } else { 'l' }));
        }
        out.push_str(&format!("\x1b[{} q", self.cursor_style));
        out.into_bytes()
    }
}

impl ExtraModes {
    pub(super) fn mode(&self, mode: u16) -> Option<bool> {
        match mode {
            6 => Some(self.origin_mode),
            7 => Some(self.autowrap),
            1004 => Some(self.focus_reporting),
            _ => None,
        }
    }
    pub(super) fn cursor_row(&self, row: u16) -> u16 {
        if self.origin_mode {
            row.saturating_sub(self.margins.map_or(0, |(top, _)| top))
        } else {
            row
        }
    }
    pub(super) fn resize(&mut self) {
        self.margins = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn csi(m: &mut ExtraModes, buffer: Buffer, i: &[u8], p: &[u16], a: char) {
        let params: Vec<_> = p.iter().map(std::slice::from_ref).collect();
        m.csi(buffer, i, &params, a);
    }

    // Kitty stacks belong to each buffer and all three assignment operations must survive restoration.
    #[test]
    fn keyboard_stack_and_restore() {
        let mut modes = ExtraModes::default();
        csi(&mut modes, Buffer::Main, b"=", &[1], 'u');
        csi(&mut modes, Buffer::Main, b">", &[3], 'u');
        csi(&mut modes, Buffer::Main, b"=", &[4, 2], 'u');
        assert_eq!(modes.keyboard_flags(Buffer::Main), 7);
        csi(&mut modes, Buffer::Main, b"=", &[2, 3], 'u');
        assert_eq!(modes.keyboard_flags(Buffer::Main), 5);
        csi(&mut modes, Buffer::Alternate, b">", &[9], 'u');
        assert_eq!(modes.keyboard_flags(Buffer::Alternate), 9);
        let restore = String::from_utf8(modes.restore(Buffer::Main)).unwrap();
        assert!(restore.starts_with("\x1b[<65535u\x1b[=1u\x1b[>5u"));
        csi(&mut modes, Buffer::Main, b"<", &[1], 'u');
        assert_eq!(modes.keyboard_flags(Buffer::Main), 1);
        csi(&mut modes, Buffer::Main, b"<", &[999], 'u');
        assert_eq!(modes.keyboard_flags(Buffer::Main), 1);
        assert_eq!(modes.keyboard_flags(Buffer::Alternate), 9);
    }

    // Modes that the overlay resets must be explicitly restored, including disabled autowrap.
    #[test]
    fn extra_modes_restore() {
        let mut modes = ExtraModes::default();
        csi(&mut modes, Buffer::Main, b">", &[4, 2], 'm');
        csi(&mut modes, Buffer::Main, b"", &[2, 20], 'r');
        csi(&mut modes, Buffer::Main, b"?", &[6, 1004], 'h');
        csi(&mut modes, Buffer::Main, b"?", &[7], 'l');
        csi(&mut modes, Buffer::Main, b" ", &[5], 'q');
        let restore = String::from_utf8(modes.restore(Buffer::Main)).unwrap();
        for sequence in [
            "\x1b[>4;2m",
            "\x1b[2;20r",
            "\x1b[?6h",
            "\x1b[?7l",
            "\x1b[?1004h",
            "\x1b[5 q",
        ] {
            assert!(restore.contains(sequence), "{sequence:?}");
        }
        assert_eq!(modes.cursor_row(3), 2);
    }
}
