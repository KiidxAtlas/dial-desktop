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

/// Run git in `root` with a throwaway identity, asserting success.
fn git_ok(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
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
    // Deleted: restored.
    fs::remove_file(root.0.join("gone.txt")).unwrap();
    revert(&root.0, "gone.txt").unwrap();
    assert_eq!(fs::read_to_string(root.0.join("gone.txt")).unwrap(), "kept\n");
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
