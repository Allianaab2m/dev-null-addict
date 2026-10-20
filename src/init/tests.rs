use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

use super::*;
use crate::test_support::TestDir;

fn fish_installed() -> bool {
    match Command::new("fish").arg("--version").output() {
        Ok(output) => {
            assert!(output.status.success());
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("fish is not installed; skipping fish execution");
            false
        }
        Err(error) => panic!("could not run fish: {error}"),
    }
}

// Exercise the actual exec words, including the program path, so shell expansion cannot alter arguments.
#[test]
fn quoted_words_survive_shell_parsing() {
    let dir = TestDir::new();
    let program = dir.0.join("a path with 'quotes'");
    std::fs::write(&program, "#!/bin/sh\nprintf '%s\\0' \"$@\"\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let words = [
        "--shell",
        "/shell path/it's",
        "",
        "two words",
        "single'quote",
        r#"\"$HOME $(false); `false` \\ end"#,
        "a\nline",
        "界",
    ];
    let startup = Startup::new(program, words);
    for shell in InitShell::ALL {
        let text = snippet(shell, &startup).unwrap();
        let tail = text.split_once("exec ").unwrap().1;
        let ending = if shell == InitShell::Sh {
            " ;;\n"
        } else {
            "\nend\n"
        };
        let command = tail.rsplit_once(ending).unwrap().0;
        match shell {
            InitShell::Sh => assert!(command.contains("'\\''")),
            InitShell::Fish => {
                assert!(command.contains("/a path with \\'quotes\\'' '--shell' '/shell path/it\\'s' '' 'two words' 'single\\'quote'"));
                assert!(command.contains("\\\\"));
                if !fish_installed() {
                    continue;
                }
            }
        }
        let output = Command::new(shell.as_str())
            .args(["-c", &format!("exec {command}")])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected: Vec<_> = words
            .iter()
            .flat_map(|word| word.as_bytes().iter().copied().chain([0]))
            .collect();
        assert_eq!(output.stdout, expected, "{shell}");
    }
}

// Fish must default to itself, but explicit wrapper options win and shell arguments cannot suppress it.
#[test]
fn fish_shell_default_respects_the_argument_boundary() {
    for (args, inject) in [
        (vec!["--toggle=ctrl+g"], true),
        (vec!["--toggle=ctrl+g", "--shell", "/bin/zsh"], false),
        (vec!["--toggle=ctrl+g", "--shell=/bin/zsh"], false),
        (vec!["--toggle=ctrl+g", "--", "--shell", "child"], true),
        (vec!["--toggle=ctrl+g", "--", "--shell=child"], true),
    ] {
        let startup = Startup::new("/my path/it's", args);
        let fish = snippet(InitShell::Fish, &startup).unwrap();
        assert_eq!(fish.contains("'--shell' (status fish-path)"), inject);
        if inject {
            assert!(
                fish.find("(status fish-path)").unwrap() < fish.find("'--toggle=ctrl+g'").unwrap()
            );
        }
        assert!(
            !snippet(InitShell::Sh, &startup)
                .unwrap()
                .contains("fish-path")
        );
    }
}

// Check complete snippets, not just their quoted words, against the shells' own syntax parsers.
#[test]
fn generated_snippets_pass_syntax_checks() {
    let fish = fish_installed();
    for args in [
        vec!["--toggle=ctrl+g", "--", "it's a path"],
        vec!["--shell=/bin/sh", "--toggle=ctrl+g"],
    ] {
        let startup = Startup::new("/a path/it's", args);
        for shell in InitShell::ALL {
            if shell == InitShell::Fish && !fish {
                continue;
            }
            let flag = if shell == InitShell::Sh {
                "-n"
            } else {
                "--no-execute"
            };
            let output = Command::new(shell.as_str())
                .args([flag, "-c", &snippet(shell, &startup).unwrap()])
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

// Sourcing a startup snippet from a script must leave the script running and produce no wrapper output.
#[test]
fn sh_snippet_is_inert_non_interactively() {
    let startup = Startup::new("/bin/sh", ["-c", "printf 'wrapper ran'; exit 37"]);
    let snippet = snippet(InitShell::Sh, &startup).unwrap();
    for disable in ["", "1"] {
        let output = Command::new("sh")
            .args(["-c", &format!("{snippet}printf 'still running'")])
            .env(crate::DISABLE_ENV, disable)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"still running");
        assert!(output.stderr.is_empty());
    }
}

// Shell text cannot preserve non-UTF-8 paths, so reject them instead of changing the executable or argument.
#[test]
fn non_utf8_words_are_reported_without_replacement() {
    let invalid = OsString::from_vec(vec![0xff]);
    for shell in InitShell::ALL {
        for startup in [
            Startup::new(std::path::PathBuf::from(&invalid), Vec::<OsString>::new()),
            Startup::new("/bin/sh", [invalid.clone()]),
        ] {
            assert_eq!(
                snippet(shell, &startup),
                Err(InitError::NotUtf8(invalid.clone()))
            );
        }
    }
}
