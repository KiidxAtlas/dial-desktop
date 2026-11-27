use super::*;

const DOCTOR: &str = "Installed (2):
  ✓ Bun                               /opt/homebrew/bin/bun
  ✓ llama.cpp (llama-server)          /Users/a b/.dial/tools/llama-cpp/llama-server

Not installed (2) — each line ends with the command that previews its setup:
  · HTML language server              dial doctor --fix vscode-html-language-server
  · helm-ls                           dial doctor --fix helm_ls

Memory: Semantic facts — Ready (optional steps left)
  Optional: Assign the memory role.
";

#[test]
fn doctor_lists_parse_and_the_rest_stays_as_notes() {
    let report = parse_doctor(DOCTOR);
    let caps = &report.capabilities;
    assert_eq!(caps.len(), 4);
    assert_eq!((caps[0].name.as_str(), caps[0].installed), ("Bun", true));
    assert_eq!(caps[0].path.as_deref(), Some("/opt/homebrew/bin/bun"));
    assert_eq!(caps[1].name, "llama.cpp (llama-server)");
    assert_eq!(
        caps[1].path.as_deref(),
        Some("/Users/a b/.dial/tools/llama-cpp/llama-server")
    );
    assert_eq!(
        (caps[2].id.as_str(), caps[2].name.as_str()),
        ("vscode-html-language-server", "HTML language server")
    );
    assert_eq!((caps[3].id.as_str(), caps[3].installed), ("helm_ls", false));
    assert_eq!(
        report.notes,
        [
            "Memory: Semantic facts — Ready (optional steps left)",
            "  Optional: Assign the memory role."
        ]
    );
}

#[test]
fn ids_cannot_smuggle_flags_or_shell_text() {
    for good in [
        "gopls",
        "helm_ls",
        "vscode-json-language-server",
        "espeak-ng",
    ] {
        assert!(valid_id(good), "{good}");
    }
    for bad in ["", "--yes", "a b", "a;b", "$(x)", "a/b"] {
        assert!(!valid_id(bad), "{bad}");
    }
    assert!(doctor_fix(Path::new("/nonexistent/dial"), None, "--yes", true).is_err());
}

#[test]
fn accounts_mark_the_active_one_and_skip_sentences() {
    let list = parse_accounts("* work  Work Person\n  home\n");
    assert_eq!(list.len(), 2);
    assert_eq!(
        (
            list[0].key.as_str(),
            list[0].detail.as_str(),
            list[0].active
        ),
        ("work", "Work Person", true)
    );
    assert_eq!((list[1].key.as_str(), list[1].active), ("home", false));
    assert!(parse_accounts("no stored copilot accounts\n").is_empty());
}

#[test]
fn structured_doctor_json_preserves_ids_paths_and_notes() {
    let report = parse_doctor_json(
        r#"{
            "capabilities": [
                {"id": "bun", "name": "Bun", "installed": true, "path": "/opt/homebrew/bin/bun"},
                {"id": "gopls", "name": "Go language server", "installed": false, "path": null}
            ],
            "notes": ["Memory: local"]
        }"#,
    )
    .unwrap();
    assert_eq!(report.capabilities.len(), 2);
    assert_eq!(report.capabilities[0].id, "bun");
    assert!(report.capabilities[0].installed);
    assert_eq!(
        report.capabilities[0].path.as_deref(),
        Some("/opt/homebrew/bin/bun")
    );
    assert_eq!(report.capabilities[1].id, "gopls");
    assert!(!report.capabilities[1].installed);
    assert_eq!(report.capabilities[1].path, None);
    assert_eq!(report.notes, ["Memory: local"]);
}

#[test]
fn structured_accounts_json_maps_labels_active_and_cooling() {
    let list = parse_accounts_json(
        r#"[
            {"key": "a1", "label": "one@example.com", "active": true, "cooling": false},
            {"key": "a2", "label": "two@example.com", "active": false, "cooling": true},
            {"key": "a3", "label": "", "active": false, "cooling": true}
        ]"#,
    )
    .unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(
        (list[0].key.as_str(), list[0].detail.as_str(), list[0].active),
        ("a1", "one@example.com", true)
    );
    assert_eq!(
        (list[1].key.as_str(), list[1].detail.as_str(), list[1].active),
        ("a2", "two@example.com (cooling down)", false)
    );
    assert_eq!(
        (list[2].key.as_str(), list[2].detail.as_str(), list[2].active),
        ("a3", "(cooling down)", false)
    );
}

#[test]
fn link_paths_follow_the_platform() {
    let home = if cfg!(windows) {
        Path::new("C:\\Users\\me")
    } else {
        Path::new("/home/me")
    };
    assert_eq!(
        link_path("linux", Some(home), None),
        Some(home.join(".local/bin/dial"))
    );
    assert_eq!(link_path("macos", None, None), None);
    assert_eq!(
        link_path("windows", Some(home), Some(home)),
        Some(home.join("Microsoft").join("WindowsApps").join("dial.cmd"))
    );
    assert_eq!(link_path("windows", Some(home), None), None);
}

fn executable_at(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn linking_never_replaces_a_foreign_binary() {
    let root = tempfile::tempdir().unwrap();
    let engine = root.path().join("engine");
    executable_at(&engine, b"engine");
    let bin = root.path().join("bin");
    let link = bin.join("dial");

    let before = link_status(&engine, &link, std::slice::from_ref(&bin));
    assert!(!before.linked && !before.blocked && before.other.is_none());
    link_install(&engine, &link).unwrap();
    let after = link_status(&engine, &link, std::slice::from_ref(&bin));
    assert!(after.linked && after.managed && after.on_path && after.other.is_none());
    link_install(&engine, &link).unwrap();
    link_remove(&link).unwrap();
    assert!(!link.exists());

    std::fs::write(&link, b"someone else's dial").unwrap();
    let foreign = link_status(&engine, &link, std::slice::from_ref(&bin));
    assert!(foreign.blocked && !foreign.linked);
    assert_eq!(foreign.other.as_deref(), Some(link.as_path()));
    assert!(link_install(&engine, &link).is_err());
    assert!(link_remove(&link).is_err());
    assert_eq!(std::fs::read(&link).unwrap(), b"someone else's dial");

    // The standalone executable itself may be Desktop's selected engine.
    executable_at(&link, b"separate install");
    let same = link_status(&link, &link, &[]);
    assert!(same.linked && same.blocked && !same.managed);
    assert!(link_install(&link, &link).is_err());
    assert!(link_remove(&link).is_err());
    assert_eq!(std::fs::read(&link).unwrap(), b"separate install");
}

#[cfg(unix)]
#[test]
fn a_replacement_symlink_never_inherits_ownership() {
    let root = tempfile::tempdir().unwrap();
    let engine = root.path().join("engine");
    let foreign = root.path().join("somebody-elses");
    let missing = root.path().join("no-longer-installed");
    executable_at(&engine, b"engine");
    executable_at(&foreign, b"foreign");
    for name in ["dial", "dial.cmd"] {
        let link = root.path().join(name);
        link_install(&engine, &link).unwrap();
        let previous = root.path().join(format!("old-{name}"));
        std::fs::rename(&link, &previous).unwrap();
        for target in [&engine, &foreign, &missing, &previous] {
            std::os::unix::fs::symlink(target, &link).unwrap();
            let state = link_status(&engine, &link, &[]);
            assert!(state.blocked && !state.managed);
            if target == &engine { assert!(state.linked); }
            assert!(link_install(&engine, &link).is_err());
            assert!(link_remove(&link).is_err());
            assert_eq!(std::fs::read_link(&link).unwrap(), *target);
            std::fs::remove_file(&link).unwrap();
        }
    }
}

#[test]
fn command_files_survive_engine_updates_but_modified_commands_are_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old engine.exe");
    let engine = root.path().join("new %user% engine.exe");
    let link = root.path().join("dial.cmd");
    executable_at(&engine, b"engine");
    // Exact shim installed by previous Desktop releases; its engine has already gone.
    let legacy = format!("@rem Dial Desktop command-line link\r\n@\"{}\" %*\r\n", old.display());
    std::fs::write(&link, &legacy).unwrap();
    assert!(link_status(&engine, &link, &[]).managed);
    link_install(&engine, &link).unwrap();
    let current = link_status(&engine, &link, &[]);
    assert!(current.managed && current.linked && !current.blocked);
    link_remove(&link).unwrap();
    assert!(!link.exists());

    let modified = format!("{legacy}@echo user command\r\n");
    std::fs::write(&link, &modified).unwrap();
    assert!(link_status(&engine, &link, &[]).blocked);
    assert!(link_install(&engine, &link).is_err());
    assert!(link_remove(&link).is_err());
    assert_eq!(std::fs::read_to_string(link).unwrap(), modified);
}

#[cfg(unix)]
#[test]
fn unix_command_retargets_and_forwards_literal_arguments() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old");
    let engine = root.path().join("new engine's $build");
    let link = root.path().join("dial");
    executable_at(&old, b"old");
    executable_at(&engine, b"#!/bin/sh\nprintf '%s\\n' \"$@\"\n");
    link_install(&old, &link).unwrap();
    std::fs::remove_file(old).unwrap();
    link_install(&engine, &link).unwrap();
    let out = Command::new(&link).args(["one two", "$(false)", ""]).output().unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, b"one two\n$(false)\n\n");
    link_remove(&link).unwrap();
}

#[test]
fn the_path_line_goes_to_the_login_shells_profile_once() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let bin = home.join(".local/bin");
    let profile = |os, shell| shell_path(os, shell, home, None, &bin).map(|s| s.profile);
    assert_eq!(profile("macos", "/bin/zsh"), Some(home.join(".zshrc")));
    assert_eq!(profile("linux", "/usr/bin/bash"), Some(home.join(".bashrc")));
    assert_eq!(profile("macos", "/bin/bash"), Some(home.join(".bash_profile")));
    assert_eq!(profile("linux", "fish"), Some(home.join(".config/fish/config.fish")));
    // An existing `.profile` is what a macOS login bash reads; a new `.bash_profile` would hide it.
    std::fs::write(home.join(".profile"), "export A=1").unwrap();
    assert_eq!(profile("macos", "/bin/bash"), Some(home.join(".profile")));
    // Unknown shells, Windows and other folders are never edited.
    assert_eq!(profile("linux", "/usr/bin/nu"), None);
    assert_eq!(profile("windows", "bash"), None);
    assert_eq!(shell_path("linux", "zsh", home, None, &home.join("bin")), None);
    let custom = home.join("zsh");
    assert_eq!(shell_path("linux", "zsh", home, Some(&custom), &bin).unwrap().profile, custom.join(".zshrc"));

    let setup = shell_path("macos", "/bin/bash", home, None, &bin).unwrap();
    assert!(!setup.present);
    shell_path_add(&setup).unwrap();
    shell_path_add(&setup).unwrap();
    let text = std::fs::read_to_string(home.join(".profile")).unwrap();
    assert!(text.starts_with("export A=1\n\n# Added by Dial Desktop"), "{text}");
    assert_eq!(text.matches(&setup.line).count(), 1);
    assert!(shell_path("macos", "/bin/bash", home, None, &bin).unwrap().present);

    let fish = shell_path("linux", "fish", home, None, &bin).unwrap();
    shell_path_add(&fish).unwrap();
    assert!(std::fs::read_to_string(&fish.profile).unwrap().ends_with("fish_add_path \"$HOME/.local/bin\"\n"));
}

#[test]
fn reports_run_only_fixed_subcommands_and_apply_only_where_asked() {
    let args =
        |kind, apply, target| report_args(kind, apply, target).map(|(a, c)| (a.join(" "), c));
    assert_eq!(
        args("gc", false, None).unwrap(),
        ("dev gc --json".to_owned(), false)
    );
    assert_eq!(
        args("gc", true, None).unwrap(),
        ("dev gc --json --apply".to_owned(), true)
    );
    assert_eq!(
        args("images", true, None).unwrap(),
        ("images status --json".to_owned(), false)
    );
    assert_eq!(
        args("images_purge", true, None).unwrap(),
        ("images purge --json --apply".to_owned(), true)
    );
    assert_eq!(
        args("ps_stop", false, Some("4242")).unwrap(),
        ("ps stop 4242 --json".to_owned(), true)
    );
    assert_eq!(
        args("ps_logs", true, Some("4242")).unwrap(),
        ("ps logs 4242 --json".to_owned(), false)
    );
    assert_eq!(
        args("ps_restart", false, Some("4242")).unwrap(),
        ("ps restart 4242 --json".to_owned(), true)
    );
    for bad in [
        None,
        Some(""),
        Some("-1"),
        Some("12 --all"),
        Some("12345678901"),
    ] {
        assert!(report_args("ps_kill", false, bad).is_err(), "{bad:?}");
    }
    assert!(report_args("auth", false, None).is_err());
}

#[test]
fn commit_previews_keep_the_message_and_split_off_the_warning() {
    let plain = parse_commit_preview("Dry run. Message:\nfeat(ui): add hooks page\n\nLists hooks.\n").unwrap();
    assert_eq!(plain.message, "feat(ui): add hooks page\n\nLists hooks.");
    assert_eq!(plain.warning, None);
    let warned = parse_commit_preview("noise\nDry run. Message:\nfix: x\n\nWarning: subject too long\n").unwrap();
    assert_eq!(warned.message, "fix: x");
    assert_eq!(warned.warning.as_deref(), Some("subject too long"));
    assert_eq!(parse_commit_preview("No changes to commit.\n").unwrap_err(), "There are no changes to describe");
    assert!(parse_commit_preview("Dry run. Message:\n  \n").is_err());
    assert!(parse_commit_preview("").is_err());

    let json_ok = parse_commit_preview_json(r#"{"kind":"dry_run","message":"feat: json","warning":"short"}"#)
        .unwrap()
        .unwrap();
    assert_eq!(json_ok.message, "feat: json");
    assert_eq!(json_ok.warning.as_deref(), Some("short"));
    let json_empty = parse_commit_preview_json(r#"{"kind":"nothing_to_commit","pushed":false}"#)
        .unwrap()
        .unwrap_err();
    assert_eq!(json_empty, "There are no changes to describe");
}

#[test]
fn reviews_take_fixed_flags_and_never_read_refs_or_focus_as_options() {
    let report = Path::new("/tmp/r.md");
    let args = review_args(&ReviewTarget::Base("main".into()), "  auth  ", report).unwrap();
    assert_eq!(args, ["review", "--output-format", "text", "--base", "main", "--output-last-message", "/tmp/r.md", "--", "auth"]);
    let args = review_args(&ReviewTarget::Uncommitted, "", report).unwrap();
    assert!(args.contains(&"--uncommitted".to_owned()) && !args.contains(&"--".to_owned()));
    for bad in ["--exec=x", "", "a b", "x\ny"] {
        assert!(review_args(&ReviewTarget::Commit(bad.into()), "", report).is_err(), "{bad:?}");
    }
}

#[test]
fn share_output_yields_the_link_and_its_notes() {
    let link = parse_share("Share URL: https://share.example/s/abc#key\nNote: the key is only in the URL fragment.\nTrimmed to fit the size cap: images.\n").unwrap();
    assert_eq!(link.url, "https://share.example/s/abc#key");
    assert_eq!(link.notes, ["the key is only in the URL fragment.", "Trimmed to fit the size cap: images."]);
    let bare = parse_share("https://gist.github.com/u/123#k\n").unwrap();
    assert_eq!(bare.url, "https://gist.github.com/u/123#k");
    assert!(bare.notes.is_empty());
    let json = parse_share_json(r#"{"url":"https://share.example/s/xyz#k","notes":["fragment only"]}"#).unwrap();
    assert_eq!(json.url, "https://share.example/s/xyz#k");
    assert_eq!(json.notes, ["fragment only"]);
    assert!(parse_share_json(r#"{"url":"javascript:alert(1)","notes":[]}"#).is_err());
    assert!(parse_share("").is_err());
    assert_eq!(parse_share("gh: not logged in").unwrap_err(), "gh: not logged in");
}
#[test]
fn understanding_queries_are_words_and_relays_need_an_address() {
    let (args, changes) = report_args("understanding", false, Some("architecture")).unwrap();
    assert_eq!(args, ["understanding", "architecture", "--json"]);
    assert!(!changes);
    assert!(report_args("understanding", false, Some("--dir=/")).is_err());
    assert_eq!(report_args("experience", false, None).unwrap().0, ["experience", "--json"]);
    assert!(RelayManager::default().start(Path::new("/nonexistent/dial"), None, "not an address").is_err());
}

#[cfg(unix)]
#[test]
fn json_fallback_only_retries_when_engine_rejects_json_flag() {
    let root = tempfile::tempdir().unwrap();
    let calls = root.path().join("calls");
    let failing = root.path().join("dial-fail");
    executable_at(
        &failing,
        format!(
            "#!/bin/sh\necho x >> '{}'\necho 'network timeout' >&2\nexit 1\n",
            calls.display()
        )
        .as_bytes(),
    );
    let err = commit_message(&failing, root.path()).unwrap_err();
    assert!(err.contains("network timeout"), "{err}");
    assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 1);

    std::fs::remove_file(&calls).unwrap();
    let err = share(&failing, root.path(), "sess1", false).unwrap_err();
    assert!(err.contains("network timeout"), "{err}");
    assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 1);

    // An older engine rejecting `--json` falls back cleanly once to legacy output.
    let legacy = root.path().join("dial-legacy");
    executable_at(
        &legacy,
        format!(
            "#!/bin/sh\necho x >> '{}'\nfor a in \"$@\"; do\n  if [ \"$a\" = \"--json\" ]; then\n    echo \"error: unexpected argument '--json' found\" >&2\n    exit 2\n  fi\ndone\nprintf 'Share URL: https://share.example/s/ok#k\\n'\n",
            calls.display()
        )
        .as_bytes(),
    );
    std::fs::remove_file(&calls).unwrap();
    let link = share(&legacy, root.path(), "sess1", false).unwrap();
    assert_eq!(link.url, "https://share.example/s/ok#k");
    assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 2);
}
