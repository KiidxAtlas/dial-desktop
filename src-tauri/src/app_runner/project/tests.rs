use super::iso::tests::read_volume;
use super::*;

use std::collections::HashMap;
use std::ffi::OsString;
use std::time::{SystemTime, UNIX_EPOCH};

const OID: &str = "0123456789abcdef0123456789abcdef01234567";

/// A directory under the system temp dir, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("dial-project-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", NULL_DEVICE)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

fn init_repo(dir: &Path) {
    git(dir, &["init", "-q"]);
}

fn commit_all(dir: &Path) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "snapshot"]);
}

fn head_sha(dir: &Path) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", NULL_DEVICE)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git runs");
    String::from_utf8(output.stdout)
        .expect("utf-8")
        .trim()
        .to_owned()
}

fn write(dir: &Path, rel: &str, contents: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
    fs::write(path, contents).expect("write file");
}

fn entries_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("dir")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn snapshot_carries_only_committed_tracked_files() {
    let repo = Scratch::new("committed");
    init_repo(repo.path());
    write(repo.path(), "a.txt", "committed");
    write(repo.path(), "src/main.rs", "fn main() {}");
    write(repo.path(), ".gitignore", "ignored.log\n");
    commit_all(repo.path());
    // Uncommitted, untracked and ignored content must not reach the guest.
    write(repo.path(), "a.txt", "edited but not committed");
    write(repo.path(), "secret.env", "TOKEN=1");
    write(repo.path(), "ignored.log", "log");

    let private = Scratch::new("private-committed");
    let iso = build_snapshot(repo.path(), private.path()).expect("snapshot");
    let image = fs::read(&iso).expect("image");
    let files = read_volume(&image, true).expect("joliet");
    let names: Vec<&str> = files.keys().map(String::as_str).collect();
    assert_eq!(names, [".gitignore", "a.txt", "src/main.rs"]);
    assert_eq!(files["a.txt"], b"committed");
}

#[test]
fn committed_recipe_adds_only_dials_launcher_and_refuses_collisions() {
    let repo = Scratch::new("recipe");
    let private = Scratch::new("recipe-out");
    init_repo(repo.path());
    write(repo.path(), RECIPE, "{}");
    commit_all(repo.path());
    let iso = build_snapshot(repo.path(), private.path()).expect("recipe disc");
    let files = read_volume(&fs::read(iso).expect("image"), true).expect("joliet");
    assert_eq!(files.len(), 2);
    assert_eq!(files[LAUNCHER], LAUNCHER_BYTES);
    assert_eq!(files[RECIPE], b"{}");
    write(repo.path(), "dial-launch.PY/file", "not the launcher");
    commit_all(repo.path());
    let error = build_snapshot(repo.path(), private.path()).expect_err("reserved launcher path");
    assert!(error.starts_with("reserved_name:"), "{error}");
}

#[cfg(unix)]
#[test]
fn tracked_symlinks_are_left_out_not_followed() {
    let repo = Scratch::new("symlink");
    init_repo(repo.path());
    write(repo.path(), "a.txt", "real");
    std::os::unix::fs::symlink("/etc/passwd", repo.path().join("link")).expect("symlink");
    commit_all(repo.path());

    let private = Scratch::new("private-symlink");
    let iso = build_snapshot(repo.path(), private.path()).expect("snapshot");
    let files = read_volume(&fs::read(iso).expect("image"), true).expect("joliet");
    assert_eq!(
        files.keys().map(String::as_str).collect::<Vec<_>>(),
        ["a.txt"]
    );
}

#[test]
fn refuses_roots_that_are_not_a_committed_repository_root() {
    let unborn = Scratch::new("unborn");
    init_repo(unborn.path());
    write(unborn.path(), "a.txt", "x");

    let repo = Scratch::new("subdir");
    init_repo(repo.path());
    write(repo.path(), "sub/a.txt", "x");
    commit_all(repo.path());

    let plain = Scratch::new("plain");
    write(plain.path(), "a.txt", "x");

    let private = Scratch::new("refused-private");
    // A folder outside any repository is `not_a_repo`. One inside another repository (for example
    // under a checkout's temp dir) is refused as not the repository root. Both are refusals.
    let cases: [(PathBuf, &[&str]); 3] = [
        (unborn.path().to_path_buf(), &["unborn_head"]),
        (repo.path().join("sub"), &["not_repo_root"]),
        (plain.path().to_path_buf(), &["not_a_repo", "not_repo_root"]),
    ];
    for (root, codes) in cases {
        let err = build_snapshot(&root, private.path()).expect_err("refused");
        assert!(
            codes.iter().any(|code| err.starts_with(code)),
            "{}: {err}",
            root.display()
        );
    }
    assert!(
        entries_in(private.path()).is_empty(),
        "a refused snapshot must leave nothing behind"
    );
}

#[test]
fn refuses_a_private_dir_inside_the_workspace() {
    let repo = Scratch::new("inside");
    init_repo(repo.path());
    write(repo.path(), "a.txt", "x");
    commit_all(repo.path());
    let inside = repo.path().join("private");
    fs::create_dir_all(&inside).expect("dir");
    let err = build_snapshot(repo.path(), &inside).expect_err("inside");
    assert!(err.starts_with("private_dir_inside_workspace"), "{err}");
}

#[test]
fn output_is_a_read_only_iso_in_the_private_dir() {
    let repo = Scratch::new("output");
    init_repo(repo.path());
    write(repo.path(), "a.txt", "x");
    commit_all(repo.path());
    let private = Scratch::new("private-output");
    let iso = build_snapshot(repo.path(), private.path()).expect("snapshot");

    assert_eq!(
        iso.parent(),
        Some(fs::canonicalize(private.path()).expect("canon").as_path())
    );
    let name = iso.file_name().and_then(|n| n.to_str()).expect("name");
    assert!(
        name.starts_with("project-") && name.ends_with(".iso"),
        "{name}"
    );
    assert!(fs::metadata(&iso).expect("meta").permissions().readonly());
    let image = fs::read(&iso).expect("image");
    assert_eq!(&image[16 * 2048 + 1..16 * 2048 + 6], b"CD001");
    assert!(
        entries_in(private.path())
            .iter()
            .all(|n| !n.ends_with(".partial")),
        "partial image left behind"
    );
}

#[test]
fn snapshot_limits_are_enforced_on_bytes_and_file_count() {
    let repo = Scratch::new("limits");
    init_repo(repo.path());
    write(repo.path(), "a.txt", "0123456789");
    write(repo.path(), "b.txt", "x");
    commit_all(repo.path());
    // (limits, the refusal code, or None when the snapshot must be written)
    let cases: [(Limits, Option<&str>); 4] = [
        (
            Limits {
                bytes: 3,
                files: MAX_SNAPSHOT_FILES,
            },
            Some("snapshot_too_large"),
        ),
        (
            Limits {
                bytes: 10,
                files: MAX_SNAPSHOT_FILES,
            },
            Some("snapshot_too_large"),
        ),
        (
            Limits {
                bytes: 11,
                files: 1,
            },
            Some("too_many_files"),
        ),
        (
            Limits {
                bytes: 11,
                files: 2,
            },
            None,
        ),
    ];
    for (limits, code) in cases {
        let private = Scratch::new("private-limits");
        let result = build_snapshot_with(repo.path(), private.path(), limits);
        match code {
            Some(code) => {
                let err = result.expect_err("refused");
                assert!(err.starts_with(code), "{limits:?}: {err}");
                assert!(
                    entries_in(private.path()).is_empty(),
                    "{limits:?}: nothing left behind"
                );
            }
            None => {
                result.expect("exactly at the limit is allowed");
            }
        }
    }
}

#[test]
fn listing_is_parsed_with_its_limits_and_refusals() {
    let blob = |path: &str, size: &str| format!("100644 blob {OID} {size:>7}\t{path}\0");
    let long_path = "p".repeat(MAX_PATH_BYTES + 1);
    // (listing, max entries, expected blob count, or the refusal code)
    let cases: [(Vec<u8>, usize, Result<usize, &str>); 8] = [
        (blob("README", "5").into_bytes(), 10, Ok(1)),
        (
            format!("120000 blob {OID} {:>7}\tlink\0", 6).into_bytes(),
            10,
            Ok(0),
        ),
        (
            format!("160000 commit {OID} {:>7}\tvendor/sub\0", "-").into_bytes(),
            10,
            Ok(0),
        ),
        (
            format!("040000 tree {OID} {:>7}\tdir\0", "-").into_bytes(),
            10,
            Err("unsupported_entry"),
        ),
        (b"100644 blob\0".to_vec(), 10, Err("git_failed")),
        (
            b"100644 blob xyz 5\tREADME\0".to_vec(),
            10,
            Err("git_failed"),
        ),
        (
            (blob("a", "1") + &blob("b", "1")).into_bytes(),
            1,
            Err("too_many_files"),
        ),
        (blob(&long_path, "1").into_bytes(), 10, Err("path_too_long")),
    ];
    for (listing, max, expected) in cases {
        let got = parse_listing(&listing, max);
        match expected {
            Ok(count) => assert_eq!(got.map(|b| b.len()), Ok(count), "{listing:?}"),
            Err(code) => {
                let err = got.expect_err("refused");
                assert!(err.starts_with(code), "{listing:?}: {err}");
            }
        }
    }
    let non_utf8 = b"100644 blob 0123456789abcdef0123456789abcdef01234567       1\t\xff\0";
    let err = parse_listing(non_utf8, 10).expect_err("non-UTF-8 path");
    assert!(err.starts_with("unsupported_name"), "{err}");
}

#[test]
fn stale_partial_is_refused_and_left_exactly_as_it_was() {
    let repo = Scratch::new("stale");
    init_repo(repo.path());
    write(repo.path(), "a.txt", "x");
    commit_all(repo.path());
    let private = Scratch::new("private-stale");
    let partial = private.path().join(format!(
        "project-{}.iso.partial",
        &head_sha(repo.path())[..12]
    ));
    fs::write(&partial, "someone else's work").expect("stale partial");

    let err = build_snapshot(repo.path(), private.path()).expect_err("partial exists");
    assert!(err.starts_with("partial_exists"), "{err}");
    assert_eq!(
        fs::read(&partial).expect("still there"),
        b"someone else's work"
    );
}

#[cfg(unix)]
#[test]
fn a_failed_write_removes_only_the_partial_it_created() {
    // A name Joliet cannot hold makes the write fail after the partial exists.
    let repo = Scratch::new("bad-name");
    init_repo(repo.path());
    write(repo.path(), "bad:name", "x");
    commit_all(repo.path());
    let private = Scratch::new("private-bad-name");
    let err = build_snapshot(repo.path(), private.path()).expect_err("unsupported name");
    assert!(err.starts_with("unsupported_name"), "{err}");
    assert!(
        entries_in(private.path()).is_empty(),
        "the partial must be removed"
    );
}

#[test]
fn git_runs_from_a_private_directory_with_the_repository_selected_by_dash_c() {
    let git = Git {
        exe: PathBuf::from(GIT_EXE),
        cwd: PathBuf::from("/private"),
        root: PathBuf::from("/repo"),
    };
    let command = git.command();
    assert_eq!(command.get_current_dir(), Some(Path::new("/private")));
    let args: Vec<&OsStr> = command.get_args().collect();
    let expected = ["-c", "protocol.allow=never", "-C", "/repo"];
    assert_eq!(args, expected.iter().map(OsStr::new).collect::<Vec<_>>());
    let envs: HashMap<&OsStr, Option<&OsStr>> = command.get_envs().collect();
    assert_eq!(
        envs.get(OsStr::new("GIT_ALLOW_PROTOCOL")),
        Some(&Some(OsStr::new("")))
    );
    assert_eq!(
        envs.get(OsStr::new("GIT_NO_LAZY_FETCH")),
        Some(&Some(OsStr::new("1")))
    );
    assert_eq!(
        envs.get(OsStr::new("GIT_CONFIG_GLOBAL")),
        Some(&Some(OsStr::new(NULL_DEVICE)))
    );
}

#[test]
fn git_is_only_taken_from_an_absolute_path_outside_the_workspace() {
    let workspace = Scratch::new("ws");
    let outside = Scratch::new("outside");
    write(workspace.path(), &format!("tools/{GIT_EXE}"), "");
    write(outside.path(), GIT_EXE, "");
    let ws = fs::canonicalize(workspace.path()).expect("canon");
    let inside_bin = ws.join("tools");
    let outside_bin = fs::canonicalize(outside.path()).expect("canon");
    let join = |dirs: &[&Path]| std::env::join_paths(dirs).expect("join");

    let cases: [(OsString, Option<PathBuf>); 4] = [
        (join(&[&inside_bin]), None),
        (
            join(&[&inside_bin, &outside_bin]),
            Some(outside_bin.join(GIT_EXE)),
        ),
        (join(&[&outside_bin]), Some(outside_bin.join(GIT_EXE))),
        (OsString::new(), None),
    ];
    for (path_var, expected) in cases {
        assert_eq!(find_git(&path_var, &ws), expected, "{path_var:?}");
    }
}
