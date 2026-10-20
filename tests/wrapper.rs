use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn read_until(pty: &mut pty_process::OwnedReadPty, output: &mut Vec<u8>, needle: &[u8]) {
    let mut chunk = [0; 8192];
    loop {
        let mut screen = vt100::Parser::new(30.try_into().unwrap(), 120.try_into().unwrap(), 0);
        screen.process(output);
        if screen
            .screen()
            .contents()
            .contains(std::str::from_utf8(needle).unwrap())
        {
            return;
        }
        let count = pty.read(&mut chunk).await.expect("wrapper output");
        assert_ne!(count, 0, "wrapper ended before marker");
        output.extend_from_slice(&chunk[..count]);
        assert!(output.len() < 1 << 20, "unexpected output flood");
    }
}

async fn wrapper_round_trip(alternate: bool) {
    let (pty, pts) = pty_process::open().unwrap();
    pty.resize(pty_process::Size::new(30, 120)).unwrap();
    let key =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/nonexistent-integration-key");
    assert!(
        !key.exists(),
        "integration test must never load a real device key"
    );
    let script = format!(
        "stty -echo; printf '{}READY\\n'; IFS= read -r line; printf 'RECEIVED:%s\\n' \"$line\"; exit 23",
        if alternate { "\\033[?1049h" } else { "" }
    );
    let mut child = pty_process::Command::new(env!("CARGO_BIN_EXE_dev-null-addict"))
        .env_remove("NO_DEV_NULL_ADDICT")
        .env("TERM", "xterm-256color")
        .args([
            "--toggle",
            "ctrl+g",
            "--full-screen",
            "--shell",
            "/bin/sh",
            "--key",
        ])
        .arg(key)
        .args(["--", "-c", &script])
        .kill_on_drop(true)
        .spawn(pts)
        .unwrap();
    let (mut output, mut input) = pty.into_split();
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        let mut seen = Vec::new();
        read_until(&mut output, &mut seen, b"READY").await;
        input.write_all(b"\x07").await.unwrap();
        seen.clear();
        read_until(&mut output, &mut seen, b"Not signed in.").await;
        assert!(seen.windows(4).any(|w| w == b"Keys"));
        input.write_all(b"\x07hello\n").await.unwrap();
        seen.clear();
        read_until(&mut output, &mut seen, b"RECEIVED:hello").await;
        assert_eq!(child.wait().await.unwrap().code(), Some(23));
    })
    .await;
    if result.is_err() {
        let _ = child.kill().await;
    }
    result.expect("bounded wrapper test");
}

// The complete wrapper must open, show the login notice, close, and forward the remainder of the same read.
#[tokio::test(flavor = "current_thread")]
async fn wrapper_overlay_replay_and_exit() {
    wrapper_round_trip(false).await;
}

// Starting over an alternate-screen application must use the redraw close path and preserve subsequent input.
#[tokio::test(flavor = "current_thread")]
async fn wrapper_overlay_alternate_redraw_and_exit() {
    wrapper_round_trip(true).await;
}

// Help and login syntax errors must exit without falling back to an interactive shell.
#[test]
fn cli_help_and_login_error_do_not_exec_shell() {
    let help = std::process::Command::new(env!("CARGO_BIN_EXE_dev-null-addict"))
        .args(["--help"])
        .env("SHELL", "/does-not-exist")
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--toggle"));
    let login = std::process::Command::new(env!("CARGO_BIN_EXE_dev-null-addict"))
        .args(["login", "--invalid"])
        .env("SHELL", "/does-not-exist")
        .output()
        .unwrap();
    assert_eq!(login.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&login.stderr).contains("fallback"));
}

// A hidden application must receive local and forwarded query replies, and neither query may be replayed.
#[tokio::test(flavor = "current_thread")]
async fn hidden_queries_are_answered_without_replay() {
    let (pty, pts) = pty_process::open().unwrap();
    pty.resize(pty_process::Size::new(30, 120)).unwrap();
    let key =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/nonexistent-integration-key");
    assert!(!key.exists());
    let script = "stty -echo -icanon min 1 time 0; printf READY; sleep 0.5; printf '\\033[6n'; reply=$(dd bs=1 count=6 2>/dev/null | od -An -tx1); printf 'LOCAL:%s\\n' \"$reply\"; printf '\\033[c'; reply=$(dd bs=1 count=7 2>/dev/null | od -An -tx1); printf 'REMOTE:%s\\n' \"$reply\"; IFS= read -r line; printf 'DONE:%s\\n' \"$line\"; exit 19";
    let mut child = pty_process::Command::new(env!("CARGO_BIN_EXE_dev-null-addict"))
        .env_remove("NO_DEV_NULL_ADDICT")
        .env("TERM", "xterm-256color")
        .args(["--toggle", "ctrl+g", "--key"])
        .arg(key)
        .args(["--shell", "/bin/sh", "--", "-c", script])
        .kill_on_drop(true)
        .spawn(pts)
        .unwrap();
    let (mut output, mut input) = pty.into_split();
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        let mut seen = Vec::new();
        read_until(&mut output, &mut seen, b"READY").await;
        input.write_all(b"\x07").await.unwrap();
        seen.clear();
        read_until(&mut output, &mut seen, b"Not signed in.").await;
        let mut chunk = [0; 8192];
        while !seen.windows(3).any(|w| w == b"\x1b[c") {
            let n = output.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            seen.extend_from_slice(&chunk[..n]);
        }
        assert!(!seen.windows(4).any(|w| w == b"\x1b[6n"));
        input.write_all(b"\x1b[?1;2c").await.unwrap();
        input.write_all(b"\x07hello\n").await.unwrap();
        seen.clear();
        read_until(&mut output, &mut seen, b"DONE:hello").await;
        let text = String::from_utf8_lossy(&seen);
        assert!(text.contains("LOCAL: 1b 5b 31 3b 36 52"), "{text:?}");
        assert!(text.contains("REMOTE: 1b 5b 3f 31 3b 32 63"), "{text:?}");
        assert!(!seen.windows(3).any(|w| w == b"\x1b[c"));
        assert!(!seen.windows(4).any(|w| w == b"\x1b[6n"));
        assert_eq!(child.wait().await.unwrap().code(), Some(19));
    })
    .await;
    if result.is_err() {
        let _ = child.kill().await;
    }
    result.expect("bounded query test");
}

// An incomplete output sequence must be cancelled before takeover, and a resize while open must reach the shell.
#[tokio::test(flavor = "current_thread")]
async fn boundary_timeout_and_resize_redraw() {
    let (pty, pts) = pty_process::open().unwrap();
    pty.resize(pty_process::Size::new(30, 120)).unwrap();
    let key =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/nonexistent-integration-key");
    assert!(!key.exists());
    let script = "stty -echo; printf 'READY\\033[31'; IFS= read -r line; printf 'SIZE:%s\\nDONE:%s\\n' \"$(stty size)\" \"$line\"; exit 29";
    let mut child = pty_process::Command::new(env!("CARGO_BIN_EXE_dev-null-addict"))
        .env_remove("NO_DEV_NULL_ADDICT")
        .env("TERM", "xterm-256color")
        .args(["--toggle", "ctrl+g", "--key"])
        .arg(key)
        .args(["--shell", "/bin/sh", "--", "-c", script])
        .kill_on_drop(true)
        .spawn(pts)
        .unwrap();
    let (mut output, mut input) = pty.into_split();
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        let mut seen = Vec::new();
        read_until(&mut output, &mut seen, b"READY").await;
        input.write_all(b"\x07").await.unwrap();
        seen.clear();
        read_until(&mut output, &mut seen, b"Not signed in.").await;
        let cancel = seen
            .iter()
            .position(|b| *b == 0x18)
            .expect("CAN before takeover");
        let enter = seen
            .windows(8)
            .position(|w| w == b"\x1b[?1049h")
            .expect("alternate takeover");
        assert!(cancel < enter);
        input.resize(pty_process::Size::new(20, 80)).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        input.write_all(b"\x07hello\n").await.unwrap();
        seen.clear();
        read_until(&mut output, &mut seen, b"DONE:hello").await;
        assert!(
            String::from_utf8_lossy(&seen).contains("SIZE:20 80"),
            "{:?}",
            String::from_utf8_lossy(&seen)
        );
        assert_eq!(child.wait().await.unwrap().code(), Some(29));
    })
    .await;
    if result.is_err() {
        let _ = child.kill().await;
    }
    result.expect("bounded resize test");
}

// Nested and non-TTY failures use the selected recovery shell rather than recursively launching the wrapper.
#[test]
fn fallback_execs_a_bounded_recovery_command() {
    let fallback = std::process::Command::new(env!("CARGO_BIN_EXE_dev-null-addict"))
        .env_remove("NO_DEV_NULL_ADDICT")
        .args([
            "--toggle",
            "ctrl+g",
            "--shell",
            "/bin/sh",
            "--",
            "-c",
            "printf recovered; exit 41",
        ])
        .output()
        .unwrap();
    assert_eq!(fallback.status.code(), Some(41));
    assert_eq!(fallback.stdout, b"recovered");
    assert!(String::from_utf8_lossy(&fallback.stderr).contains("terminal"));
    let nested = std::process::Command::new(env!("CARGO_BIN_EXE_dev-null-addict"))
        .env("NO_DEV_NULL_ADDICT", "1")
        .args(["--toggle", "ctrl+g", "--shell", "/unknown-shell"])
        .output()
        .unwrap();
    assert_eq!(nested.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&nested.stderr).contains("cannot safely skip startup files"));
}

// SIGTERM must hang up the inner shell and return its status instead of leaving the wrapper's raw terminal active.
#[tokio::test(flavor = "current_thread")]
async fn termination_signal_hangs_up_shell() {
    let (pty, pts) = pty_process::open().unwrap();
    pty.resize(pty_process::Size::new(24, 80)).unwrap();
    let mut child = pty_process::Command::new(env!("CARGO_BIN_EXE_dev-null-addict"))
        .env_remove("NO_DEV_NULL_ADDICT")
        .args([
            "--toggle",
            "ctrl+g",
            "--shell",
            "/bin/sh",
            "--",
            "-c",
            "trap 'exit 42' HUP; printf READY; while :; do read line; done",
        ])
        .kill_on_drop(true)
        .spawn(pts)
        .unwrap();
    let (mut output, _input) = pty.into_split();
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        read_until(&mut output, &mut Vec::new(), b"READY").await;
        assert!(
            std::process::Command::new("kill")
                .args(["-TERM", &child.id().unwrap().to_string()])
                .status()
                .unwrap()
                .success()
        );
        // Poll here too: this environment's Tokio signal dispatcher misses SIGCHLD.
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(42));
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    if result.is_err() {
        let _ = child.kill().await;
    }
    result.expect("bounded termination test");
}

// Shell exit must end the wrapper after its short drain window even when a background process retains the PTY.
#[tokio::test(flavor = "current_thread")]
async fn shell_exit_drain_is_bounded() {
    let (pty, pts) = pty_process::open().unwrap();
    pty.resize(pty_process::Size::new(24, 80)).unwrap();
    let mut child = pty_process::Command::new(env!("CARGO_BIN_EXE_dev-null-addict"))
        .env_remove("NO_DEV_NULL_ADDICT")
        .args([
            "--toggle",
            "ctrl+g",
            "--shell",
            "/bin/sh",
            "--",
            "-c",
            "sleep 2 & printf FINAL; exit 31",
        ])
        .kill_on_drop(true)
        .spawn(pts)
        .unwrap();
    let (mut output, _input) = pty.into_split();
    let result = tokio::time::timeout(Duration::from_secs(1), async {
        read_until(&mut output, &mut Vec::new(), b"FINAL").await;
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(31));
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    if result.is_err() {
        let _ = child.kill().await;
    }
    result.expect("shell exit must not wait for its background job");
}
