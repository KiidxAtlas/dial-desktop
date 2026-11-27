use super::*;

#[test]
fn labels_are_stable_per_project() {
    assert_eq!(label("/a"), label("/a"));
    assert_ne!(label("/a"), label("/b"));
    assert!(label("/a").starts_with("dev.dial.desktop.schedule."));
    assert_eq!(label("/a").len(), "dev.dial.desktop.schedule.".len() + 16);
}

#[test]
fn launch_agent_runs_the_project_scheduler_and_escapes_text() {
    let plist = launchd_plist(
        "x",
        Path::new("/opt/d&l/dial"),
        "/p <1>",
        Some("/bin:/usr/bin"),
        "/l/x.log",
    );
    assert!(plist.contains("<string>/opt/d&amp;l/dial</string>\n    <string>schedule</string>\n    <string>run</string>\n    <string>-C</string>\n    <string>/p &lt;1&gt;</string>"));
    assert!(plist.contains("<key>PATH</key>\n    <string>/bin:/usr/bin</string>"));
    assert!(plist.contains("<key>SuccessfulExit</key>\n    <false/>"));
    assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
    assert!(
        !launchd_plist("x", Path::new("/d"), "/p", None, "/l").contains("EnvironmentVariables")
    );
}

#[test]
fn systemd_unit_quotes_paths_and_specifiers() {
    let unit = systemd_unit(Path::new("/opt/my dial/dial"), "/p/50% \"x\"", Some("/bin"));
    assert!(
        unit.contains("ExecStart=\"/opt/my dial/dial\" schedule run -C \"/p/50%% \\\"x\\\"\"\n")
    );
    assert!(unit.contains("WorkingDirectory=\"/p/50%% \\\"x\\\"\"\n"));
    assert!(unit.contains("Environment=\"PATH=/bin\"\n"));
    assert!(unit.contains("KillSignal=SIGINT"));
    assert!(unit.contains("WantedBy=default.target"));

    // Newlines and carriage returns cannot inject extra unit directives.
    let injected = systemd_unit(
        Path::new("/opt/dial"),
        "/p/a\nExecStartPre=/bin/evil\r",
        Some("/bin\nUser=root"),
    );
    assert!(!injected.contains("\nExecStartPre="));
    assert!(!injected.contains("\nUser=root"));
    assert!(injected.contains("Description=Dial schedules for /p/a ExecStartPre=/bin/evil \n"));
}

#[test]
fn definitions_live_where_each_manager_looks() {
    let paths = Paths {
        home: "/h".into(),
        config: None,
        app_dir: "/h/app".into(),
    };
    assert_eq!(
        paths.definition(Manager::Launchd, "l"),
        Path::new("/h/Library/LaunchAgents/l.plist")
    );
    assert_eq!(
        paths.definition(Manager::Systemd, "l"),
        Path::new("/h/.config/systemd/user/l.service")
    );
    let xdg = Paths {
        config: Some("/x".into()),
        ..paths.clone()
    };
    assert_eq!(
        xdg.definition(Manager::Systemd, "l"),
        Path::new("/x/systemd/user/l.service")
    );
    assert_eq!(
        paths.log(Manager::Launchd, "l"),
        "/h/app/schedule-logs/l.log"
    );
    assert_eq!(Manager::for_os("windows"), None);
}

#[test]
fn installed_state_follows_the_definition_file() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths {
        home: dir.path().into(),
        config: None,
        app_dir: dir.path().join("app"),
    };
    assert!(!status_in(Manager::Launchd, &paths, "/p").installed);
    let file = paths.definition(Manager::Launchd, &label("/p"));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "x").unwrap();
    assert!(status_in(Manager::Launchd, &paths, "/p").installed);
}

#[test]
fn refuses_engines_that_would_not_outlive_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap();
    assert!(
        validate(Path::new("dial"), root)
            .unwrap_err()
            .contains("not an installed executable")
    );
    assert!(validate(Path::new("/bin/sh"), "relative").is_err());
    assert!(validate(Path::new("/bin/sh"), root).is_ok());
}

/// Registers a real LaunchAgent for a temporary project: `cargo test -- --ignored live_launchd`.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "changes ~/Library/LaunchAgents"]
fn live_launchd_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let root = root.to_str().unwrap();
    std::fs::write(
        Path::new(root).join("schedule"),
        "trap 'exit 0' INT\nwhile :; do sleep 0.2; done\n",
    )
    .unwrap();
    let status = install(Path::new("/bin/sh"), root).unwrap();
    assert!(status.installed && installed(root));
    let target = format!("{}/{}", gui_domain(), label(root));
    let mut running = false;
    for _ in 0..50 {
        let out = crate::updates::run_bounded(
            Command::new("launchctl").args(["print", &target]),
            MANAGER_TIMEOUT,
        )
        .unwrap();
        if out.stdout.contains("pid = ") {
            running = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let removed = remove(root).unwrap();
    assert!(running, "launchd started the scheduler");
    assert!(!removed.installed && !installed(root));
    let out = crate::updates::run_bounded(
        Command::new("launchctl").args(["print", &target]),
        MANAGER_TIMEOUT,
    )
    .unwrap();
    assert!(!out.success, "unloaded");
}
