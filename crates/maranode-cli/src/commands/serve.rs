//! start maranoded with exec. same process, no parent wrapper

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

const DAEMON_BIN: &str = if cfg!(windows) {
    "maranoded.exe"
} else {
    "maranoded"
};

pub fn resolve_maranoded() -> Result<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join(DAEMON_BIN);
            if sibling.is_file() {
                return Ok(sibling);
            }
        }
    }

    if let Some(path) = find_on_path(DAEMON_BIN) {
        return Ok(path);
    }

    bail!(
        "could not find {DAEMON_BIN}\n\
         Build and install both binaries, e.g.:\n\
           cargo build --release --bin maranode --bin maranoded\n\
         Or put {DAEMON_BIN} on your PATH next to maranode."
    )
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn run(daemon_args: &[impl AsRef<OsStr>]) -> Result<()> {
    let daemon = resolve_maranoded()?;
    exec_daemon(&daemon, daemon_args)
}

fn exec_daemon(daemon: &Path, args: &[impl AsRef<OsStr>]) -> Result<()> {
    let mut cmd = Command::new(daemon);
    cmd.args(args);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        return Err(err).context(format!("failed to exec {}", daemon.display()));
    }

    #[cfg(not(unix))]
    {
        let status = cmd
            .status()
            .context(format!("failed to run {}", daemon.display()))?;
        std::process::exit(status.code().unwrap_or(1));
    }
}
