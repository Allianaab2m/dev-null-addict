use std::path::PathBuf;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Trigger(PathBuf);

impl Trigger {
    fn new() -> Self {
        let mut random = [0; 16];
        getrandom::fill(&mut random).unwrap();
        let name: String = random.iter().map(|b| format!("{b:02x}")).collect();
        Self(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join(format!("float-{name}")),
        )
    }
}

impl Drop for Trigger {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn until(output: &mut pty_process::OwnedReadPty, screen: &mut vt100::Parser, needle: &str) {
    let mut bytes = [0; 8192];
    while !screen.screen().contents().contains(needle) {
        let n = output.read(&mut bytes).await.expect("PTY output");
        assert!(n > 0, "wrapper exited before {needle}");
        screen.process(&bytes[..n]);
    }
}

async fn live_background(alternate: bool) {
    let trigger = Trigger::new();
    let key = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/nonexistent-integration-key");
    assert!(!key.exists());
    let (pty, pts) = pty_process::open().unwrap();
    pty.resize(pty_process::Size::new(30, 120)).unwrap();
    let script = format!(
        "stty -echo; printf '{}READY'; while [ ! -e \"$1\" ]; do sleep 0.01; done; printf '\\r\\033[31;44;1mLIVE界B\\033[m'; IFS= read -r line; printf '\\nDONE:%s\\n' \"$line\"; exit 23",
        if alternate { "\\033[?1049h" } else { "" }
    );
    let mut child = pty_process::Command::new(env!("CARGO_BIN_EXE_dev-null-addict"))
        .env_remove("NO_DEV_NULL_ADDICT")
        .env_remove("NO_COLOR")
        .env("TERM", "xterm-256color")
        .args(["--toggle", "ctrl+g", "--shell", "/bin/sh", "--key"])
        .arg(&key)
        .args(["--", "-c", &script, "float-test"])
        .arg(&trigger.0)
        .kill_on_drop(true)
        .spawn(pts)
        .unwrap();
    let (mut output, mut input) = pty.into_split();
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        let mut screen = vt100::Parser::new(30.try_into().unwrap(), 120.try_into().unwrap(), 0);
        until(&mut output, &mut screen, "READY").await;
        input.write_all(b"\x07").await.unwrap();
        until(&mut output, &mut screen, "Not signed in.").await;
        assert_eq!(screen.screen().cell(3, 12).unwrap().contents(), "┌");
        assert!(screen.screen().contents().contains("READY"));
        assert!(
            !screen.screen().contents().contains("Keys"),
            "the 96-column window has no help column"
        );

        std::fs::File::create_new(&trigger.0).unwrap();
        until(&mut output, &mut screen, "LIVE界B").await;
        let cell = screen.screen().cell(0, 0).unwrap();
        assert_eq!(cell.fgcolor(), vt100::Color::Idx(8));
        assert_eq!(cell.bgcolor(), vt100::Color::Default);
        assert!(!cell.bold());
        assert_eq!(screen.screen().cell(0, 4).unwrap().contents(), "界");
        assert_eq!(screen.screen().cell(0, 6).unwrap().contents(), "B");
        input.write_all(b"typed immediately").await.unwrap();
        until(&mut output, &mut screen, "typed immediately").await;
        input.write_all(b"\x07hello\n").await.unwrap();
        until(&mut output, &mut screen, "DONE:hello").await;
        assert_eq!(child.wait().await.unwrap().code(), Some(23));
    })
    .await;
    if result.is_err() {
        let _ = child.kill().await;
    }
    result.expect("bounded floating overlay test");
}

// A real PTY verifies the default float, live dimmed output, input responsiveness, replay, and shell exit status.
#[tokio::test(flavor = "current_thread")]
async fn floating_background_updates_and_replays() {
    live_background(false).await;
}

// The same background update must work over an alternate-screen shell and retain the redraw close path.
#[tokio::test(flavor = "current_thread")]
async fn floating_background_updates_and_redraws() {
    live_background(true).await;
}
