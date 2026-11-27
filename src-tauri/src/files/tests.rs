use super::*;

/// A fresh folder for one test, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("dial-desktop-files-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir.canonicalize().unwrap())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn paths_stay_inside_the_workspace() {
    let root = Scratch::new("resolve");
    fs::create_dir_all(root.0.join("src")).unwrap();
    fs::write(root.0.join("src/main.rs"), "fn main() {}").unwrap();

    assert_eq!(
        resolve(&root.0, "src/main.rs").unwrap(),
        root.0.join("src/main.rs")
    );
    let absolute = root.0.join("src/main.rs");
    assert_eq!(
        resolve(&root.0, &absolute.to_string_lossy()).unwrap(),
        absolute
    );
    assert!(
        resolve(&root.0, "../etc/passwd")
            .unwrap_err()
            .contains("outside")
    );
    assert!(
        resolve(&root.0, "src/../../x")
            .unwrap_err()
            .contains("outside")
    );
    assert!(
        resolve(&root.0, "/etc/hosts")
            .unwrap_err()
            .contains("outside")
    );
    assert!(
        resolve(&root.0, "missing.txt")
            .unwrap_err()
            .starts_with("Cannot open")
    );
}

#[cfg(unix)]
#[test]
fn symlinks_out_of_the_workspace_are_refused() {
    let root = Scratch::new("symlink");
    let outside = Scratch::new("symlink-target");
    fs::write(outside.0.join("secret"), "x").unwrap();
    std::os::unix::fs::symlink(outside.0.join("secret"), root.0.join("link")).unwrap();
    assert!(resolve(&root.0, "link").unwrap_err().contains("outside"));
    assert!(read(&root.0, "link").is_err());
    std::os::unix::fs::symlink(&root.0, root.0.join("loop")).unwrap();
    assert!(!list(&root.0, "").unwrap().into_iter().find(|e| e.name == "loop").unwrap().dir);
    assert!(list(&root.0, "loop").is_err(), "a remembered expanded directory link must not be loaded");
}

#[cfg(windows)]
#[test]
fn directory_junctions_are_never_expanded() {
    let root = Scratch::new("junction");
    // Unlike a Windows symlink, a directory junction does not need Developer Mode/elevation.
    let out = Command::new("cmd.exe").current_dir(&root.0)
        .args(["/d", "/c", "mklink", "/j", "loop", "."]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(!list(&root.0, "").unwrap().into_iter().find(|e| e.name == "loop").unwrap().dir);
    assert!(list(&root.0, "loop").is_err());
    fs::remove_dir(root.0.join("loop")).unwrap();
}

#[test]
fn listing_puts_folders_first_and_hides_git() {
    let root = Scratch::new("list");
    for dir in [".git", "zeta", "Alpha"] {
        fs::create_dir_all(root.0.join(dir)).unwrap();
    }
    fs::write(root.0.join("b.txt"), "").unwrap();
    fs::write(root.0.join("A.md"), "").unwrap();
    fs::write(root.0.join("zeta/inner.rs"), "").unwrap();

    let top = list(&root.0, "").unwrap();
    let names: Vec<_> = top.iter().map(|e| (e.name.as_str(), e.dir)).collect();
    assert_eq!(
        names,
        [
            ("Alpha", true),
            ("zeta", true),
            ("A.md", false),
            ("b.txt", false)
        ]
    );
    let inner = list(&root.0, "zeta").unwrap();
    assert_eq!(inner[0].path, "zeta/inner.rs");
    assert!(list(&root.0, "..").is_err());
}

#[test]
fn reading_gives_text_or_marks_binary() {
    let root = Scratch::new("read");
    fs::write(root.0.join("hello.txt"), "héllo").unwrap();
    fs::write(root.0.join("blob.bin"), [0u8, 1, 2, 3]).unwrap();

    let text = read(&root.0, "hello.txt").unwrap();
    assert_eq!(text.text.as_deref(), Some("héllo"));
    assert_eq!(text.path, "hello.txt");
    assert!(!text.truncated);
    assert_eq!(read(&root.0, "blob.bin").unwrap().text, None);
    assert!(read(&root.0, ".").unwrap_err().contains("folder"));
}

#[test]
fn a_truncated_read_drops_a_split_character() {
    // "é" is two bytes; a read that stopped after the first must not become binary.
    assert_eq!(decode_text(b"ab\xc3", true).as_deref(), Some("ab"));
    assert_eq!(decode_text(b"ab\xc3", false), None);
    assert_eq!(decode_text(b"a\xffb", true), None);
}

#[test]
fn diffs_show_tracked_and_untracked_changes() {
    let root = Scratch::new("diff");
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(&root.0)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(status.status.success(), "{:?}", status);
    };
    assert!(diff(&root.0, ".").unwrap_err().contains("folder"));
    git(&["init", "-q"]);
    fs::write(root.0.join("a.txt"), "one\n").unwrap();
    git(&["add", "a.txt"]);
    fs::write(root.0.join("a.txt"), "unstaged\n").unwrap();
    let against_index = diff(&root.0, "a.txt").unwrap();
    assert_eq!(against_index.base, DiffBase::Index);
    assert!(against_index.diff.contains("+unstaged"));
    fs::write(root.0.join("a.txt"), "one\n").unwrap();
    git(&["commit", "-qm", "init"]);
    fs::write(root.0.join("a.txt"), "two\n").unwrap();
    fs::write(root.0.join("new.txt"), "fresh\n").unwrap();

    let changed = diff(&root.0, "a.txt").unwrap();
    assert_eq!(changed.base, DiffBase::Head);
    assert!(
        changed.diff.contains("-one") && changed.diff.contains("+two"),
        "{}",
        changed.diff
    );

    let added = diff(&root.0, &root.0.join("new.txt").to_string_lossy()).unwrap();
    assert_eq!(added.base, DiffBase::Untracked);
    assert_eq!(added.path, "new.txt");
    assert!(added.diff.contains("+fresh"), "{}", added.diff);

    fs::write(root.0.join("a.txt"), "one\n").unwrap();
    assert_eq!(diff(&root.0, "a.txt").unwrap().diff, "");
}

#[test]
fn git_operations_select_only_the_named_file() {
    let root = Scratch::new("literal");
    git_ok(&root.0, &["init", "-q"]);
    let selected = "[ab].txt";
    let sibling = "a.txt";
    for path in [selected, sibling] { fs::write(root.0.join(path), "original\n").unwrap(); }
    git_ok(&root.0, &["add", "."]);
    git_ok(&root.0, &["commit", "-qm", "init"]);
    for path in [selected, sibling] { fs::write(root.0.join(path), format!("modified {path}\n")).unwrap(); }
    let shown = diff(&root.0, selected).unwrap();
    assert!(shown.diff.contains("+modified [ab].txt"));
    assert!(!shown.diff.contains("+modified a.txt"));
    stage(&root.0, &[selected.into()], true).unwrap();
    assert_eq!(commit_review(&root.0).unwrap().paths, [selected]);
    revert(&root.0, selected).unwrap();
    assert_eq!(fs::read_to_string(root.0.join(selected)).unwrap(), "original\n");
    // This alternate spelling can resolve to the sibling on case-insensitive filesystems,
    // but it isn't a distinct untracked entry and must never be deleted as one.
    assert!(revert(&root.0, "A.TXT").is_err());
    assert_eq!(fs::read_to_string(root.0.join(sibling)).unwrap(), "modified a.txt\n");
}

#[cfg(unix)]
#[test]
fn actions_on_links_do_not_target_the_worktree_file() {
    use std::os::unix::fs::symlink;
    let root = Scratch::new("git-link");
    git_ok(&root.0, &["init", "-q"]);
    fs::write(root.0.join("target"), "original\n").unwrap();
    git_ok(&root.0, &["add", "."]);
    git_ok(&root.0, &["commit", "-qm", "init"]);
    fs::write(root.0.join("target"), "edited\n").unwrap();
    symlink("target", root.0.join("link")).unwrap();
    let path = root.0.join("link");
    let name = path.to_str().unwrap();
    assert_eq!(read(&root.0, name).unwrap().path, "link");
    let shown = diff(&root.0, name).unwrap();
    assert_eq!(shown.path, "link");
    assert_eq!(shown.base, DiffBase::Untracked);
    assert!(shown.diff.contains("+target"));
    stage(&root.0, &[name.into()], true).unwrap();
    assert_eq!(commit_review(&root.0).unwrap().paths, ["link"]);
    assert!(diff(&root.0, name).unwrap().delete_on_revert);
    revert(&root.0, name).unwrap();
    assert!(!path.exists());
    assert_eq!(fs::read_to_string(root.0.join("target")).unwrap(), "edited\n");
}

#[test]
fn unreadable_head_never_turns_restore_into_delete() {
    let root = Scratch::new("revert-damaged");
    git_ok(&root.0, &["init", "-q"]);
    fs::write(root.0.join("tracked"), "original\n").unwrap();
    git_ok(&root.0, &["add", "."]);
    git_ok(&root.0, &["commit", "-qm", "init"]);
    fs::write(root.0.join("tracked"), "edited\n").unwrap();
    // Keep a valid HEAD ref but remove its object database.
    fs::rename(root.0.join(".git/objects"), root.0.join("objects-backup")).unwrap();
    fs::create_dir(root.0.join(".git/objects")).unwrap();
    assert!(revert(&root.0, "tracked").is_err());
    assert_eq!(fs::read_to_string(root.0.join("tracked")).unwrap(), "edited\n");
    git_ok(&root.0, &["ls-files", "--error-unmatch", "--", "tracked"]);
}

#[test]
fn search_ranks_matches_and_skips_ignored_dirs_without_git() {
    let root = Scratch::new("search-nogit");
    for dir in ["src/components", "node_modules/pkg", "target/debug"] {
        fs::create_dir_all(root.0.join(dir)).unwrap();
    }
    fs::write(root.0.join("src/components/Composer.vue"), "").unwrap();
    fs::write(root.0.join("src/commands.ts"), "").unwrap();
    fs::write(root.0.join("README.md"), "").unwrap();
    fs::write(root.0.join("node_modules/pkg/composer.js"), "").unwrap();
    fs::write(root.0.join("target/debug/composer"), "").unwrap();
    // Prevent discovery of an ancestor repo when the harness puts TMPDIR inside a checkout.
    fs::write(root.0.join(".git"), "gitdir: missing\n").unwrap();

    let results = search(&root.0, "comp", 10).unwrap();
    assert_eq!(results, ["src/components/Composer.vue"]);

    let fuzzy = search(&root.0, "cmpsr", 10).unwrap();
    assert_eq!(fuzzy, ["src/components/Composer.vue"]);

    let all = search(&root.0, "", 2).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0], "README.md");
}

#[test]
fn search_respects_gitignore_and_skips_deleted_files() {
    let root = Scratch::new("search-git");
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(&root.0)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(status.status.success(), "{:?}", status);
    };
    git(&["init", "-q"]);
    fs::write(root.0.join(".gitignore"), "ignored.log\nbuild-out/\n").unwrap();
    fs::create_dir_all(root.0.join("build-out")).unwrap();
    fs::write(root.0.join("ignored.log"), "").unwrap();
    fs::write(root.0.join("build-out/secret.ts"), "").unwrap();
    fs::write(root.0.join("kept.ts"), "").unwrap();
    fs::write(root.0.join("deleted.ts"), "").unwrap();
    git(&["add", "kept.ts", "deleted.ts", ".gitignore"]);
    git(&["commit", "-qm", "init"]);
    fs::remove_file(root.0.join("deleted.ts")).unwrap();
    fs::write(root.0.join("untracked.ts"), "").unwrap();

    let ts = search(&root.0, ".ts", 10).unwrap();
    assert_eq!(ts, ["kept.ts", "untracked.ts"]);
    assert!(search(&root.0, "ignored", 10).unwrap().is_empty());
    fs::create_dir(root.0.join("only-ignored")).unwrap();
    fs::write(root.0.join("only-ignored/ignored.log"), "private").unwrap();
    assert!(
        search(&root.0.join("only-ignored"), "", 10)
            .unwrap()
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn search_omits_symlinks_leaving_the_workspace() {
    let root = Scratch::new("search-symlink");
    fs::write(root.0.join(".git"), "gitdir: missing\n").unwrap();
    let outside = Scratch::new("search-symlink-outside");
    fs::write(outside.0.join("secret.txt"), "x").unwrap();
    fs::write(root.0.join("inside.txt"), "ok").unwrap();
    std::os::unix::fs::symlink(outside.0.join("secret.txt"), root.0.join("escape.txt")).unwrap();

    let results = search(&root.0, ".txt", 10).unwrap();
    assert_eq!(results, ["inside.txt"]);
}

/// Run git in `root` with a throwaway identity, asserting success. New repositories store and
/// restore bytes as written, independent of a host-wide `core.autocrlf` (on for Windows runners).
fn git_ok(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    if args.first() == Some(&"init") {
        git_ok(root, &["config", "core.autocrlf", "false"]);
    }
}

#[test]
fn listing_hides_gitignored_entries_unless_asked() {
    let root = Scratch::new("list-ignore");
    git_ok(&root.0, &["init", "-q"]);
    fs::write(root.0.join(".gitignore"), "dial-out/\n*.dialtmp\n").unwrap();
    for dir in ["dial-out", "kept"] {
        fs::create_dir_all(root.0.join(dir)).unwrap();
    }
    fs::write(root.0.join("dial-out/x.txt"), "").unwrap();
    fs::write(root.0.join("scratch.dialtmp"), "").unwrap();
    fs::write(root.0.join("kept/scratch.dialtmp"), "").unwrap();
    fs::write(root.0.join("b.txt"), "").unwrap();

    let names = |entries: Vec<DirEntryView>| -> Vec<(String, bool)> {
        entries.into_iter().map(|e| (e.name, e.ignored)).collect()
    };
    assert_eq!(
        names(list(&root.0, "").unwrap()),
        [("kept".into(), false), (".gitignore".into(), false), ("b.txt".into(), false)]
    );
    assert!(list(&root.0, "kept").unwrap().is_empty());
    assert_eq!(
        names(list_with(&root.0, "", true).unwrap()),
        [
            ("dial-out".into(), true),
            ("kept".into(), false),
            (".gitignore".into(), false),
            ("b.txt".into(), false),
            ("scratch.dialtmp".into(), true),
        ]
    );
}

#[test]
fn porcelain_status_is_rerooted_and_classified() {
    let raw = b" M src/a.rs\0A  src/new.rs\0R  src/to.rs\0src/from.rs\0?? src/tmp/\0 D src/gone.rs\0UU src/both.rs\0M  other/x.rs\0?? src/\0";
    let got: Vec<(String, char)> = parse_status(raw, "src/")
        .into_iter()
        .map(|s| (s.path, s.status))
        .collect();
    let want = [
        ("a.rs", 'M'),
        ("new.rs", 'A'),
        ("to.rs", 'R'),
        ("tmp/", 'U'),
        ("gone.rs", 'D'),
        ("both.rs", 'C'),
    ];
    assert_eq!(got, want.map(|(p, s)| (p.to_owned(), s)));
    assert_eq!(parse_status(b"?? a\0", "").len(), 1);
    let names = parse_status(b" M bad\xff\0?? bad\xef\xbf\xbd\0", "");
    assert_eq!(names.len(), 1);
    assert_eq!((names[0].path.as_str(), names[0].status), ("bad�", 'U'));

    let stats = parse_numstat(b"3\t1\ta.rs\0-\t-\tlogo.png\x002\t0\t\0old.rs\0new.rs\0");
    assert_eq!(stats["a.rs"], (Some(3), Some(1)));
    assert_eq!(stats["logo.png"], (None, None));
    assert_eq!(stats["new.rs"], (Some(2), Some(0)));
    assert!(!stats.contains_key("old.rs"));
}

#[test]
fn status_reports_changes_with_line_counts() {
    let root = Scratch::new("status");
    let outside = Scratch::new("status-nogit");
    fs::write(outside.0.join(".git"), "gitdir: missing\n").unwrap();
    assert!(!status(&outside.0, true).unwrap().repo);

    git_ok(&root.0, &["init", "-q"]);
    fs::write(root.0.join("a.txt"), "one\ntwo\n").unwrap();
    git_ok(&root.0, &["add", "a.txt"]);
    git_ok(&root.0, &["commit", "-qm", "init"]);
    fs::write(root.0.join("a.txt"), "one\nthree\nfour\n").unwrap();
    fs::write(root.0.join("new.txt"), "x\ny\n").unwrap();

    let view = status(&root.0, true).unwrap();
    assert!(view.repo);
    let found: Vec<_> = view
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.status, f.added, f.removed))
        .collect();
    assert_eq!(
        found,
        [("a.txt", 'M', Some(2), Some(1)), ("new.txt", 'U', Some(2), Some(0))]
    );
    assert!(status(&root.0, false).unwrap().files.iter().all(|f| f.added.is_none()));
}

#[test]
fn nested_changes_use_the_child_index_and_workspace_paths() {
    let root = Scratch::new("nested-changes");
    let child = root.0.join("packages/child");
    fs::create_dir_all(child.join("src")).unwrap();
    git_ok(&child, &["init", "-q"]);
    fs::write(child.join("src/a"), "before\n").unwrap();
    git_ok(&child, &["add", "src/a"]);
    git_ok(&child, &["commit", "-qm", "initial"]);
    fs::write(child.join("src/a"), "after\n").unwrap();
    git_ok(&child, &["add", "src/a"]);
    let path = "packages/child/src/a";
    for parent_repo in [false, true] {
        if parent_repo {
            git_ok(&root.0, &["init", "-q"]);
            fs::write(root.0.join("a"), "outer\n").unwrap();
        }
        let view = status(&root.0, true).unwrap();
        assert_eq!(view.repo, parent_repo, "commit controls belong to the workspace repo");
        let row = view.files.iter().find(|f| f.path == path).expect("nested file status");
        assert_eq!((row.status, row.staged, row.added, row.removed), ('M', true, Some(1), Some(1)));
        let shown = diff(&root.0, path).unwrap();
        assert_eq!(shown.path, path);
        assert!(shown.diff.contains("-before\n+after"));
        assert!(shown.nested);
        assert!(stage(&root.0, &[path.to_owned()], false).is_err());
        assert!(revert(&root.0, path).is_err());
        assert!(revert_hunk(&root.0, path, split_hunks(&shown.diff).1[0]).is_err());
        assert_eq!(fs::read_to_string(child.join("src/a")).unwrap(), "after\n");
        assert_eq!(commit_review(&child).unwrap().paths, ["src/a"]);
    }
}

#[test]
fn linked_nested_worktrees_show_deletions_and_untracked_files() {
    let root = Scratch::new("nested-worktree");
    let repo = Scratch::new("nested-worktree-source");
    git_ok(&repo.0, &["init", "-q"]);
    fs::write(repo.0.join("tracked"), "original\n").unwrap();
    git_ok(&repo.0, &["add", "tracked"]);
    git_ok(&repo.0, &["commit", "-qm", "initial"]);
    let child = root.0.join("linked");
    git_ok(&repo.0, &["worktree", "add", "--detach", child.to_str().unwrap()]);
    fs::remove_file(child.join("tracked")).unwrap();
    fs::create_dir(child.join("new")).unwrap();
    fs::write(child.join("new/file"), "added\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&repo.0, root.0.join("outside")).unwrap();
    let view = status(&root.0, false).unwrap();
    assert_eq!(view.nested_repos, ["linked"]);
    assert!(view.files.iter().any(|f| f.path == "linked/new/file" && f.status == 'U'));
    assert!(view.files.iter().any(|f| f.path == "linked/tracked" && f.status == 'D'));
    assert!(diff(&root.0, "linked/tracked").unwrap().diff.contains("-original"));
    assert!(diff(&root.0, "linked/new/file").unwrap().diff.contains("+added"));
}

#[test]
fn revert_discards_changes_inside_the_workspace_only() {
    let root = Scratch::new("revert");
    git_ok(&root.0, &["init", "-q"]);
    fs::write(root.0.join("a.txt"), "one\n").unwrap();
    fs::write(root.0.join("gone.txt"), "kept\n").unwrap();
    git_ok(&root.0, &["add", "."]);
    git_ok(&root.0, &["commit", "-qm", "init"]);

    // Modified and staged: back to HEAD.
    fs::write(root.0.join("a.txt"), "two\n").unwrap();
    git_ok(&root.0, &["add", "a.txt"]);
    revert(&root.0, "a.txt").unwrap();
    assert_eq!(fs::read_to_string(root.0.join("a.txt")).unwrap(), "one\n");
    // Deletion can be in the index or only the worktree. Hunk undo touches just the latter.
    for staged in [false, true] {
        fs::remove_file(root.0.join("gone.txt")).unwrap();
        if staged { stage(&root.0, &["gone.txt".into()], true).unwrap(); }
        let shown = diff(&root.0, "gone.txt").unwrap();
        assert_eq!(shown.base, DiffBase::Head);
        assert!(!shown.delete_on_revert);
        assert!(shown.diff.contains("-kept"));
        revert_hunk(&root.0, "gone.txt", split_hunks(&shown.diff).1[0]).unwrap();
        assert_eq!(fs::read_to_string(root.0.join("gone.txt")).unwrap(), "kept\n");
        fs::remove_file(root.0.join("gone.txt")).unwrap();
        revert(&root.0, "gone.txt").unwrap();
        assert_eq!(fs::read_to_string(root.0.join("gone.txt")).unwrap(), "kept\n");
    }
    // Newly added (staged) and untracked: unstaged and deleted.
    fs::write(root.0.join("staged.txt"), "s").unwrap();
    git_ok(&root.0, &["add", "staged.txt"]);
    fs::write(root.0.join("loose.txt"), "l").unwrap();
    for path in ["staged.txt", "loose.txt"] {
        revert(&root.0, path).unwrap();
        assert!(!root.0.join(path).exists(), "{path}");
    }
    assert!(status(&root.0, false).unwrap().files.is_empty());
    assert!(revert(&root.0, ".").is_err());

    let outside = Scratch::new("revert-outside");
    fs::write(outside.0.join("secret"), "x").unwrap();
    for path in [
        "../secret".to_owned(),
        "missing/../../secret".to_owned(),
        outside.0.join("secret").to_string_lossy().into_owned(),
    ] {
        assert!(revert(&root.0, &path).unwrap_err().contains("outside"), "{path}");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.0.join("secret"), root.0.join("link")).unwrap();
        assert!(revert(&root.0, "link").unwrap_err().contains("outside"));
        std::os::unix::fs::symlink(&outside.0, root.0.join("external")).unwrap();
        assert!(revert(&root.0, "external/missing").unwrap_err().contains("outside"));
        std::os::unix::fs::symlink(outside.0.join("missing"), root.0.join("dangling")).unwrap();
        assert!(revert(&root.0, "dangling").is_err());
        assert!(fs::symlink_metadata(root.0.join("dangling")).unwrap().is_symlink());
    }
    assert_eq!(fs::read_to_string(outside.0.join("secret")).unwrap(), "x");
}

#[test]
fn only_plain_files_open_in_the_default_app() {
    let root = Scratch::new("openable");
    fs::write(root.0.join("notes.md"), "").unwrap();
    fs::write(root.0.join("run.command"), "").unwrap();
    assert!(openable(&root.0, "notes.md").is_ok());
    assert!(openable(&root.0, "run.command").unwrap_err().contains("program"));
    assert!(openable(&root.0, ".").is_err());
    assert!(openable(&root.0, "../x").is_err());
    let uri = vscode_line_uri(&root.0.join("a #%.rs"), 17).unwrap();
    assert!(uri.starts_with("vscode://file/"));
    assert!(uri.ends_with("/a%20%23%25.rs:17"));
}

#[test]
fn revert_hunk_undoes_only_that_hunk() {
    let root = Scratch::new("revert-hunk");
    git_ok(&root.0, &["init", "-q"]);
    let original: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    fs::write(root.0.join("a.txt"), &original).unwrap();
    git_ok(&root.0, &["add", "."]);
    git_ok(&root.0, &["commit", "-qm", "init"]);
    let edited = original.replace("line 2\n", "line two\n").replace("line 28\n", "line twenty-eight\n");
    fs::write(root.0.join("a.txt"), &edited).unwrap();

    let shown = diff(&root.0, "a.txt").unwrap().diff;
    let (_, hunks) = split_hunks(&shown);
    assert_eq!(hunks.len(), 2, "{shown}");
    // The panel sends a hunk as its header and lines joined by newlines, without the last one.
    let second = hunks[1].trim_end_matches('\n');
    revert_hunk(&root.0, "a.txt", second).unwrap();
    assert_eq!(
        fs::read_to_string(root.0.join("a.txt")).unwrap(),
        original.replace("line 2\n", "line two\n")
    );
    // The same hunk again is no longer in the diff, and is refused without touching the file.
    assert!(revert_hunk(&root.0, "a.txt", second).unwrap_err().contains("no longer"));
    assert!(revert_hunk(&root.0, "a.txt", "@@ -1 +1 @@\n-x\n+y").unwrap_err().contains("no longer"));
    let first = split_hunks(&diff(&root.0, "a.txt").unwrap().diff).1[0].to_owned();
    revert_hunk(&root.0, "a.txt", &first).unwrap();
    assert_eq!(fs::read_to_string(root.0.join("a.txt")).unwrap(), original);

    fs::write(root.0.join("new.txt"), "fresh\n").unwrap();
    let added = diff(&root.0, "new.txt").unwrap().diff;
    let hunk = split_hunks(&added).1[0].to_owned();
    assert!(revert_hunk(&root.0, "new.txt", &hunk).unwrap_err().contains("new file"));
    assert!(revert_hunk(&root.0, "../a.txt", &hunk).unwrap_err().contains("outside"));
}

#[test]
fn revert_hunk_works_in_a_subfolder_workspace() {
    let repo = Scratch::new("revert-hunk-sub");
    git_ok(&repo.0, &["init", "-q"]);
    fs::create_dir_all(repo.0.join("app")).unwrap();
    fs::write(repo.0.join("app/x.txt"), "a\nb\nc\n").unwrap();
    git_ok(&repo.0, &["add", "."]);
    git_ok(&repo.0, &["commit", "-qm", "init"]);
    fs::write(repo.0.join("app/x.txt"), "a\nB\nc\n").unwrap();
    let workspace = repo.0.join("app");
    let hunk = split_hunks(&diff(&workspace, "x.txt").unwrap().diff).1[0].to_owned();
    revert_hunk(&workspace, "x.txt", &hunk).unwrap();
    assert_eq!(fs::read_to_string(workspace.join("x.txt")).unwrap(), "a\nb\nc\n");
}

#[test]
fn staging_commits_and_branch() {
    let root = Scratch::new("stage-commit");
    git_ok(&root.0, &["init", "-q", "-b", "trunk"]);
    fs::write(root.0.join("a.txt"), "one\n").unwrap();
    fs::write(root.0.join("b.txt"), "two\n").unwrap();
    let flags = |path: &str| {
        let view = status(&root.0, false).unwrap();
        let file = view.files.into_iter().find(|f| f.path == path).unwrap();
        (file.staged, file.unstaged)
    };
    // Before the first commit: stage, unstage, stage again.
    stage(&root.0, &["a.txt".into()], true).unwrap();
    assert_eq!(flags("a.txt"), (true, false));
    stage(&root.0, &["a.txt".into()], false).unwrap();
    assert_eq!(flags("a.txt"), (false, true));
    assert!(commit_review(&root.0).unwrap_err().contains("Nothing is staged"));
    stage(&root.0, &["a.txt".into()], true).unwrap();
    let reviewed = commit_review(&root.0).unwrap();
    assert_eq!(reviewed.paths, ["a.txt"]);
    assert!(commit(&root.0, "  ", &reviewed).unwrap_err().contains("message"));
    // Another client may stage a different blob under the very same path after the review.
    fs::write(root.0.join("a.txt"), "revised\n").unwrap();
    stage(&root.0, &["a.txt".into()], true).unwrap();
    assert!(commit(&root.0, "stale", &reviewed).unwrap_err().contains("Review"));
    let reviewed = commit_review(&root.0).unwrap();
    // Respect another writer's lock without taking or deleting it.
    let lock = root.0.join(".git/index.lock");
    fs::write(&lock, "other writer").unwrap();
    assert!(commit(&root.0, "busy", &reviewed).unwrap_err().contains("lock"));
    assert_eq!(fs::read_to_string(&lock).unwrap(), "other writer");
    fs::remove_file(lock).unwrap();

    // A throwaway identity in the repository's config, as the user's own config would provide.
    for (key, value) in [("user.name", "t"), ("user.email", "t@t"), ("commit.gpgsign", "false")] {
        git_ok(&root.0, &["config", key, value]);
    }
    let sha = commit(&root.0, "Add a", &reviewed).unwrap();
    assert!(!sha.is_empty());
    let view = status(&root.0, false).unwrap();
    assert_eq!(view.branch.as_deref(), Some("trunk"));
    assert_eq!(view.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["b.txt"]);

    // After it: a modified file unstages back to HEAD's index entry, the edit kept.
    fs::write(root.0.join("a.txt"), "changed\n").unwrap();
    stage(&root.0, &["a.txt".into()], true).unwrap();
    assert_eq!(flags("a.txt"), (true, false));
    stage(&root.0, &["a.txt".into()], false).unwrap();
    assert_eq!(flags("a.txt"), (false, true));
    assert_eq!(fs::read_to_string(root.0.join("a.txt")).unwrap(), "changed\n");
    // A deleted file stages as a deletion.
    fs::remove_file(root.0.join("a.txt")).unwrap();
    stage(&root.0, &["a.txt".into()], true).unwrap();
    assert_eq!(flags("a.txt"), (true, false));
    assert!(stage(&root.0, &["../x".into()], true).unwrap_err().contains("outside"));
}

#[test]
fn commit_refuses_staged_files_outside_the_workspace() {
    let repo = Scratch::new("commit-outside");
    git_ok(&repo.0, &["init", "-q"]);
    fs::create_dir_all(repo.0.join("app")).unwrap();
    fs::write(repo.0.join("app/in.txt"), "x").unwrap();
    fs::write(repo.0.join("out.txt"), "y").unwrap();
    git_ok(&repo.0, &["add", "."]);
    let err = commit_review(&repo.0.join("app")).unwrap_err();
    assert!(err.contains("outside this project"), "{err}");
    // A rename *into* the project also stages the removal of a path outside it.
    git_ok(&repo.0, &["commit", "-qm", "init"]);
    git_ok(&repo.0, &["mv", "out.txt", "app/moved.txt"]);
    assert!(commit_review(&repo.0.join("app")).unwrap_err().contains("outside this project"));
}

#[test]
fn the_commit_author_is_saved_for_one_repository_and_read_back() {
    let repo = Scratch::new("identity");
    git_ok(&repo.0, &["init", "-q"]);
    let saved = set_identity(&repo.0, "  Ada Lovelace ", "ada@example.com", false).unwrap();
    assert_eq!(saved, GitIdentity { name: "Ada Lovelace".into(), email: "ada@example.com".into() });
    assert_eq!(identity(&repo.0).unwrap(), saved);
    for (name, email) in [("", "a@b.c"), ("Ada", "not-an-email"), ("Ada", "a b@c.d"), ("--global", "a@b.c"), ("A <x>", "a@b.c"), ("Ada", "a@b.c\nx")] {
        assert!(set_identity(&repo.0, name, email, false).is_err(), "{name:?} {email:?}");
    }
    assert_eq!(identity(&repo.0).unwrap(), saved);
}

#[test]
fn content_search_finds_lines_with_and_without_git() {
    let plain = Scratch::new("grep-nogit");
    fs::write(plain.0.join(".git"), "gitdir: missing\n").unwrap();
    fs::create_dir_all(plain.0.join("src")).unwrap();
    fs::create_dir_all(plain.0.join("node_modules/pkg")).unwrap();
    fs::write(plain.0.join("src/a.rs"), "fn main() {\n    let Needle = 1;\n}\n").unwrap();
    fs::write(plain.0.join("node_modules/pkg/x.js"), "needle").unwrap();
    fs::write(plain.0.join("blob.bin"), b"needle\0\0").unwrap();
    let found = grep(&plain.0, "needle", 10).unwrap();
    assert_eq!(found, [GrepMatch { path: "src/a.rs".into(), line: 2, text: "let Needle = 1;".into() }]);
    assert!(grep(&plain.0, "  ", 10).unwrap().is_empty());

    let repo = Scratch::new("grep-git");
    git_ok(&repo.0, &["init", "-q"]);
    fs::write(repo.0.join(".gitignore"), "skip.txt\n").unwrap();
    fs::write(repo.0.join("skip.txt"), "needle\n").unwrap();
    fs::write(repo.0.join("tracked.txt"), "a\nneedle one\nneedle two\n").unwrap();
    git_ok(&repo.0, &["add", "tracked.txt"]);
    fs::write(repo.0.join("loose.txt"), "-needle\n").unwrap();
    let mut found: Vec<_> = grep(&repo.0, "-NEEDLE", 10).unwrap().into_iter().map(|m| (m.path, m.line)).collect();
    found.sort();
    assert_eq!(found, [("loose.txt".to_owned(), 1)]);
    let mut found: Vec<_> = grep(&repo.0, "needle", 10).unwrap().into_iter().map(|m| (m.path, m.line)).collect();
    found.sort();
    assert_eq!(found, [("loose.txt".to_owned(), 1), ("tracked.txt".to_owned(), 2), ("tracked.txt".to_owned(), 3)]);
    assert_eq!(grep(&repo.0, "needle", 1).unwrap().len(), 1);
}

#[test]
fn log_records_split_on_separators_and_skip_noise() {
    let text = "0123456789abcdef0123456789abcdef01234567\u{1f}Ada\u{1f}2026-11-25T08:00:00+00:00\u{1f}HEAD -> main\u{1f}fix: keep | pipes\u{1e}\nnot-a-sha\u{1f}x\u{1f}y\u{1f}\u{1f}z\u{1e}\nfedcba9\u{1f}Bo\u{1f}2026-11-24T08:00:00+00:00\u{1f}\u{1f}first\u{1e}\n";
    let rows = parse_log(text);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].subject, "fix: keep | pipes");
    assert_eq!(rows[0].refs, "HEAD -> main");
    assert_eq!(rows[0].author, "Ada");
    assert_eq!(rows[1].sha, "fedcba9");
    assert_eq!(rows[1].refs, "");
}

#[test]
fn write_updates_workspace_files_and_refuses_escapes_and_links() {
    let root = Scratch::new("write-inline");
    let outside = Scratch::new("write-inline-outside");
    fs::write(root.0.join("note.txt"), "before\n").unwrap();
    fs::write(outside.0.join("secret.txt"), "keep\n").unwrap();

    let initial = read(&root.0, "note.txt").unwrap();
    assert!(initial.sha256.as_deref().is_some_and(|h| h.len() == 64));
    let saved = write(&root.0, "note.txt", "after\n", initial.sha256.as_deref()).unwrap();
    assert_eq!(saved.text.as_deref(), Some("after\n"));
    assert_ne!(saved.sha256, initial.sha256);
    assert_eq!(fs::read_to_string(root.0.join("note.txt")).unwrap(), "after\n");

    // Concurrent external modification is refused rather than silently clobbered.
    fs::write(root.0.join("note.txt"), "external edit\n").unwrap();
    let conflict = write(&root.0, "note.txt", "stale save\n", saved.sha256.as_deref()).unwrap_err();
    assert!(conflict.contains("changed on disk"), "{conflict}");
    assert_eq!(fs::read_to_string(root.0.join("note.txt")).unwrap(), "external edit\n");

    assert!(write(&root.0, "../secret.txt", "bad", None).is_err());
    assert!(write(&root.0, ".git/config", "bad", None).is_err());
    assert!(write(&root.0, "missing.txt", "bad", None).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.0.join("secret.txt"), root.0.join("out.link")).unwrap();
        std::os::unix::fs::symlink(root.0.join("note.txt"), root.0.join("in.link")).unwrap();
        assert!(write(&root.0, "out.link", "bad", None).is_err());
        assert!(write(&root.0, "in.link", "bad", None).is_err());
        assert_eq!(fs::read_to_string(outside.0.join("secret.txt")).unwrap(), "keep\n");
    }
}

#[test]
fn git_sync_errors_give_actionable_hints() {
    assert!(format_git_sync_error("push", "fatal: could not read Username for 'https://github.com': terminal prompts disabled").contains("authentication"));
    assert!(format_git_sync_error("pull", "fatal: Not possible to fast-forward, aborting.").contains("diverged"));
    assert!(format_git_sync_error("pull", "error: Your local changes to the following files would be overwritten by merge").contains("stash"));
}

#[test]
fn stage_hunk_stages_only_the_selected_hunk_and_refuses_stale_hunks() {
    let root = Scratch::new("stage-hunk");
    git_ok(&root.0, &["init", "-q"]);
    let original: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    fs::write(root.0.join("a.txt"), &original).unwrap();
    git_ok(&root.0, &["add", "a.txt"]);
    git_ok(&root.0, &["commit", "-qm", "init"]);

    let edited = original
        .replace("line 2\n", "line 2 edited\nextra top\n")
        .replace("line 18\n", "line 18 edited\n");
    fs::write(root.0.join("a.txt"), &edited).unwrap();

    let shown = diff(&root.0, "a.txt").unwrap();
    let (_, hunks) = split_hunks(&shown.diff);
    assert_eq!(hunks.len(), 2, "{}", shown.diff);

    // Stage only the first hunk.
    stage_hunk(&root.0, "a.txt", hunks[0]).unwrap();
    let staged = String::from_utf8(git_in(&root.0, &["diff", "--cached", "--", "a.txt"]).unwrap().stdout).unwrap();
    let unstaged = String::from_utf8(git_in(&root.0, &["diff", "--", "a.txt"]).unwrap().stdout).unwrap();
    assert!(staged.contains("+line 2 edited"), "{staged}");
    assert!(!staged.contains("+line 18 edited"), "{staged}");
    assert!(unstaged.contains("+line 18 edited"), "{unstaged}");

    // Staging the same hunk again is refused cleanly as already staged.
    let dup = stage_hunk(&root.0, "a.txt", hunks[0]).unwrap_err();
    assert!(dup.contains("already staged"), "{dup}");

    // Staging the second hunk from the HEAD diff adjusts cleanly against the index.
    stage_hunk(&root.0, "a.txt", hunks[1]).unwrap();
    let unstaged_after = String::from_utf8(git_in(&root.0, &["diff", "--", "a.txt"]).unwrap().stdout).unwrap();
    assert!(unstaged_after.trim().is_empty(), "{unstaged_after}");

    // Stale hunk is refused.
    assert!(stage_hunk(&root.0, "a.txt", "@@ -1,1 +1,1 @@\n-bogus\n+nope\n").is_err());
}

#[test]
fn diff_scoped_and_unstage_hunk_split_staged_and_unstaged_changes() {
    let root = Scratch::new("diff-scoped-unstage");
    git_ok(&root.0, &["init", "-q"]);
    let original: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    fs::write(root.0.join("a.txt"), &original).unwrap();
    git_ok(&root.0, &["add", "a.txt"]);
    git_ok(&root.0, &["commit", "-qm", "init"]);

    let edited = original
        .replace("line 2\n", "line 2 edited\n")
        .replace("line 18\n", "line 18 edited\n");
    fs::write(root.0.join("a.txt"), &edited).unwrap();

    let unstaged = diff_scoped(&root.0, "a.txt", DiffScope::Unstaged).unwrap();
    let (_, hunks) = split_hunks(&unstaged.diff);
    assert_eq!(hunks.len(), 2);
    stage_hunk(&root.0, "a.txt", hunks[0]).unwrap();

    let staged = diff_scoped(&root.0, "a.txt", DiffScope::Staged).unwrap();
    assert!(staged.diff.contains("+line 2 edited"));
    assert!(!staged.diff.contains("+line 18 edited"));
    let remaining_unstaged = diff_scoped(&root.0, "a.txt", DiffScope::Unstaged).unwrap();
    assert!(!remaining_unstaged.diff.contains("+line 2 edited"));
    assert!(remaining_unstaged.diff.contains("+line 18 edited"));

    // Unstage the first hunk; working tree keeps the edit while index clears.
    let (_, staged_hunks) = split_hunks(&staged.diff);
    assert_eq!(staged_hunks.len(), 1);
    unstage_hunk(&root.0, "a.txt", staged_hunks[0]).unwrap();
    assert!(diff_scoped(&root.0, "a.txt", DiffScope::Staged).unwrap().diff.trim().is_empty());
    assert!(fs::read_to_string(root.0.join("a.txt")).unwrap().contains("line 2 edited"));
}

#[test]
fn create_rename_and_delete_file_enforce_workspace_guards() {
    let root = Scratch::new("file-crud");
    let created = create_file(&root.0, "nested/hello.txt", "hi\n").unwrap();
    assert_eq!(created.path, "nested/hello.txt");
    assert_eq!(created.text.as_deref(), Some("hi\n"));
    // Refuses duplicate, escapes, and .git.
    assert!(create_file(&root.0, "nested/hello.txt", "dup").is_err());
    assert!(create_file(&root.0, "../escape.txt", "no").is_err());
    assert!(create_file(&root.0, ".git/evil", "no").is_err());

    let moved = rename_file(&root.0, "nested/hello.txt", "nested/world.txt").unwrap();
    assert_eq!(moved, "nested/world.txt");
    assert!(!root.0.join("nested/hello.txt").exists());
    assert_eq!(fs::read_to_string(root.0.join("nested/world.txt")).unwrap(), "hi\n");
    assert!(rename_file(&root.0, "nested/world.txt", "../out.txt").is_err());

    delete_file(&root.0, "nested/world.txt").unwrap();
    assert!(!root.0.join("nested/world.txt").exists());
    assert!(delete_file(&root.0, "../outside").is_err());
}

#[test]
fn git_branches_checkout_sync_and_stash_work_end_to_end() {
    let remote = Scratch::new("git-remote");
    git_ok(&remote.0, &["init", "--bare", "-q"]);

    let root = Scratch::new("git-ops");
    git_ok(&root.0, &["init", "-q", "-b", "main"]);
    fs::write(root.0.join("a.txt"), "one\n").unwrap();
    git_ok(&root.0, &["add", "a.txt"]);
    git_ok(&root.0, &["commit", "-qm", "init"]);
    git_ok(&root.0, &["remote", "add", "origin", remote.0.to_str().unwrap()]);

    // Push sets upstream on main.
    git_sync(&root.0, "push").unwrap();
    let info = git_branches(&root.0).unwrap();
    assert_eq!(info.current.as_deref(), Some("main"));
    assert_eq!(info.local, vec!["main".to_owned()]);
    assert_eq!(info.upstream.as_deref(), Some("origin/main"));
    assert_eq!((info.ahead, info.behind), (0, 0));

    // Create and switch branches, and reject invalid names.
    assert!(git_checkout(&root.0, "-bad", true).is_err());
    assert!(git_checkout(&root.0, "does-not-exist", false).is_err());
    git_checkout(&root.0, "feature/test", true).unwrap();
    assert_eq!(git_branches(&root.0).unwrap().current.as_deref(), Some("feature/test"));
    git_checkout(&root.0, "main", false).unwrap();
    assert_eq!(git_branches(&root.0).unwrap().current.as_deref(), Some("main"));

    // Stash push & pop round-trip.
    fs::write(root.0.join("a.txt"), "dirty\n").unwrap();
    let stashes = git_stash(&root.0, "push", Some("wip")).unwrap();
    assert_eq!(stashes.len(), 1);
    assert!(stashes[0].summary.contains("wip"));
    assert_eq!(fs::read_to_string(root.0.join("a.txt")).unwrap(), "one\n");
    let after_pop = git_stash(&root.0, "pop", None).unwrap();
    assert!(after_pop.is_empty());
    assert_eq!(fs::read_to_string(root.0.join("a.txt")).unwrap(), "dirty\n");

    // Fetch & fast-forward pull succeed against the local bare remote.
    git_sync(&root.0, "fetch").unwrap();
    git_sync(&root.0, "pull").unwrap();
}

#[test]
fn git_conflict_detection_and_resolution_support_ours_theirs_and_manual() {
    let root = Scratch::new("git-conflict");
    git_ok(&root.0, &["init", "-q", "-b", "main"]);
    for f in ["a.txt", "b.txt", "c.txt"] {
        fs::write(root.0.join(f), "base\n").unwrap();
    }
    git_ok(&root.0, &["add", "."]);
    git_ok(&root.0, &["commit", "-qm", "base"]);

    git_ok(&root.0, &["checkout", "-qb", "other"]);
    for f in ["a.txt", "b.txt", "c.txt"] {
        fs::write(root.0.join(f), "theirs\n").unwrap();
    }
    git_ok(&root.0, &["add", "."]);
    git_ok(&root.0, &["commit", "-qm", "theirs"]);

    git_ok(&root.0, &["checkout", "-q", "main"]);
    for f in ["a.txt", "b.txt", "c.txt"] {
        fs::write(root.0.join(f), "ours\n").unwrap();
    }
    git_ok(&root.0, &["add", "."]);
    git_ok(&root.0, &["commit", "-qm", "ours"]);

    // Merge fails with conflicts on all three files.
    let merge = git_in(&root.0, &["merge", "other"]).unwrap();
    assert!(!merge.status.success());

    let shown = diff_scoped(&root.0, "a.txt", DiffScope::All).unwrap();
    assert!(shown.conflicted, "unmerged file must report conflicted=true");
    assert!(
        git_resolve_conflict(&root.0, "a.txt", "resolved").is_err(),
        "marking resolved must refuse while conflict markers remain"
    );

    git_resolve_conflict(&root.0, "a.txt", "ours").unwrap();
    assert_eq!(fs::read_to_string(root.0.join("a.txt")).unwrap(), "ours\n");
    assert!(!diff_scoped(&root.0, "a.txt", DiffScope::All).unwrap().conflicted);

    git_resolve_conflict(&root.0, "b.txt", "theirs").unwrap();
    assert_eq!(fs::read_to_string(root.0.join("b.txt")).unwrap(), "theirs\n");
    assert!(!diff_scoped(&root.0, "b.txt", DiffScope::All).unwrap().conflicted);

    fs::write(root.0.join("c.txt"), "merged cleanly\n").unwrap();
    git_resolve_conflict(&root.0, "c.txt", "resolved").unwrap();
    assert!(!diff_scoped(&root.0, "c.txt", DiffScope::All).unwrap().conflicted);
    assert!(git_resolve_conflict(&root.0, "c.txt", "ours").is_err());
}

#[cfg(unix)]
#[test]
fn git_sync_cancellation_kills_running_command_promptly() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    let root = Scratch::new("git-sync-cancel");
    git_ok(&root.0, &["init", "-q"]);
    // Configure an SSH remote whose ssh command sleeps so `git fetch` blocks without network.
    git_ok(&root.0, &["remote", "add", "origin", "ssh://example.invalid/repo.git"]);
    git_ok(&root.0, &["config", "core.sshCommand", "sh -c 'sleep 30' --"]);

    let canceller = Arc::new(GitSyncCanceller::default());
    let guard = canceller.begin(&root.0);
    let cancel_clone = Arc::clone(&canceller);
    let root_clone = root.0.clone();
    let killer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        assert!(cancel_clone.cancel(&root_clone));
    });

    let started = Instant::now();
    let err = git_sync_cancellable(&root.0, "fetch", guard.flag()).unwrap_err();
    killer.join().unwrap();
    assert!(
        err.to_ascii_lowercase().contains("cancel"),
        "expected cancellation error, got: {err}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "cancelled git_sync should abort promptly, took {:?}",
        started.elapsed()
    );
    drop(guard);
    assert!(!canceller.cancel(&root.0), "guard drop must unregister active sync");

    // Pre-cancelled flag aborts before spawning git.
    let pre = AtomicBool::new(true);
    assert!(git_sync_cancellable(&root.0, "fetch", &pre).unwrap_err().contains("cancelled"));
}
