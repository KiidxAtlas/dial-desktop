use super::*;

fn write_engine(dir: &Path) {
    let path = dir.join(DIAL_NAME);
    std::fs::write(&path, b"test engine").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}

#[test]
fn private_state_never_uses_workspace_relative_paths() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let config = root.path().join("config");
    for (os, name, fallback) in [
        ("linux", "dial-desktop", ".config"),
        ("windows", "dev.dial.desktop", "AppData/Roaming"),
    ] {
        assert_eq!(app_dir_from(os, None, Some(&config)), Some(config.join(name)));
        for bad in [Path::new(""), Path::new("relative")] {
            assert_eq!(app_dir_from(os, Some(&home), Some(bad)), Some(home.join(fallback).join(name)));
            assert_eq!(app_dir_from(os, Some(bad), Some(bad)), None);
        }
    }
    assert_eq!(app_dir_from("macos", None, Some(&config)), None);
}

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
    write_engine(&local);
    #[cfg(unix)]
    {
        // A present but non-executable file on PATH must not shadow the fallback.
        use std::os::unix::fs::PermissionsExt;
        let plain = bin.join(DIAL_NAME);
        std::fs::write(&plain, b"notes").unwrap();
        std::fs::set_permissions(plain, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let path = std::env::join_paths([&bin]).unwrap();

    // Not on PATH: the ~/.local/bin fallback.
    assert_eq!(
        find_dial(None, Some(&path), Some(&root)),
        local.join(DIAL_NAME)
    );
    // On PATH wins.
    write_engine(&bin);
    assert_eq!(find_dial(None, Some(&path), Some(&root)), bin.join(DIAL_NAME));
    // Explicit wins over everything; an empty value is ignored.
    assert_eq!(
        find_dial(Some("/opt/x/dial".into()), Some(&path), Some(&root)),
        PathBuf::from("/opt/x/dial")
    );
    assert_eq!(
        find_dial(Some("".into()), Some(&path), Some(&root)),
        bin.join(DIAL_NAME)
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn engine_origin_names_the_rule_that_chose_it() {
    let root = std::env::temp_dir().join(format!("dial-desktop-origin-{}", std::process::id()));
    let bin = root.join("bin");
    let local = root.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&local).unwrap();
    write_engine(&local);
    let path = std::env::join_paths([&bin]).unwrap();
    let at = |flag: Option<&str>, env: Option<&str>| {
        locate_dial(flag.map(Into::into), env.map(Into::into), None, Some(&path), Some(&root)).1
    };

    assert_eq!(at(None, None), EngineOrigin::Fallback);
    write_engine(&bin);
    assert_eq!(at(None, None), EngineOrigin::Path);
    assert_eq!(at(None, Some("/opt/x/dial")), EngineOrigin::Env);
    assert_eq!(at(Some("/opt/y/dial"), Some("/opt/x/dial")), EngineOrigin::Flag);
    assert_eq!(at(Some(""), Some("")), EngineOrigin::Path);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn settings_choice_ranks_below_flag_and_env_but_above_the_usual_order() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let bin = home.join("bin");
    let custom = home.join("custom");
    let desktop = app_dir(Some(home)).unwrap();
    let copy = desktop.join("engine-1");
    for dir in [&bin, &custom, &copy] {
        std::fs::create_dir_all(dir).unwrap();
        write_engine(dir);
    }
    std::fs::write(desktop.join(ENGINE_SELECTION), "engine-1").unwrap();
    let path = std::env::join_paths([&bin]).unwrap();
    let picked = custom.join(DIAL_NAME);
    let at = |flag: Option<&str>, env: Option<&str>, choice: Option<EngineChoice>| {
        locate_dial(flag.map(Into::into), env.map(Into::into), choice.as_ref(), Some(&path), Some(home))
    };
    let custom_choice = || Some(EngineChoice::Custom { path: picked.clone() });

    // The usual order: Desktop's copy, then PATH.
    assert_eq!(at(None, None, None), (copy.join(DIAL_NAME), EngineOrigin::Desktop));
    // A chosen binary beats Desktop's copy and PATH, but not DIAL_BIN or --dial.
    assert_eq!(at(None, None, custom_choice()), (picked.clone(), EngineOrigin::Choice));
    assert_eq!(at(None, Some("/opt/x/dial"), custom_choice()).1, EngineOrigin::Env);
    assert_eq!(at(Some("/opt/y/dial"), None, custom_choice()).1, EngineOrigin::Flag);
    // "Use the one on PATH" skips Desktop's copy; "Desktop's copy" is the usual first pick.
    assert_eq!(at(None, None, Some(EngineChoice::Path)), (bin.join(DIAL_NAME), EngineOrigin::Path));
    assert_eq!(at(None, None, Some(EngineChoice::Desktop)).1, EngineOrigin::Desktop);
    assert_eq!(choice_ignored(custom_choice().as_ref(), EngineOrigin::Choice), None);
    assert_eq!(choice_ignored(custom_choice().as_ref(), EngineOrigin::Env), None);

    // A chosen binary that is gone falls back to the usual order, with a warning.
    let gone = Some(EngineChoice::Custom { path: home.join("missing/dial") });
    let (_, origin) = at(None, None, gone.clone());
    assert_eq!(origin, EngineOrigin::Desktop);
    assert!(choice_ignored(gone.as_ref(), origin).is_some());
    // PATH asked for but empty: the usual order, with a warning.
    let empty = std::env::join_paths([home.join("nothing")]).unwrap();
    let origin = locate_dial(None, None, Some(&EngineChoice::Path), Some(&empty), Some(home)).1;
    assert_eq!(origin, EngineOrigin::Desktop);
    assert!(choice_ignored(Some(&EngineChoice::Path), origin).is_some());
}

#[test]
fn engine_choice_round_trips_and_clears() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("app");
    assert_eq!(load_choice(Some(&dir)), None);
    for choice in [
        EngineChoice::Custom { path: PathBuf::from("/opt/dial/bin/dial") },
        EngineChoice::Path,
        EngineChoice::Desktop,
    ] {
        save_choice(&dir, Some(&choice)).unwrap();
        assert_eq!(load_choice(Some(&dir)), Some(choice));
    }
    // The file the UI writes through serde: `{"kind":"custom","path":…}`.
    std::fs::write(dir.join(ENGINE_CHOICE), br#"{"kind":"custom","path":"/x/dial"}"#).unwrap();
    assert_eq!(load_choice(Some(&dir)), Some(EngineChoice::Custom { path: "/x/dial".into() }));
    std::fs::write(dir.join(ENGINE_CHOICE), b"not json").unwrap();
    assert_eq!(load_choice(Some(&dir)), None);
    save_choice(&dir, None).unwrap();
    save_choice(&dir, None).unwrap();
    assert!(!dir.join(ENGINE_CHOICE).exists());
}

#[cfg(unix)]
#[test]
fn engine_version_reads_the_first_line_or_says_why_not() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let script = root.path().join("dial");
    std::fs::write(&script, "#!/bin/sh\necho\necho 'dial 9.9.9'\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(engine_version(&script).unwrap(), "dial 9.9.9");
    std::fs::write(&script, "#!/bin/sh\nexit 3\n").unwrap();
    assert!(engine_version(&script).unwrap_err().contains("exited"));
    assert!(engine_version(&root.path().join("none")).unwrap_err().contains("not an executable"));
}

#[test]
fn windows_fallbacks_never_search_unix_system_directories() {
    assert!(fallback_dirs("windows").iter().all(|dir| dir.starts_with("~/")));
    assert!(!fallback_dirs("linux").contains(&"/opt/homebrew/bin"));
    assert!(fallback_dirs("macos").contains(&"/opt/homebrew/bin"));
}

#[cfg(unix)]
#[test]
fn failed_probe_says_why() {
    let failed = run_detailed(Command::new("/bin/sh").args(["-c", "exit 3"]), SHELL_TIMEOUT);
    assert!(failed.unwrap_err().starts_with("exited with"));
    let slow = run_detailed(Command::new("/bin/sh").args(["-c", "sleep 5"]), Duration::from_millis(100));
    assert_eq!(slow.unwrap_err(), "timed out after 0 s");
    let missing = run_detailed(&mut Command::new("/nonexistent/dial-probe"), SHELL_TIMEOUT);
    assert!(missing.unwrap_err().starts_with("could not start"));
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
