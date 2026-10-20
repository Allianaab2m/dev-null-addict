//! The shell the wrapper runs, and how it ended.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus};

/// A shell program and its arguments.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShellCommand {
    program: PathBuf,
    args: Vec<OsString>,
}

impl ShellCommand {
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

    /// `$SHELL`, or `/bin/sh` when it is unset or empty.
    pub fn from_env<I, A>(args: I) -> Self
    where
        I: IntoIterator<Item = A>,
        A: Into<OsString>,
    {
        Self::new(
            std::env::var_os("SHELL")
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "/bin/sh".into()),
            args,
        )
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// The same shell told not to read its startup files (`bash --norc`,
    /// `zsh --no-rcs`, `fish --no-config`), or `None` for a shell this does
    /// not know.
    ///
    /// This is the way out when a startup file `exec`s the wrapper without
    /// checking [`DISABLE_ENV`](crate::DISABLE_ENV): starting the shell as
    /// usual would run that line, and the wrapper, again.
    pub fn without_startup_files(&self) -> Option<Self> {
        let flag = match self.program.file_name()?.to_str()? {
            "bash" => "--norc",
            "zsh" => "--no-rcs",
            "fish" => "--no-config",
            _ => return None,
        };
        Some(Self::new(
            &self.program,
            std::iter::once(OsString::from(flag)).chain(self.args.iter().cloned()),
        ))
    }

    /// Replaces this process with the shell, with
    /// [`DISABLE_ENV`](crate::DISABLE_ENV) set so that its startup files skip
    /// the wrapper. Returns only if that fails.
    pub fn exec(&self) -> io::Error {
        use std::os::unix::process::CommandExt;
        std::process::Command::new(&self.program)
            .args(&self.args)
            .env(crate::DISABLE_ENV, "1")
            .exec()
    }
}

/// How the shell ended, as the exit code the wrapper passes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShellExit(u8);

impl ShellExit {
    /// The shell's exit code, or 128 plus the signal number when a signal
    /// ended it, as shells report it.
    pub fn code(self) -> u8 {
        self.0
    }
}

impl From<ExitStatus> for ShellExit {
    fn from(status: ExitStatus) -> Self {
        use std::os::unix::process::ExitStatusExt;
        Self(
            status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)) as u8,
        )
    }
}

impl From<ShellExit> for ExitCode {
    fn from(exit: ShellExit) -> Self {
        Self::from(exit.0)
    }
}
