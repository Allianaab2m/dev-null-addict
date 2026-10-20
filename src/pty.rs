//! The shell running on a pseudo-terminal.

use std::io;
use std::time::Duration;

use pty_process::{OwnedReadPty, OwnedWritePty};
use tokio::process::Child;

use crate::shell::{ShellCommand, ShellExit};
use crate::terminal::TermSize;

/// The shell on its own PTY, split so that reading, writing and waiting can
/// run side by side.
#[derive(Debug)]
pub struct ShellProcess {
    /// What the shell writes to its terminal. Reading reports EOF, or `EIO` on
    /// Linux, once the shell side is closed, which can be after the shell
    /// exits if a background job still holds the PTY.
    pub output: OwnedReadPty,
    /// What the shell reads from its terminal. Also resizes the PTY.
    pub input: OwnedWritePty,
    /// Spawned with `kill_on_drop`, so that an unwinding panic never leaves
    /// the shell running.
    pub child: Child,
}

impl ShellProcess {
    /// How long [`hang_up`] waits for the shell to exit after the hangup
    /// before it kills it.
    pub const HANGUP_GRACE: Duration = Duration::from_secs(1);

    /// Starts `command` as the session leader of a new PTY of `size`, with
    /// [`DISABLE_ENV`](crate::DISABLE_ENV) set.
    pub fn spawn(command: &ShellCommand, size: TermSize) -> io::Result<Self> {
        let (pty, pts) = pty_process::open().map_err(io::Error::other)?;
        pty.resize(size.into()).map_err(io::Error::other)?;
        let child = pty_process::Command::new(command.program())
            .args(command.args())
            .env(crate::DISABLE_ENV, "1")
            .kill_on_drop(true)
            .spawn(pts)
            .map_err(io::Error::other)?;
        let (output, input) = pty.into_split();
        Ok(Self {
            output,
            input,
            child,
        })
    }
}

/// Ends the shell: closes both halves of the PTY, which hangs it up
/// (`SIGHUP`) as closing a terminal window does, waits up to
/// [`ShellProcess::HANGUP_GRACE`], kills it if it is still there, and reaps
/// it.
pub async fn hang_up(
    output: OwnedReadPty,
    input: OwnedWritePty,
    mut child: Child,
) -> io::Result<ShellExit> {
    drop(output);
    drop(input);
    match tokio::time::timeout(ShellProcess::HANGUP_GRACE, child.wait()).await {
        Ok(status) => status.map(Into::into),
        Err(_) => {
            child.start_kill()?;
            child.wait().await.map(Into::into)
        }
    }
}

impl From<TermSize> for pty_process::Size {
    fn from(size: TermSize) -> Self {
        Self::new(size.rows, size.cols)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{GateAction, ToggleGate, ToggleScanner};
    use crate::screen::{Frame, OutputFramer, ScreenModel, Vt100Screen};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // This uses a real PTY without a TTY on test stdin and exercises input, framing, screen state and exit status.
    #[tokio::test(flavor = "current_thread")]
    async fn real_pty_passthrough_and_exit_code() {
        let size = TermSize { cols: 80, rows: 24 };
        let command = ShellCommand::new(
            "/bin/sh",
            [
                "-c",
                "read line; printf 'received:%s env:%s\\n' \"$line\" \"$NO_DEV_NULL_ADDICT\"; exit 37",
            ],
        );
        let mut shell = ShellProcess::spawn(&command, size).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(5), async {
            let mut scanner = ToggleScanner::new("ctrl+g".parse().unwrap());
            let mut gate = ToggleGate::new();
            let mut pieces = Vec::new();
            let now = std::time::Instant::now();
            scanner.push(b"hello\n", now, &mut pieces);
            for piece in pieces {
                if let GateAction::Forward(bytes) = gate.handle(piece, now) {
                    shell.input.write_all(&bytes).await.unwrap();
                }
            }
            let mut framer = OutputFramer::new();
            let mut screen = Vt100Screen::new(size);
            let mut output = Vec::new();
            let mut buf = [0; 1024];
            loop {
                match shell.output.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut frames = Vec::new();
                        framer.push(&buf[..n], false, &mut frames);
                        for frame in frames {
                            let Frame::Bytes(raw) = frame else {
                                panic!("no queries expected");
                            };
                            screen.process(&raw);
                            output.extend(raw);
                        }
                    }
                    Err(e) if e.raw_os_error() == Some(5) => break,
                    Err(e) => panic!("PTY read: {e}"),
                }
            }
            assert!(String::from_utf8_lossy(&output).contains("received:hello env:1"));
            assert!(!screen.redraw().is_empty());
            ShellExit::from(shell.child.wait().await.unwrap())
        })
        .await
        .expect("PTY program must terminate");
        assert_eq!(outcome.code(), 37);
    }

    // Closing a PTY must reap even a child that ignores SIGHUP, using the bounded kill fallback.
    #[tokio::test(flavor = "current_thread")]
    async fn hangup_is_bounded() {
        let mut shell = ShellProcess::spawn(
            &ShellCommand::new(
                "/bin/sh",
                ["-c", "trap '' HUP; printf ready; while :; do sleep 1; done"],
            ),
            TermSize { cols: 80, rows: 24 },
        )
        .unwrap();
        let mut ready = [0; 5];
        tokio::time::timeout(Duration::from_secs(3), shell.output.read_exact(&mut ready))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&ready, b"ready");
        let status = tokio::time::timeout(
            Duration::from_secs(3),
            hang_up(shell.output, shell.input, shell.child),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(status.code(), 137);
    }
}
