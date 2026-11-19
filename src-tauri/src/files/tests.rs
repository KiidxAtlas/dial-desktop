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
