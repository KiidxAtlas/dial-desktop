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
    git(&["commit", "-qm", "init"]);
    fs::write(root.0.join("a.txt"), "two\n").unwrap();
    fs::write(root.0.join("new.txt"), "fresh\n").unwrap();

    let changed = diff(&root.0, "a.txt").unwrap();
    assert!(!changed.untracked);
    assert!(
        changed.diff.contains("-one") && changed.diff.contains("+two"),
        "{}",
        changed.diff
    );

    let added = diff(&root.0, &root.0.join("new.txt").to_string_lossy()).unwrap();
    assert!(added.untracked);
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
