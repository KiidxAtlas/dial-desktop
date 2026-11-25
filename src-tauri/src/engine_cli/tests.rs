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

#[cfg(unix)]
#[test]
fn linking_never_replaces_a_foreign_binary() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let engine = root.path().join("engine/dial");
    std::fs::create_dir_all(engine.parent().unwrap()).unwrap();
    std::fs::write(&engine, b"engine").unwrap();
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
    let bin = root.path().join("bin");
    let link = bin.join("dial");

    let before = link_status(&engine, &link, std::slice::from_ref(&bin));
    assert!(!before.linked && !before.blocked && before.other.is_none());
    link_install(&engine, &link).unwrap();
    let after = link_status(&engine, &link, std::slice::from_ref(&bin));
    assert!(after.linked && after.on_path && after.other.is_none());
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
    assert!(relay_start(Path::new("/nonexistent/dial"), None, "not an address").is_err());
}
