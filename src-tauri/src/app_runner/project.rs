//! The project snapshot a guest may see: one committed Git tree, written as a read-only ISO image.
//!
//! [`build_snapshot`] is the only entry point. It reads committed blobs through `git cat-file`,
//! never the working tree, so uncommitted edits, untracked files and ignored files stay on the
//! host. Symlinks and submodules are left out rather than followed.
//! A committed `.dial/app.json` also includes Dial's guest-only recipe reader on the disc.
//! Attaching the ISO never executes it; the user or agent invokes it inside the guest.
//!
//! Git is run as an absolute executable resolved from `PATH` (outside the workspace), with a
//! private directory as its working directory and the repository selected with `-C`. A workspace
//! file can therefore never be picked up as the `git` command, least of all on Windows, where a
//! bare name is searched for in the current directory. Git's configuration is ignored, replace
//! refs and lazy fetch are off, and every transport is refused, so a repository cannot steer the
//! export through filters, hooks or a remote. Missing Git, an unborn HEAD and a root that is not
//! the repository root are explicit errors, with no fallback to copying files.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

mod iso;

#[cfg(test)]
mod tests;

/// Largest committed content a snapshot will export. Larger trees are refused, not truncated.
pub const MAX_SNAPSHOT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Most tracked entries a snapshot will list. Symlinks and submodules count too, so a repository
/// full of them cannot push the listing past the bound.
pub const MAX_SNAPSHOT_FILES: usize = 100_000;
/// Longest repository path Git may report, in bytes. The ISO writer applies its stricter limits.
pub const MAX_PATH_BYTES: usize = 1024;

/// The bounds one snapshot is checked against. Tests pass smaller ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Limits {
    bytes: u64,
    files: usize,
}

const DEFAULT_LIMITS: Limits = Limits {
    bytes: MAX_SNAPSHOT_BYTES,
    files: MAX_SNAPSHOT_FILES,
};

const GIT_EXE: &str = if cfg!(windows) { "git.exe" } else { "git" };
const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

const RECIPE: &str = ".dial/app.json";
const LAUNCHER: &str = "DIAL-LAUNCH.py";
const LAUNCHER_BYTES: &[u8] = include_bytes!("../../../packaging/guest/launch.py");

/// Exports the committed `HEAD` of the repository at `root` into `private_dir` as a read-only ISO
/// image and returns its path. Errors are `code: detail` strings with a stable code first:
/// `no_git`, `not_a_repo`, `not_repo_root`, `unborn_head`, `private_dir_inside_workspace`,
/// `partial_exists`, `snapshot_too_large`, `too_many_files`, `path_too_long`, `unsupported_name`,
/// `too_deep`, `too_many_directories`, `image_too_large`, `io_failed`, and `git_failed` for
/// anything else.
///
/// Nothing is executed: the image is only data for the guest.
pub fn build_snapshot(root: &Path, private_dir: &Path) -> Result<PathBuf, String> {
    build_snapshot_with(root, private_dir, DEFAULT_LIMITS)
}

fn build_snapshot_with(root: &Path, private_dir: &Path, limits: Limits) -> Result<PathBuf, String> {
    let root = fs::canonicalize(root).map_err(|e| format!("workspace_unreadable: {e}"))?;
    if !root.is_dir() {
        return Err(format!(
            "workspace_unreadable: {} is not a directory",
            root.display()
        ));
    }
    let private_dir =
        fs::canonicalize(private_dir).map_err(|e| format!("private_dir_unreadable: {e}"))?;
    if !private_dir.is_dir() {
        return Err(format!(
            "private_dir_unreadable: {} is not a directory",
            private_dir.display()
        ));
    }
    // The image must never contain itself, nor be written where the user can browse the copy.
    if private_dir.starts_with(&root) {
        return Err(format!(
            "private_dir_inside_workspace: {}",
            private_dir.display()
        ));
    }

    let git = Git {
        exe: resolve_git(&root)?,
        cwd: private_dir.clone(),
        root: strip_verbatim(&root),
    };

    let toplevel = git
        .output(&["rev-parse", "--show-toplevel"])
        .map_err(|e| classify(e, "not_a_repo"))?;
    let toplevel = fs::canonicalize(toplevel.trim_end_matches(['\r', '\n']))
        .map_err(|e| format!("not_a_repo: {e}"))?;
    if toplevel != root {
        return Err(format!(
            "not_repo_root: the snapshot must be the repository root, which is {}",
            toplevel.display()
        ));
    }

    let head = git
        .output(&["rev-parse", "--verify", "-q", "HEAD^{commit}"])
        .map_err(|e| classify(e, "unborn_head"))?;
    let commit = head.trim().to_owned();
    if !is_object_id(&commit) {
        return Err(format!("git_failed: unexpected commit id {commit:?}"));
    }

    let listing = git.bytes(&["ls-tree", "-r", "-l", "-z", "--full-tree", &commit])?;
    let blobs = parse_listing(&listing, limits.files)?;
    let total = blobs
        .iter()
        .fold(0u64, |sum, blob| sum.saturating_add(blob.size));
    if total > limits.bytes {
        return Err(format!(
            "snapshot_too_large: {total} bytes of committed content exceed the {} byte limit",
            limits.bytes
        ));
    }

    let name = format!("project-{}.iso", &commit[..12]);
    let target = private_dir.join(&name);
    let partial = private_dir.join(format!("{name}.partial"));
    // A partial that already exists is not ours to remove: it may be a leftover from a crash, or
    // another process's work. It is refused, and left exactly as it is.
    let file = create_partial(&partial)?;
    match write_snapshot(&git, &blobs, file).and_then(|()| publish(&partial, &target)) {
        Ok(()) => Ok(target),
        Err(error) => {
            // This process created the partial, so removing it on failure is safe.
            let _ = fs::remove_file(&partial);
            Err(error)
        }
    }
}

fn is_object_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// One committed regular file: its path, object id and size as Git reports them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Blob {
    path: String,
    oid: String,
    size: u64,
}

/// Reads `git ls-tree -r -l -z` output. Symlinks and submodules are skipped; any other
/// non-regular entry is refused. Paths must be UTF-8 because the ISO names are derived from them.
/// Fails with `too_many_files` once more than `max_files` entries are listed.
fn parse_listing(listing: &[u8], max_files: usize) -> Result<Vec<Blob>, String> {
    let mut blobs = Vec::new();
    let mut entries = 0usize;
    for record in listing.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        entries += 1;
        if entries > max_files {
            return Err(format!(
                "too_many_files: more than {max_files} tracked entries"
            ));
        }
        let Some(tab) = record.iter().position(|b| *b == b'\t') else {
            return Err("git_failed: malformed ls-tree record".to_owned());
        };
        let meta = std::str::from_utf8(&record[..tab])
            .map_err(|_| "git_failed: malformed ls-tree record".to_owned())?;
        let mut fields = meta.split_whitespace();
        let (Some(mode), Some(kind), Some(oid), Some(size)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return Err("git_failed: malformed ls-tree record".to_owned());
        };
        if !is_object_id(oid) {
            return Err(format!("git_failed: unexpected object id {oid:?}"));
        }
        match (mode, kind) {
            // A symlink's target is not project content, and following it would leave the repo.
            ("120000", "blob") => continue,
            // A submodule is another repository and is not part of this one's tree.
            (_, "commit") => continue,
            (m, "blob") if m.starts_with("100") => {}
            _ => return Err(format!("unsupported_entry: mode {mode} type {kind}")),
        }
        let raw_path = &record[tab + 1..];
        if raw_path.len() > MAX_PATH_BYTES {
            return Err(format!("path_too_long: {} bytes", raw_path.len()));
        }
        let path = std::str::from_utf8(raw_path)
            .map_err(|_| format!("unsupported_name: {}", String::from_utf8_lossy(raw_path)))?;
        let size: u64 = size
            .parse()
            .map_err(|_| format!("git_failed: bad size for {path}"))?;
        blobs.push(Blob {
            path: path.to_owned(),
            oid: oid.to_owned(),
            size,
        });
    }
    Ok(blobs)
}

fn write_snapshot(git: &Git, blobs: &[Blob], file: File) -> Result<(), String> {
    let mut out = BufWriter::new(file);
    let mut files: Vec<iso::ImageFile> = blobs
        .iter()
        .map(|blob| iso::ImageFile {
            path: blob.path.clone(),
            size: blob.size,
        })
        .collect();
    if blobs.iter().any(|b| b.path == RECIPE) {
        if blobs.iter().any(|b| {
            b.path
                .split('/')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case(LAUNCHER))
        }) {
            return Err(format!(
                "reserved_name: a project with {RECIPE} must leave {LAUNCHER} free"
            ));
        }
        files.push(iso::ImageFile {
            path: LAUNCHER.to_owned(),
            size: LAUNCHER_BYTES.len() as u64,
        });
    }
    let mut reader = BlobReader::spawn(git)?;
    let mut content = |index: usize, w: &mut dyn Write| match blobs.get(index) {
        Some(blob) => reader.read(&blob.oid, blob.size, w),
        None => w.write_all(LAUNCHER_BYTES).map_err(io_failed),
    };
    iso::write_image(&files, &mut content, &mut out)?;
    reader.finish()?;
    let file = out.into_inner().map_err(|e| io_failed(e.into_error()))?;
    file.sync_all().map_err(io_failed)?;
    let mut permissions = file.metadata().map_err(io_failed)?.permissions();
    permissions.set_readonly(true);
    file.set_permissions(permissions).map_err(io_failed)?;
    Ok(())
}

/// Streams blobs through one `git cat-file --batch`, in the order the image needs them.
struct BlobReader {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl BlobReader {
    fn spawn(git: &Git) -> Result<Self, String> {
        let mut child = git
            .command()
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(spawn_error)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "git_failed: no cat-file stdin".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "git_failed: no cat-file stdout".to_owned())?;
        Ok(BlobReader {
            child: Some(child),
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
        })
    }

    fn read(&mut self, oid: &str, size: u64, out: &mut dyn Write) -> Result<(), String> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| "git_failed: cat-file already finished".to_owned())?;
        writeln!(stdin, "{oid}")
            .and_then(|()| stdin.flush())
            .map_err(io_failed)?;
        let mut header = String::new();
        self.stdout.read_line(&mut header).map_err(io_failed)?;
        let fields: Vec<&str> = header.split_whitespace().collect();
        match fields.as_slice() {
            [_, "blob", len] if len.parse::<u64>() == Ok(size) => {}
            [_, "blob", _] => return Err(format!("object_size_changed: {oid}")),
            [_, "missing"] => return Err(format!("missing_object: {oid}")),
            _ => return Err(format!("git_failed: unexpected cat-file header {header:?}")),
        }
        let copied = io::copy(&mut (&mut self.stdout).take(size), out).map_err(io_failed)?;
        if copied != size {
            return Err(format!("object_truncated: {oid}"));
        }
        let mut newline = [0u8; 1];
        self.stdout.read_exact(&mut newline).map_err(io_failed)?;
        Ok(())
    }

    fn finish(mut self) -> Result<(), String> {
        drop(self.stdin.take());
        let Some(mut child) = self.child.take() else {
            return Err("git_failed: cat-file already finished".to_owned());
        };
        let status = child.wait().map_err(io_failed)?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("git_failed: cat-file exited with {status}"))
        }
    }
}

impl Drop for BlobReader {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// One Git executable, run against one repository from one private working directory.
struct Git {
    /// Absolute path of the `git` executable, resolved by [`resolve_git`].
    exe: PathBuf,
    /// The child's working directory: a directory this process owns, never the workspace.
    cwd: PathBuf,
    /// The repository, passed with `-C`.
    root: PathBuf,
}

impl Git {
    /// A command for one plumbing call, with the environment cleared and only what Git needs
    /// passed back in.
    fn command(&self) -> Command {
        let mut command = Command::new(&self.exe);
        crate::launcher::without_console(&mut command).env_clear();
        for key in ["PATH", "SYSTEMROOT", "TMPDIR", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", NULL_DEVICE)
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            // Older Git ignores GIT_NO_LAZY_FETCH. An empty allow-list refuses every transport,
            // so a partial clone cannot reach a promisor remote on those versions either.
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_ALLOW_PROTOCOL", "")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .current_dir(&self.cwd)
            .args(["-c", "protocol.allow=never", "-C"])
            .arg(&self.root);
        command
    }

    fn output(&self, args: &[&str]) -> Result<String, String> {
        String::from_utf8(self.bytes(args)?).map_err(|_| "git_failed: non-UTF-8 output".to_owned())
    }

    fn bytes(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let output = self
            .command()
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(spawn_error)?;
        if !output.status.success() {
            return Err(format!(
                "git_failed: git {}: {}",
                args.first().copied().unwrap_or(""),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(output.stdout)
    }
}

/// The `git` executable, as an absolute path from `PATH`. Refused as `no_git` when there is none.
fn resolve_git(root: &Path) -> Result<PathBuf, String> {
    std::env::var_os("PATH")
        .and_then(|path| find_git(&path, root))
        .ok_or_else(|| "no_git: git was not found on PATH".to_owned())
}

/// The first `PATH` entry that holds `git` outside `root`. Relative entries are skipped, because
/// they resolve against the current directory, and that is how a checkout's own files could be
/// run. A candidate is resolved before the check, so a symlink into the workspace is refused too.
fn find_git(path_var: &OsStr, root: &Path) -> Option<PathBuf> {
    std::env::split_paths(path_var)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(GIT_EXE))
        .filter_map(|candidate| fs::canonicalize(candidate).ok())
        .find(|candidate| !candidate.starts_with(root) && candidate.is_file())
}

/// Windows `canonicalize` returns `\\?\` paths, which Git does not accept as `-C` arguments.
#[cfg(windows)]
fn strip_verbatim(path: &Path) -> PathBuf {
    match path.to_str().and_then(|text| text.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with(r"UNC\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

#[cfg(not(windows))]
fn strip_verbatim(path: &Path) -> PathBuf {
    path.to_path_buf()
}

/// Keeps `no_git` as it is and gives every other Git failure the caller's code.
fn classify(error: String, code: &str) -> String {
    if error.starts_with("no_git") {
        error
    } else {
        format!("{code}: {error}")
    }
}

fn spawn_error(error: io::Error) -> String {
    if error.kind() == io::ErrorKind::NotFound {
        "no_git: git was not found on PATH".to_owned()
    } else {
        format!("git_failed: {error}")
    }
}

fn io_failed(error: io::Error) -> String {
    format!("io_failed: {error}")
}

/// Creates the partial image, failing if anything is already at that path. Nothing is removed
/// here: a failure to create leaves the existing file exactly as it was.
fn create_partial(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    set_private_mode(&mut options);
    options.open(path).map_err(|e| {
        if e.kind() == io::ErrorKind::AlreadyExists {
            format!(
                "partial_exists: {} is left over from an earlier run and was not touched",
                path.display()
            )
        } else {
            io_failed(e)
        }
    })
}

#[cfg(unix)]
fn set_private_mode(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_private_mode(_options: &mut OpenOptions) {}

/// Moves the finished image into place.
fn publish(partial: &Path, target: &Path) -> Result<(), String> {
    unlock_for_replace(target)?;
    fs::rename(partial, target).map_err(io_failed)
}

/// Windows refuses to replace a read-only file, so an earlier copy is made writable first.
#[cfg(windows)]
fn unlock_for_replace(target: &Path) -> Result<(), String> {
    if let Ok(metadata) = fs::metadata(target) {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(target, permissions).map_err(io_failed)?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn unlock_for_replace(_target: &Path) -> Result<(), String> {
    Ok(())
}
