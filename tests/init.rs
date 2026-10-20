use std::process::{Command, Stdio};

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dev-null-addict"));
    command
        .stdin(Stdio::null())
        .env("SHELL", "/no-such-fallback-shell");
    command
}

// Init must print a snippet even without a terminal, using the running executable's absolute path.
#[test]
fn init_dispatches_without_starting_a_wrapper() {
    let output = command()
        .args(["init", "sh", "--toggle=ctrl+g"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snippet = String::from_utf8(output.stdout).unwrap();
    let program = std::fs::canonicalize(env!("CARGO_BIN_EXE_dev-null-addict")).unwrap();
    let quoted = program.to_str().unwrap().replace('\'', "'\\''");
    assert!(snippet.contains(&format!("exec '{quoted}' '--toggle=ctrl+g'")));
    assert!(snippet.contains("case $- in"));
    assert!(output.stderr.is_empty());
}

// Init argument errors must exit through clap, without attempting any fallback shell.
#[test]
fn init_parse_errors_never_fall_back() {
    for args in [
        vec!["init", "sh"],
        vec!["init", "unknown", "--toggle=ctrl+g"],
        vec!["init", "fish", "--toggle=ctrl+i"],
        vec!["init", "sh", "--toggle=ctrl+g", "--unknown"],
    ] {
        let output = command().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("error:"));
        assert!(!error.contains("fallback"));
    }
}

// Init must retain the opt-out from floating layout for either startup shell.
#[test]
fn init_forwards_full_screen() {
    for shell in ["sh", "fish"] {
        let output = command()
            .args(["init", shell, "--toggle=ctrl+g", "--full-screen"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let snippet = String::from_utf8(output.stdout).unwrap();
        assert!(snippet.contains("'--full-screen'"));
    }
}
