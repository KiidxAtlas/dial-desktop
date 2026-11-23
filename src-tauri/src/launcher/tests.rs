use super::*;

#[test]
fn env_output_parses_nul_separated_pairs() {
    let out = b"PATH=/usr/bin:/bin\0ANTHROPIC_API_KEY=sk=1\0MULTI=a\nb\0junk\0\0";
    assert_eq!(
        parse_env(out),
        [
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
            ("ANTHROPIC_API_KEY".to_owned(), "sk=1".to_owned()),
            ("MULTI".to_owned(), "a\nb".to_owned()),
        ]
    );
}

#[test]
fn shell_env_fills_gaps_and_extends_path_without_overriding() {
    let pair = |k: &str, v: &str| (k.to_owned(), v.to_owned());
    let current = [pair("PATH", "/usr/bin:/bin"), pair("HOME", "/Users/ada")];
    let shell = [
        pair("PATH", "/Users/ada/.local/bin:/usr/bin"),
        pair("HOME", "/elsewhere"),
        pair("ANTHROPIC_API_KEY", "k"),
        pair("SHLVL", "2"),
    ];
    assert_eq!(
        env_changes(&current, &shell),
        [
            pair("PATH", "/usr/bin:/bin:/Users/ada/.local/bin"),
            pair("ANTHROPIC_API_KEY", "k"),
        ]
    );
    assert!(env_changes(&current, &[pair("PATH", "/bin")]).is_empty());
}

#[cfg(unix)]
#[test]
fn shell_probe_has_no_controlling_terminal() {
    // An interactive probe shell that shares the caller's terminal takes its foreground and
    // suspends whatever was running there (a terminal coding agent, `tauri dev`).
    let out = run_with_timeout(
        Command::new("/bin/sh").args(["-c", "(: </dev/tty) 2>/dev/null && echo tty || echo none"]),
        SHELL_TIMEOUT,
    );
    assert_eq!(out.as_deref(), Some(&b"none\n"[..]));
}

#[cfg(windows)]
#[test]
fn windows_probe_captures_stdout_without_a_visible_console() {
    let script = r#"
Add-Type -Name Console -Namespace Probe -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow();
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr hwnd);
'@
[Probe.Console]::IsWindowVisible([Probe.Console]::GetConsoleWindow())
"#;
    let out = run_with_timeout(
        Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", script]),
        Duration::from_secs(30),
    )
    .expect("probe returned its piped stdout");
    assert_eq!(String::from_utf8(out).unwrap().trim(), "False");
}

#[test]
fn dial_is_found_on_path_then_in_install_folders() {
    let root = std::env::temp_dir().join(format!("dial-desktop-find-{}", std::process::id()));
    let bin = root.join("bin");
    let local = root.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(local.join("dial"), "").unwrap();
    let path = std::env::join_paths([&bin]).unwrap();

    // Not on PATH: the ~/.local/bin fallback.
    assert_eq!(
        find_dial(None, Some(&path), Some(&root)),
        local.join("dial")
    );
    // On PATH wins.
    std::fs::write(bin.join("dial"), "").unwrap();
    assert_eq!(find_dial(None, Some(&path), Some(&root)), bin.join("dial"));
    // Explicit wins over everything; an empty value is ignored.
    assert_eq!(
        find_dial(Some("/opt/x/dial".into()), Some(&path), Some(&root)),
        PathBuf::from("/opt/x/dial")
    );
    assert_eq!(
        find_dial(Some("".into()), Some(&path), Some(&root)),
        bin.join("dial")
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn workspace_comes_from_argument_cwd_or_recents() {
    let home = Path::new("/Users/ada");
    let tmp = std::env::temp_dir();
    let recents = [PathBuf::from("/does/not/exist"), tmp.clone()];
    // An argument is resolved against the cwd and `~`.
    assert_eq!(
        choose_workspace(
            Some(Path::new("~/code/app")),
            Path::new("/"),
            Some(home),
            &[]
        ),
        Some(PathBuf::from("/Users/ada/code/app"))
    );
    assert_eq!(
        choose_workspace(Some(Path::new("app")), Path::new("/w"), Some(home), &[]),
        Some(PathBuf::from("/w/app"))
    );
    // A terminal launch in a project uses it.
    assert_eq!(
        choose_workspace(None, Path::new("/Users/ada/code/app"), Some(home), &recents),
        Some(PathBuf::from("/Users/ada/code/app"))
    );
    // A Finder launch (`/`), the home folder or `tauri dev` fall back to the newest existing recent.
    for cwd in [Path::new("/"), home, Path::new(env!("CARGO_MANIFEST_DIR"))] {
        assert_eq!(
            choose_workspace(None, cwd, Some(home), &recents),
            Some(tmp.clone())
        );
    }
    // Nothing usable: the folder picker.
    assert_eq!(
        choose_workspace(None, Path::new("/"), Some(home), &[]),
        None
    );
}

#[test]
fn recents_move_to_front_dedupe_and_persist() {
    let file = std::env::temp_dir()
        .join(format!("dial-desktop-recents-{}", std::process::id()))
        .join("recent.json");
    let mut recents = Recents::load(Some(file.clone()));
    assert!(recents.paths.is_empty());
    recents.touch(Path::new("/a"));
    recents.touch(Path::new("/b"));
    recents.touch(Path::new("/a"));
    assert_eq!(recents.paths, [PathBuf::from("/a"), PathBuf::from("/b")]);
    for i in 0..20 {
        recents.touch(Path::new(&format!("/p{i}")));
    }
    assert_eq!(recents.paths.len(), MAX_RECENTS);
    let reloaded = Recents::load(Some(file.clone()));
    assert_eq!(reloaded.paths, recents.paths);
    std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
}

#[test]
fn recents_forget_removes_a_folder_and_persists() {
    let file = std::env::temp_dir()
        .join(format!(
            "dial-desktop-recents-forget-{}",
            std::process::id()
        ))
        .join("recent.json");
    let mut recents = Recents::load(Some(file.clone()));
    recents.touch(Path::new("/a"));
    recents.touch(Path::new("/b"));
    recents.touch(Path::new("/c"));
    assert!(recents.forget(Path::new("/b")));
    assert_eq!(recents.paths, [PathBuf::from("/c"), PathBuf::from("/a")]);
    assert!(
        !recents.forget(Path::new("/missing")),
        "forgetting an unknown folder changes nothing"
    );
    let reloaded = Recents::load(Some(file.clone()));
    assert_eq!(reloaded.paths, recents.paths);
    std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
}

#[test]
fn recents_merge_changes_from_independent_windows() {
    let file = std::env::temp_dir()
        .join(format!(
            "dial-desktop-recents-windows-{}",
            std::process::id()
        ))
        .join("recent.json");
    let mut first = Recents::load(Some(file.clone()));
    first.touch(Path::new("/old"));
    let mut second = Recents::load(Some(file.clone()));
    second.touch(Path::new("/new"));
    assert!(first.forget(Path::new("/old")));
    second.touch(Path::new("/another"));
    let actual = Recents::load(Some(file.clone())).paths;
    std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
    assert_eq!(actual, [PathBuf::from("/another"), PathBuf::from("/new")]);
}
