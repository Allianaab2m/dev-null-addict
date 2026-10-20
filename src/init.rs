//! Startup snippets that start the wrapper from a shell's startup file.
//!
//! In `~/.bashrc` or `~/.zshrc`:
//!
//! ```sh
//! eval "$(dev-null-addict init sh --toggle ctrl+g)"
//! ```
//!
//! In `~/.config/fish/config.fish`:
//!
//! ```fish
//! dev-null-addict init fish --toggle ctrl+g | source
//! ```

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

/// A shell that `init` writes a snippet for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InitShell {
    /// bash, zsh and other POSIX shells.
    Sh,
    Fish,
}

impl InitShell {
    pub const ALL: [Self; 2] = [Self::Sh, Self::Fish];

    /// The name `init` takes, also accepted by [`FromStr`].
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sh => "sh",
            Self::Fish => "fish",
        }
    }
}

impl fmt::Display for InitShell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for InitShell {
    type Err = InitShellError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|shell| shell.as_str() == s)
            .ok_or_else(|| InitShellError(s.to_owned()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("expected sh or fish, not {0:?}")]
pub struct InitShellError(String);

/// How a snippet starts the wrapper: the program, by its absolute path, and
/// the arguments it gets.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Startup {
    program: PathBuf,
    args: Vec<OsString>,
}

impl Startup {
    pub fn new<I, A>(program: impl Into<PathBuf>, args: I) -> Self
    where
        I: IntoIterator<Item = A>,
        A: Into<OsString>,
    {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

/// The snippet for `shell` that `exec`s the wrapper.
///
/// It does nothing when [`DISABLE_ENV`](crate::DISABLE_ENV) is non-empty,
/// the shell is not interactive, or stdin or stdout is not a terminal. Every
/// word is quoted for the shell, so paths and arguments with spaces or
/// quotes are safe.
///
/// For [`InitShell::Sh`] it is POSIX sh (`[ ... ]` and `case $- in`), so it
/// works in bash, zsh and dash alike. For [`InitShell::Fish`], when the
/// arguments have no `--shell`, the snippet adds `--shell (status
/// fish-path)`, so that the wrapper starts fish even when `$SHELL` names
/// another shell, as it often does for fish users.
pub fn snippet(shell: InitShell, startup: &Startup) -> Result<String, InitError> {
    let program = quote(shell, startup.program.as_os_str())?;
    let args = startup
        .args
        .iter()
        .map(|arg| quote(shell, arg))
        .collect::<Result<Vec<_>, _>>()?;
    let has_shell = startup
        .args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--shell" || arg.to_str().is_some_and(|arg| arg.starts_with("--shell=")));
    let fish_shell = if shell == InitShell::Fish && !has_shell {
        " '--shell' (status fish-path)"
    } else {
        ""
    };
    let command = std::iter::once(format!("{program}{fish_shell}"))
        .chain(args)
        .collect::<Vec<_>>()
        .join(" ");
    let disable = crate::DISABLE_ENV;
    Ok(match shell {
        InitShell::Sh => format!(
            "if [ -z \"${{{disable}:-}}\" ] && [ -t 0 ] && [ -t 1 ]; then\n    case $- in\n        *i*) exec {command} ;;\n    esac\nfi\n"
        ),
        InitShell::Fish => format!(
            "if test -z \"${disable}\"; and status is-interactive; and isatty stdin; and isatty stdout\n    exec {command}\nend\n"
        ),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InitError {
    #[error("{0:?} is not valid UTF-8, so it can't go into a shell snippet")]
    NotUtf8(OsString),
}

fn quote(shell: InitShell, word: &std::ffi::OsStr) -> Result<String, InitError> {
    let word = word
        .to_str()
        .ok_or_else(|| InitError::NotUtf8(word.to_owned()))?;
    let escaped = match shell {
        InitShell::Sh => word.replace('\'', "'\\''"),
        InitShell::Fish => word.replace('\\', "\\\\").replace('\'', "\\'"),
    };
    Ok(format!("'{escaped}'"))
}

#[cfg(test)]
mod tests;
