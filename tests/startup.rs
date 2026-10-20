use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// A real PTY startup must query before shell output, seed the floating background,
// and return to the nonzero cursor without clearing preexisting main-screen text.
#[tokio::test(flavor = "current_thread")]
async fn startup_cpr_preserves_existing_terminal_and_seeds_background() {
    let (pty, pts) = pty_process::open().unwrap();
    pty.resize(pty_process::Size::new(30, 120)).unwrap();
    let key =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/nonexistent-integration-key");
    assert!(!key.exists());
    let mut child = pty_process::Command::new(env!("CARGO_BIN_EXE_dev-null-addict"))
        .env_remove("NO_DEV_NULL_ADDICT")
        .env("TERM", "xterm-256color")
        .args(["--toggle", "ctrl+g", "--key"])
        .arg(key)
        .args([
            "--shell",
            "/bin/sh",
            "--",
            "-c",
            "stty -echo; printf READY; IFS= read -r line; printf 'DONE:%s' \"$line\"; exit 17",
        ])
        .kill_on_drop(true)
        .spawn(pts)
        .unwrap();
    let (mut output, mut input) = pty.into_split();
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let mut query = [0; 4];
        output.read_exact(&mut query).await.unwrap();
        assert_eq!(&query, b"\x1b[6n");
        input.write_all(b"\x1b[10;").await.unwrap();
        input.write_all(b"7R").await.unwrap();
        let mut terminal = vt100::Parser::new(30.try_into().unwrap(), 120.try_into().unwrap(), 0);
        terminal.process(b"EXISTING\x1b[10;7H");
        let mut bytes = [0; 8192];
        loop {
            let n = output.read(&mut bytes).await.unwrap();
            assert!(n > 0);
            terminal.process(&bytes[..n]);
            if terminal.screen().contents().contains("READY") {
                break;
            }
        }
        assert_eq!(terminal.screen().cursor_position(), (9, 11));
        input.write_all(b"\x07").await.unwrap();
        loop {
            let n = output.read(&mut bytes).await.unwrap();
            assert!(n > 0);
            terminal.process(&bytes[..n]);
            if terminal.screen().contents().contains("Not signed in.") {
                break;
            }
        }
        // This part of the background lies to the left of the centered overlay.
        assert_eq!(terminal.screen().cell(9, 6).unwrap().contents(), "R");
        input.write_all(b"\x07hello\n").await.unwrap();
        loop {
            let n = output.read(&mut bytes).await.unwrap();
            assert!(n > 0);
            terminal.process(&bytes[..n]);
            if terminal.screen().contents().contains("DONE:hello") {
                break;
            }
        }
        let rows: Vec<_> = terminal.screen().rows(0, 120).collect();
        assert_eq!(rows[0], "EXISTING");
        assert_eq!(rows[9], "      READYDONE:hello");
        assert_eq!(child.wait().await.unwrap().code(), Some(17));
    })
    .await;
    if result.is_err() {
        let _ = child.kill().await;
    }
    result.expect("bounded CPR startup test");
}
