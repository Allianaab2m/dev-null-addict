use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_millis(100);

// A separate, non-blocking open file description of the terminal, so that
// stdin's blocking reader is left unchanged.
const NONBLOCK: i32 = libc::O_NONBLOCK;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct StartupInput {
    pub position: Option<(u16, u16)>,
    pub held: Vec<u8>,
}

pub(super) fn cursor(display: &mut super::Display) -> io::Result<StartupInput> {
    let mut tty = OpenOptions::new()
        .read(true)
        .custom_flags(NONBLOCK)
        .open("/dev/tty")?;
    display.write(b"\x1b[6n")?;
    collect(&mut tty, Instant::now() + TIMEOUT)
}

fn collect(reader: &mut impl Read, deadline: Instant) -> io::Result<StartupInput> {
    let mut held = Vec::new();
    let mut chunk = [0; 1024];
    while Instant::now() < deadline && held.len() < 65536 {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                held.extend_from_slice(&chunk[..n]);
                if let Some((start, end, position)) = find_cpr(&held) {
                    held.drain(start..end);
                    return Ok(StartupInput {
                        position: Some(position),
                        held,
                    });
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(StartupInput {
        position: None,
        held,
    })
}

fn find_cpr(bytes: &[u8]) -> Option<(usize, usize, (u16, u16))> {
    bytes
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| *pair == b"\x1b[")
        .find_map(|(start, _)| {
            let tail = &bytes[start + 2..];
            let end = tail
                .iter()
                .position(|byte| !byte.is_ascii_digit() && *byte != b';')?;
            if tail[end] != b'R' {
                return None;
            }
            let text = std::str::from_utf8(&tail[..end]).ok()?;
            let (row, col) = text.split_once(';')?;
            let row: u16 = row.parse().ok()?;
            let col: u16 = col.parse().ok()?;
            (row > 0 && col > 0).then(|| (start, start + end + 3, (row - 1, col - 1)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Startup consumes only a complete CPR, keeping input around every fragmented reply in order.
    #[test]
    fn fragmented_cpr_preserves_typeahead() {
        struct Chunks(std::collections::VecDeque<Vec<u8>>);
        impl Read for Chunks {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let Some(bytes) = self.0.pop_front() else {
                    return Ok(0);
                };
                out[..bytes.len()].copy_from_slice(&bytes);
                Ok(bytes.len())
            }
        }
        let reply = b"\x1b[10;7R";
        for split in 1..reply.len() {
            let mut reader = Chunks(
                [
                    [b"before\x1b[0;7R\x1b[999999;7R".as_slice(), &reply[..split]].concat(),
                    [&reply[split..], b"after"].concat(),
                ]
                .into(),
            );
            let StartupInput { position, held } =
                collect(&mut reader, Instant::now() + TIMEOUT).unwrap();
            assert_eq!(position, Some((9, 6)));
            assert_eq!(held, b"before\x1b[0;7R\x1b[999999;7Rafter");
        }
    }

    // A terminal that does not answer must not block startup or lose an incomplete input sequence.
    #[test]
    fn unanswered_cpr_times_out() {
        struct Silent;
        impl Read for Silent {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::WouldBlock.into())
            }
        }
        let start = Instant::now();
        assert_eq!(
            collect(&mut Silent, start + TIMEOUT).unwrap(),
            StartupInput {
                position: None,
                held: vec![]
            }
        );
        assert!(start.elapsed() >= TIMEOUT);
        assert!(start.elapsed() < Duration::from_secs(1));
        let mut incomplete = &b"typed\x1b[10;"[..];
        assert_eq!(
            collect(&mut incomplete, Instant::now() + TIMEOUT).unwrap(),
            StartupInput {
                position: None,
                held: b"typed\x1b[10;".to_vec()
            }
        );
    }
}
