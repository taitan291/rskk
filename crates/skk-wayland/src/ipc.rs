//! Control socket so that `rskk-wayland direct` / `rskk-wayland normal` can switch the running
//! instance's input mode from outside (for example from a compositor keybinding or nvim).

use std::{
    env, fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Pass every key through untouched.
    Direct,
    /// Leave direct mode and return to SKK input.
    Normal,
}

impl Command {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "direct" => Some(Self::Direct),
            "normal" => Some(Self::Normal),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Normal => "normal",
        }
    }
}

pub fn socket_path() -> PathBuf {
    if let Some(path) = env::var_os("RSKK_SOCKET") {
        return PathBuf::from(path);
    }
    let dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    dir.join("rskk.sock")
}

pub fn bind() -> Result<UnixListener> {
    let path = socket_path();
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            bail!(
                "another rskk-wayland is already running ({})",
                path.display()
            );
        }
        fs::remove_file(&path).with_context(|| format!("remove stale {}", path.display()))?;
    }
    let listener = UnixListener::bind(&path).with_context(|| format!("bind {}", path.display()))?;
    listener
        .set_nonblocking(true)
        .context("configure control socket")?;
    Ok(listener)
}

/// Accept every pending connection and return the commands received.
pub fn drain(listener: &UnixListener) -> Vec<Command> {
    let mut commands = Vec::new();
    while let Ok((stream, _)) = listener.accept() {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
        let mut line = String::new();
        let mut reader = BufReader::new(&stream);
        let reply = match reader.read_line(&mut line) {
            Ok(_) => match Command::parse(line.trim()) {
                Some(command) => {
                    commands.push(command);
                    "ok\n"
                }
                None => "error: unknown command\n",
            },
            Err(_) => "error: read failed\n",
        };
        let _ = (&stream).write_all(reply.as_bytes());
    }
    commands
}

pub fn send(command: Command) -> Result<()> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("connect {} (is rskk-wayland running?)", path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    writeln!(stream, "{}", command.name()).context("send command")?;
    let mut reply = String::new();
    BufReader::new(&stream)
        .read_line(&mut reply)
        .context("read reply")?;
    if reply.trim() != "ok" {
        bail!("rskk-wayland replied: {}", reply.trim());
    }
    Ok(())
}
