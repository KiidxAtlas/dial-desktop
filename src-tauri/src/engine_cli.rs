//! Engine features that have no RPC yet, reached by running `dial` subcommands: the capability
//! doctor, stored provider accounts, and putting the engine on the terminal's PATH.
//!
//! Each call runs one fixed subcommand with validated arguments and a time limit; nothing here
//! needs an open project. Text output is parsed into structures the UI draws; lines that do not
//! parse are kept as notes rather than dropped.

use crate::updates::{RunOutput, last_lines, run_bounded};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[cfg(test)]
mod tests;

const LIST_TIMEOUT: Duration = Duration::from_secs(60);
/// Installing a language server or runtime downloads and may build.
const FIX_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const OUTPUT_TAIL: usize = 6000;

/// One optional capability from `dial doctor --all`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    /// The id `dial doctor --fix` takes; empty for installed ones (the doctor prints none).
    pub id: String,
    pub name: String,
    pub installed: bool,
    /// Where it was found, for installed ones.
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub capabilities: Vec<Capability>,
    /// Everything after the capability lists (memory backends and the like), as printed.
    pub notes: Vec<String>,
}

/// Ids are command names: `gopls`, `vscode-html-language-server`, `helm_ls`.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Split a doctor row at its column gap (two or more spaces).
fn columns(row: &str) -> (&str, &str) {
    match row.find("  ") {
        Some(at) => (row[..at].trim(), row[at..].trim()),
        None => (row.trim(), ""),
    }
}

/// Parse the text of `dial doctor --all`.
pub fn parse_doctor(text: &str) -> DoctorReport {
    let mut report = DoctorReport::default();
    let mut in_lists = true;
    for line in text.lines() {
        let trimmed = line.trim();
        if in_lists {
            if let Some(row) = trimmed.strip_prefix('✓') {
                let (name, path) = columns(row.trim_start());
                report.capabilities.push(Capability {
                    id: String::new(),
                    name: name.to_owned(),
                    installed: true,
                    path: (!path.is_empty()).then(|| path.to_owned()),
                });
                continue;
            }
            if let Some(row) = trimmed.strip_prefix('·') {
                let (name, command) = columns(row.trim_start());
                let id = command
                    .rsplit_once("--fix ")
                    .map_or("", |(_, id)| id.trim());
                if valid_id(id) {
                    report.capabilities.push(Capability {
                        id: id.to_owned(),
                        name: name.to_owned(),
                        installed: false,
                        path: None,
                    });
                    continue;
                }
            }
            if trimmed.is_empty()
                || trimmed.starts_with("Installed (")
                || trimmed.starts_with("Not installed (")
            {
                continue;
            }
            in_lists = false;
        }
        report.notes.push(line.trim_end().to_owned());
    }
    while report.notes.last().is_some_and(|l| l.is_empty()) {
        report.notes.pop();
    }
    report
}

fn failed(output: &RunOutput, what: &str) -> String {
    let text = last_lines(
        &format!("{}\n{}", output.stdout.trim(), output.stderr.trim()),
        OUTPUT_TAIL,
    );
    if text.is_empty() {
        format!("{what} failed")
    } else {
        text
    }
}

fn dial(program: &Path, cwd: Option<&Path>) -> Command {
    let mut command = Command::new(program);
    if let Some(cwd) = cwd.filter(|p| p.is_dir()) {
        command.current_dir(cwd);
    }
    command
}

/// `dial doctor --all`.
pub fn doctor(program: &Path, cwd: Option<&Path>) -> Result<DoctorReport, String> {
    let output = run_bounded(dial(program, cwd).args(["doctor", "--all"]), LIST_TIMEOUT)?;
    if !output.success {
        return Err(failed(&output, "dial doctor"));
    }
    Ok(parse_doctor(&output.stdout))
}

/// A commit message `dial commit --dry-run` wrote for the staged changes; nothing is committed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitPreview {
    pub message: String,
    /// The engine's validation warning about the message, if any.
    pub warning: Option<String>,
}

/// Model calls can take a while; a dry run never commits, so a slow one is only a wait.
const COMMIT_PREVIEW_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// `dial commit --dry-run` in the project: the engine's AI message for what is staged.
pub fn commit_message(program: &Path, cwd: &Path) -> Result<CommitPreview, String> {
    let output = run_bounded(
        dial(program, Some(cwd)).args(["commit", "--dry-run", "--no-changelog"]),
        COMMIT_PREVIEW_TIMEOUT,
    )?;
    if !output.success {
        return Err(failed(&output, "dial commit --dry-run"));
    }
    parse_commit_preview(&output.stdout)
}

/// The message in `dial commit --dry-run` output (`Dry run. Message:` then the message, then an
/// optional blank line and `Warning: …`).
pub fn parse_commit_preview(stdout: &str) -> Result<CommitPreview, String> {
    const HEAD: &str = "Dry run. Message:\n";
    let Some(at) = stdout.find(HEAD) else {
        let said = stdout.trim();
        return Err(if said.starts_with("Dry run. No changes") || said.starts_with("No changes") {
            "There are no changes to describe".to_owned()
        } else if said.is_empty() {
            "Dial wrote no commit message".to_owned()
        } else {
            last_lines(said, OUTPUT_TAIL)
        });
    };
    let rest = stdout[at + HEAD.len()..].trim_end();
    let (message, warning) = match rest.rfind("\n\nWarning: ") {
        Some(split) => (&rest[..split], Some(rest[split + "\n\nWarning: ".len()..].trim().to_owned())),
        None => (rest, None),
    };
    let message = message.trim().to_owned();
    if message.is_empty() {
        return Err("Dial wrote no commit message".into());
    }
    Ok(CommitPreview { message, warning })
}

/// `dial doctor --fix <id>`: the preview of what would run, or with `apply` the install itself.
/// Returns what it printed.
pub fn doctor_fix(
    program: &Path,
    cwd: Option<&Path>,
    id: &str,
    apply: bool,
) -> Result<String, String> {
    if !valid_id(id) {
        return Err(format!("`{id}` is not a capability id"));
    }
    let mut command = dial(program, cwd);
    command.args(["doctor", "--fix", id]);
    if apply {
        command.arg("--yes");
    }
    let output = run_bounded(&mut command, if apply { FIX_TIMEOUT } else { LIST_TIMEOUT })?;
    if !output.success {
        return Err(failed(&output, "dial doctor --fix"));
    }
    Ok(last_lines(
        &format!("{}\n{}", output.stdout.trim(), output.stderr.trim()),
        OUTPUT_TAIL,
    ))
}

/// One stored sign-in of an OAuth provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// What `select` and `remove` take.
    pub key: String,
    /// The rest of the line (a name or address), when the engine prints one.
    pub detail: String,
    pub active: bool,
}

/// Parse `dial auth accounts <provider> list`: one account per line, the active one marked `*`.
/// A line without a key-like first word (a "no stored accounts" sentence) is not an account.
pub fn parse_accounts(text: &str) -> Vec<Account> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (active, rest) = match line.strip_prefix('*') {
                Some(rest) => (true, rest.trim_start()),
                None => (false, line),
            };
            let (key, detail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if key.is_empty()
                || line.to_ascii_lowercase().starts_with("no stored")
                || key.ends_with(':')
            {
                return None;
            }
            Some(Account {
                key: key.to_owned(),
                detail: detail.trim().to_owned(),
                active,
            })
        })
        .collect()
}

/// What to do with a provider's stored accounts.
pub enum AccountAction<'a> {
    List,
    Select(&'a str),
    Remove(&'a str),
}

/// `dial auth accounts <provider> [list|select|remove]`; always answers with the list afterwards.
pub fn accounts(
    program: &Path,
    cwd: Option<&Path>,
    provider: &str,
    action: AccountAction,
) -> Result<Vec<Account>, String> {
    if !valid_id(provider) {
        return Err(format!("`{provider}` is not a provider id"));
    }
    let run = |args: &[&str]| -> Result<RunOutput, String> {
        let mut command = dial(program, cwd);
        command.args(["auth", "accounts", provider]).args(args);
        let output = run_bounded(&mut command, LIST_TIMEOUT)?;
        if output.success {
            Ok(output)
        } else {
            Err(failed(&output, "dial auth accounts"))
        }
    };
    match action {
        AccountAction::List => {}
        AccountAction::Select(key) | AccountAction::Remove(key)
            if key.is_empty() || key.starts_with('-') =>
        {
            return Err("Choose an account first".into());
        }
        AccountAction::Select(key) => drop(run(&["select", key])?),
        AccountAction::Remove(key) => drop(run(&["remove", key])?),
    }
    Ok(parse_accounts(&run(&["list"])?.stdout))
}

// ── `dial` in the terminal ───────────────────────────────────────────────────

/// Whether typing `dial` in a terminal reaches the engine Desktop uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliLink {
    /// The engine Desktop runs.
    pub engine: PathBuf,
    /// Where the command would be (or is) placed.
    pub link: PathBuf,
    /// `link` already runs `engine`.
    pub linked: bool,
    /// `link`'s folder is on this process's PATH (which includes the login shell's).
    pub on_path: bool,
    /// A different `dial` the terminal finds first, if any.
    pub other: Option<PathBuf>,
    /// Something that is not Desktop's link already sits at `link`; it is never replaced.
    pub blocked: bool,
    /// How new terminals get `link`'s folder on PATH, where Desktop can arrange that.
    pub shell: Option<ShellPath>,
}

/// First line of the Windows shim, so only our own file is ever replaced.
const SHIM_MARK: &str = "@rem Dial Desktop command-line link";

/// Where the terminal command goes: `~/.local/bin/dial`, or on Windows a `dial.cmd` in the
/// per-user WindowsApps folder, which is on PATH by default.
pub fn link_path(os: &str, home: Option<&Path>, local_app_data: Option<&Path>) -> Option<PathBuf> {
    match os {
        "windows" => Some(
            local_app_data
                .filter(|p| p.is_absolute())?
                .join("Microsoft")
                .join("WindowsApps")
                .join("dial.cmd"),
        ),
        _ => Some(home.filter(|p| p.is_absolute())?.join(".local/bin/dial")),
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn shim_text(engine: &Path) -> String {
    format!("{SHIM_MARK}\r\n@\"{}\" %*\r\n", engine.display())
}

/// Whether `link` is ours to replace: a symlink, or a shim carrying our mark.
fn replaceable(link: &Path) -> bool {
    match std::fs::symlink_metadata(link) {
        Err(_) => true,
        Ok(meta) if meta.file_type().is_symlink() => true,
        Ok(meta) if meta.is_file() && link.extension().is_some_and(|e| e == "cmd") => {
            std::fs::read_to_string(link).is_ok_and(|text| text.starts_with(SHIM_MARK))
        }
        Ok(_) => false,
    }
}

fn runs_engine(link: &Path, engine: &Path) -> bool {
    if link.extension().is_some_and(|e| e == "cmd") {
        return std::fs::read_to_string(link).is_ok_and(|text| text == shim_text(engine));
    }
    same_file(link, engine)
}

/// The state of the terminal command for `engine`, given the PATH folders in order.
pub fn link_status(engine: &Path, link: &Path, path_dirs: &[PathBuf]) -> CliLink {
    let dir = link.parent().unwrap_or(Path::new(""));
    let linked = runs_engine(link, engine);
    let names: &[&str] = if cfg!(windows) {
        &["dial.exe", "dial.cmd"]
    } else {
        &["dial"]
    };
    let first = path_dirs
        .iter()
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|candidate| candidate.is_file());
    let other = first.filter(|found| !same_file(found, engine) && !runs_engine(found, engine));
    CliLink {
        engine: engine.to_owned(),
        link: link.to_owned(),
        linked,
        on_path: path_dirs.iter().any(|d| d == dir || same_file(d, dir)),
        other,
        blocked: !linked && !replaceable(link),
        shell: None,
    }
}

/// Point `link` at `engine`. Never replaces a file that is not Desktop's own link.
pub fn link_install(engine: &Path, link: &Path) -> Result<(), String> {
    if !crate::launcher::is_executable(engine) {
        return Err(format!("{} is not an executable file", engine.display()));
    }
    if runs_engine(link, engine) {
        return Ok(());
    }
    if !replaceable(link) {
        return Err(format!(
            "{} already exists and was not created by Dial Desktop. Remove or rename it, or choose it as the engine in Settings › Updates.",
            link.display()
        ));
    }
    let dir = link.parent().ok_or("The link has no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    if std::fs::symlink_metadata(link).is_ok() {
        std::fs::remove_file(link)
            .map_err(|e| format!("Could not replace {}: {e}", link.display()))?;
    }
    let made = if link.extension().is_some_and(|e| e == "cmd") {
        std::fs::write(link, shim_text(engine))
    } else {
        symlink(engine, link)
    };
    made.map_err(|e| format!("Could not create {}: {e}", link.display()))
}

/// Remove Desktop's link; anything else at that path is left alone.
pub fn link_remove(link: &Path) -> Result<(), String> {
    if std::fs::symlink_metadata(link).is_err() {
        return Ok(());
    }
    if !replaceable(link) {
        return Err(format!(
            "{} was not created by Dial Desktop, so it was left in place.",
            link.display()
        ));
    }
    std::fs::remove_file(link).map_err(|e| format!("Could not remove {}: {e}", link.display()))
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink(_target: &Path, _link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "symbolic links are not used on this platform",
    ))
}

// ── PATH for new terminals ───────────────────────────────────────────────────

/// The shell profile line that puts the link's folder on PATH for new terminals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellPath {
    /// The file a new terminal of the login shell reads.
    pub profile: PathBuf,
    /// The line that adds the folder.
    pub line: String,
    /// `profile` already holds `line`.
    pub present: bool,
}

/// Written above the line, so its origin is clear to whoever reads the profile later.
const PROFILE_MARK: &str = "# Added by Dial Desktop, so `dial` works in a terminal";
const POSIX_LINE: &str = "export PATH=\"$HOME/.local/bin:$PATH\"";
const FISH_LINE: &str = "fish_add_path \"$HOME/.local/bin\"";

/// The profile and line for the login shell `shell` (its path or name).
///
/// `None` on Windows (the link's folder is on PATH there already), for a folder other than
/// `~/.local/bin`, and for a shell whose profile this does not know: those are never edited.
pub fn shell_path(
    os: &str,
    shell: &str,
    home: &Path,
    zdotdir: Option<&Path>,
    dir: &Path,
) -> Option<ShellPath> {
    if os == "windows" || !home.is_absolute() || dir != home.join(".local/bin") {
        return None;
    }
    let (profile, line) = match Path::new(shell).file_name()?.to_str()? {
        "zsh" => (
            zdotdir
                .filter(|dir| dir.is_absolute())
                .unwrap_or(home)
                .join(".zshrc"),
            POSIX_LINE,
        ),
        // A macOS terminal starts a login shell, which reads only the first of these that
        // exists: creating `.bash_profile` beside a `.profile` would hide the latter.
        "bash" if os == "macos" => (
            [".bash_profile", ".bash_login", ".profile"]
                .iter()
                .map(|name| home.join(name))
                .find(|file| file.is_file())
                .unwrap_or_else(|| home.join(".bash_profile")),
            POSIX_LINE,
        ),
        "bash" => (home.join(".bashrc"), POSIX_LINE),
        "fish" => (home.join(".config/fish/config.fish"), FISH_LINE),
        "sh" | "dash" | "ksh" => (home.join(".profile"), POSIX_LINE),
        _ => return None,
    };
    let present = std::fs::read_to_string(&profile)
        .is_ok_and(|text| text.lines().any(|existing| existing.trim() == line));
    Some(ShellPath {
        profile,
        line: line.to_owned(),
        present,
    })
}

/// Append `setup.line` to its profile unless it is there already. Existing content is never
/// rewritten: the line is added at the end, after a comment naming Dial Desktop.
pub fn shell_path_add(setup: &ShellPath) -> Result<(), String> {
    use std::io::Write;
    let profile = &setup.profile;
    let existing = match std::fs::read(profile) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| format!("{} is not text, so it was left alone", profile.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("Could not read {}: {error}", profile.display())),
    };
    if existing.lines().any(|line| line.trim() == setup.line) {
        return Ok(());
    }
    if let Some(dir) = profile.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    }
    let gap = match existing.as_str() {
        "" => "",
        text if text.ends_with('\n') => "\n",
        _ => "\n\n",
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(profile)
        .map_err(|e| format!("Could not open {}: {e}", profile.display()))?;
    file.write_all(format!("{gap}{PROFILE_MARK}\n{}\n", setup.line).as_bytes())
        .map_err(|e| format!("Could not write {}: {e}", profile.display()))
}

// ── JSON reports ─────────────────────────────────────────────────────────────

const APPLY_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// The arguments of one fixed maintenance subcommand, and whether it changes anything.
/// `target` is a process id for the `ps` actions.
pub fn report_args(
    kind: &str,
    apply: bool,
    target: Option<&str>,
) -> Result<(Vec<String>, bool), String> {
    let pid = || -> Result<String, String> {
        target
            .filter(|t| !t.is_empty() && t.len() <= 10 && t.bytes().all(|b| b.is_ascii_digit()))
            .map(str::to_owned)
            .ok_or_else(|| "Choose a process first".to_owned())
    };
    let (mut args, changes): (Vec<String>, bool) = match kind {
        "gc" => (vec!["dev".into(), "gc".into()], apply),
        "images" => (vec!["images".into(), "status".into()], false),
        "images_purge" => (vec!["images".into(), "purge".into()], apply),
        "ps" => (vec!["ps".into(), "list".into()], false),
        "ps_stop" => (vec!["ps".into(), "stop".into(), pid()?], true),
        "ps_kill" => (vec!["ps".into(), "kill".into(), pid()?], true),
        "ps_logs" => (vec!["ps".into(), "logs".into(), pid()?], false),
        "ps_restart" => (vec!["ps".into(), "restart".into(), pid()?], true),
        "rules" => (vec!["rules".into(), "list".into()], false),
        // Saved project understanding and experience: read without loading a model.
        "understanding" => {
            let mut args = vec!["understanding".to_owned()];
            if let Some(query) = target.map(str::trim).filter(|q| !q.is_empty()) {
                if query.starts_with('-') || query.len() > 200 {
                    return Err("Search for words, not options".into());
                }
                args.push(query.to_owned());
            }
            (args, false)
        }
        "experience" => (vec!["experience".into()], false),
        other => return Err(format!("Unknown report `{other}`")),
    };
    args.push("--json".into());
    if apply && matches!(kind, "gc" | "images_purge") {
        args.push("--apply".into());
    }
    Ok((args, changes))
}

/// Run one maintenance subcommand and return its JSON (or its text, when it prints none).
pub fn report(
    program: &Path,
    cwd: Option<&Path>,
    kind: &str,
    apply: bool,
    target: Option<&str>,
) -> Result<serde_json::Value, String> {
    let (args, changes) = report_args(kind, apply, target)?;
    let output = run_bounded(
        dial(program, cwd).args(&args),
        if changes { APPLY_TIMEOUT } else { LIST_TIMEOUT },
    )?;
    if !output.success {
        return Err(failed(&output, &format!("dial {}", args.join(" "))));
    }
    Ok(serde_json::from_str(output.stdout.trim())
        .unwrap_or_else(|_| serde_json::Value::String(last_lines(&output.stdout, OUTPUT_TAIL))))
}

// ── Review, share and the collaboration relay ───────────────────────────────

/// What `dial review` looks at: the working tree, the changes since a branch, or one commit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum ReviewTarget {
    Uncommitted,
    Base(String),
    Commit(String),
}

/// A git ref or hash the review may name: one word that cannot be read as an option.
fn valid_ref(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 200
        && !text.starts_with('-')
        && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The fixed `dial review` arguments for `target`, writing the report to `report`.
pub fn review_args(target: &ReviewTarget, focus: &str, report: &Path) -> Result<Vec<String>, String> {
    let mut args = vec!["review".to_owned(), "--output-format".into(), "text".into()];
    match target {
        ReviewTarget::Uncommitted => args.push("--uncommitted".into()),
        ReviewTarget::Base(name) | ReviewTarget::Commit(name) => {
            if !valid_ref(name) {
                return Err(format!("`{name}` is not a branch or commit"));
            }
            let flag = if matches!(target, ReviewTarget::Base(_)) { "--base" } else { "--commit" };
            args.extend([flag.to_owned(), name.clone()]);
        }
    }
    args.extend(["--output-last-message".to_owned(), report.display().to_string()]);
    let focus = focus.trim();
    if !focus.is_empty() {
        // After `--`, focus words are never read as options.
        args.extend(["--".to_owned(), focus.to_owned()]);
    }
    Ok(args)
}

/// A review runs a model turn over the diff; long ones are a wait, not a failure.
const REVIEW_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// `dial review` in the project, read-only: the engine's review report.
pub fn review(program: &Path, cwd: &Path, target: &ReviewTarget, focus: &str) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|e| format!("Could not prepare the review: {e}"))?;
    let report = dir.path().join("review.md");
    let args = review_args(target, focus, &report)?;
    let output = run_bounded(dial(program, Some(cwd)).args(&args), REVIEW_TIMEOUT)?;
    if !output.success {
        return Err(failed(&output, "dial review"));
    }
    let text = std::fs::read_to_string(&report).unwrap_or_default();
    let text = if text.trim().is_empty() { output.stdout } else { text };
    if text.trim().is_empty() {
        return Err("Dial wrote no review".into());
    }
    Ok(text.trim().to_owned())
}

/// A link `dial share` uploaded the session to, with what was trimmed to fit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShareLink {
    pub url: String,
    /// Everything else it printed: the key warning and any trim note.
    pub notes: Vec<String>,
}

/// The `Share URL: …` line and the notes after it.
pub fn parse_share(stdout: &str) -> Result<ShareLink, String> {
    let mut url = None;
    let mut notes = Vec::new();
    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match line.strip_prefix("Share URL:") {
            Some(link) if url.is_none() => url = Some(link.trim().to_owned()),
            _ => notes.push(line.trim_start_matches("Note: ").to_owned()),
        }
    }
    match url.filter(|u| u.starts_with("https://") || u.starts_with("http://")) {
        Some(url) => Ok(ShareLink { url, notes }),
        None => {
            let said = last_lines(stdout.trim(), 600);
            Err(if said.is_empty() { "Dial did not return a link".into() } else { said })
        }
    }
}

/// Session ids are hex-ish words; nothing that could be read as an option.
fn valid_session(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

/// `dial share <session>`: an encrypted upload of the active branch, or a secret gist via `gh`.
pub fn share(program: &Path, cwd: &Path, session: &str, gist: bool) -> Result<ShareLink, String> {
    if !valid_session(session) {
        return Err("Open a saved session first".into());
    }
    let mut command = dial(program, Some(cwd));
    command.args(["share", session]);
    if gist {
        command.arg("--gist");
    }
    let output = run_bounded(&mut command, LIST_TIMEOUT)?;
    if !output.success {
        return Err(failed(&output, "dial share"));
    }
    parse_share(&output.stdout)
}

/// The collaboration relay Desktop started, kept until stopped or the app quits.
static RELAY: std::sync::Mutex<Option<(std::process::Child, String)>> = std::sync::Mutex::new(None);
const RELAY_READY: &str = "Dial collaboration relay listening on ";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelayStatus {
    /// The address it listens on, while running.
    pub bind: Option<String>,
}

fn relay_slot() -> std::sync::MutexGuard<'static, Option<(std::process::Child, String)>> {
    let mut slot = RELAY.lock().unwrap_or_else(|p| p.into_inner());
    // Forget a relay that exited on its own.
    if slot.as_mut().is_some_and(|(child, _)| !matches!(child.try_wait(), Ok(None))) {
        *slot = None;
    }
    slot
}

pub fn relay_status() -> RelayStatus {
    RelayStatus { bind: relay_slot().as_ref().map(|(_, bind)| bind.clone()) }
}

/// `dial relay --bind <addr>`, waiting until it reports it is listening.
pub fn relay_start(program: &Path, cwd: Option<&Path>, bind: &str) -> Result<RelayStatus, String> {
    use std::io::{BufRead, BufReader};
    let bind = bind.trim();
    bind.parse::<std::net::SocketAddr>()
        .map_err(|_| format!("`{bind}` is not an address like 127.0.0.1:7331"))?;
    let mut slot = relay_slot();
    if let Some((_, running)) = slot.as_ref() {
        return Ok(RelayStatus { bind: Some(running.clone()) });
    }
    let mut command = dial(program, cwd);
    command
        .args(["relay", "--bind", bind])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    let mut child = crate::launcher::without_console(&mut command)
        .spawn()
        .map_err(|e| format!("Could not start the relay: {e}"))?;
    let stderr = child.stderr.take().ok_or("The relay has no output to watch")?;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut said = String::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if line.starts_with(RELAY_READY) {
                let _ = tx.send(Ok(()));
                // Keep draining so the relay never blocks on a full pipe.
                continue;
            }
            if said.len() < 4096 {
                said.push_str(&line);
                said.push('\n');
            }
        }
        let _ = tx.send(Err(said));
    });
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => {
            *slot = Some((child, bind.to_owned()));
            Ok(RelayStatus { bind: Some(bind.to_owned()) })
        }
        outcome => {
            let _ = child.kill();
            let _ = child.wait();
            Err(match outcome {
                Ok(Err(said)) if !said.trim().is_empty() => format!("The relay stopped: {}", said.trim()),
                Ok(Err(_)) => "The relay stopped before listening".into(),
                _ => "The relay did not report that it is listening".into(),
            })
        }
    }
}

/// Stop the relay Desktop started; also called when the app exits.
pub fn relay_stop() -> RelayStatus {
    if let Some((mut child, _)) = relay_slot().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    RelayStatus { bind: None }
}
