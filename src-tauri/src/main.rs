// No console window behind the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use dial_desktop::app::{self, Source};
use dial_desktop::cli::{self, USAGE};
use dial_desktop::launcher::{self, Recents};
use std::path::PathBuf;

fn main() {
    // First, while single-threaded: a Finder launch lacks the shell's PATH and API keys.
    launcher::import_shell_env();

    let cli = match cli::parse(std::env::args_os().skip(1)) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("dial-desktop: {error}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    if cli.help {
        println!("{USAGE}");
        return;
    }

    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let recents = Recents::load(Recents::default_file(home.as_deref()));
    let cwd = std::env::current_dir().unwrap_or_default();
    let workspace = launcher::choose_workspace(
        cli.workspace.as_deref(),
        &cwd,
        home.as_deref(),
        &recents.paths,
    )
    .map(|path| path.canonicalize().unwrap_or(path));
    if let Some(path) = workspace.as_ref().filter(|path| !path.is_dir()) {
        eprintln!("dial-desktop: {} is not a directory", path.display());
        std::process::exit(2);
    }

    let source = match cli.preview {
        Some(kind) => Source::Preview(kind),
        None => Source::Engine {
            program: launcher::find_dial(
                cli.dial.or_else(|| std::env::var_os("DIAL_BIN")),
                std::env::var_os("PATH").as_deref(),
                home.as_deref(),
            ),
            args: cli.rpc_args,
        },
    };

    if let Err(error) = app::run(source, workspace, recents) {
        eprintln!("dial-desktop: {error}");
        std::process::exit(1);
    }
}
