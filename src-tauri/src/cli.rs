//! Command-line options.

use std::ffi::OsString;
use std::path::PathBuf;

#[cfg(test)]
mod tests;

pub const USAGE: &str = "\
Usage: dial-desktop [OPTIONS] [WORKSPACE] [-- RPC_ARGS...]

Opens the Dial desktop app on WORKSPACE. Without one it uses the current directory when started
from a project in a terminal, else the most recently opened folder, else it asks for a folder.

Options:
  --dial PATH         Dial engine binary (default: $DIAL_BIN, else `dial` on PATH or in
                      ~/.local/bin, ~/.cargo/bin, /opt/homebrew/bin or /usr/local/bin)
  --preview[=KIND]    Show the design preview without an engine; KIND is `session` (default),
                      `question` or `empty`
  -h, --help          Show this help

Arguments after `--` are passed to `dial rpc`, for example `-- --model local`.";

/// Which fixture the design preview shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preview {
    /// A session mid-turn: every kind of row, an approval and a plan.
    Session,
    /// A fresh session's empty state.
    Empty,
    /// The session waiting on a question from the agent instead of an approval.
    Question,
}

/// Parsed options.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cli {
    pub workspace: Option<PathBuf>,
    pub dial: Option<OsString>,
    pub preview: Option<Preview>,
    pub rpc_args: Vec<OsString>,
    pub help: bool,
}

/// Parse arguments (without the program name).
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        match text.as_ref() {
            "--" => {
                cli.rpc_args.extend(args.by_ref());
                break;
            }
            "-h" | "--help" => cli.help = true,
            "--preview" | "--preview=session" => cli.preview = Some(Preview::Session),
            "--preview=empty" => cli.preview = Some(Preview::Empty),
            "--preview=question" => cli.preview = Some(Preview::Question),
            "--dial" => {
                cli.dial = Some(args.next().ok_or("--dial needs a path")?);
            }
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}`")),
            _ if cli.workspace.is_none() => cli.workspace = Some(arg.into()),
            extra => return Err(format!("unexpected argument `{extra}`")),
        }
    }
    Ok(cli)
}
