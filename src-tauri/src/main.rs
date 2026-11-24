// Explorer launches should open only the Desktop window, including for debug builds.
#![cfg_attr(windows, windows_subsystem = "windows")]

use dial_desktop::app::{self, Source};
use dial_desktop::cli::{self, USAGE};
use dial_desktop::launcher::{self, Recents};
use dial_desktop::{agent_browser, browser_mcp};

fn main() {
    // The engine starts this binary as the Browser panel's MCP server; it needs nothing else.
    if std::env::args_os().nth(1).as_deref()
        == Some(std::ffi::OsStr::new(agent_browser::BRIDGE_FLAG))
    {
        std::process::exit(browser_mcp::run());
    }

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

    let home = launcher::home_dir();
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
        None => {
            let choice = launcher::load_choice(launcher::app_dir(home.as_deref()).as_deref());
            let (program, origin) = launcher::locate_dial(
                cli.dial,
                std::env::var_os("DIAL_BIN"),
                choice.as_ref(),
                std::env::var_os("PATH").as_deref(),
                home.as_deref(),
            );
            launcher::note_engine(&program, origin);
            if let Some(warning) = launcher::choice_ignored(choice.as_ref(), origin) {
                launcher::warn(warning);
            }
            // Before any engine starts, so its sessions get the Browser panel's tools.
            if let Ok(exe) = std::env::current_exe()
                && let Err(error) = agent_browser::register(&program, &exe)
            {
                eprintln!("dial-desktop: the agent's browser tools are off: {error}");
                launcher::warn(format!("The agent's browser tools are off: {error}"));
            }
            Source::Engine {
                program,
                args: cli.rpc_args,
            }
        }
    };

    if let Err(error) = app::run(source, workspace, recents) {
        eprintln!("dial-desktop: {error}");
        std::process::exit(1);
    }
}
