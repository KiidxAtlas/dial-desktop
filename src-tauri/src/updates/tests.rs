use super::*;

fn ran(success: bool, stdout: &str, stderr: &str) -> Result<RunOutput, String> {
    Ok(RunOutput {
        success,
        stdout: stdout.into(),
        stderr: stderr.into(),
    })
}

#[test]
fn a_check_answer_is_read_and_unknown_fields_are_ignored() {
    let json = r#"{"current":"0.3.10","latest":"0.3.11","update_available":true,
        "installation":"installer","method":"release","can_update":true,
        "update_command":"dial update","error":null,"added_later":1}"#;
    let EngineUpdateView::Checked { status } = parse_check(ran(true, json, "")) else {
        panic!("not checked");
    };
    assert_eq!(status.latest.as_deref(), Some("0.3.11"));
    assert!(status.update_available && status.can_update);
    assert_eq!(status.method, "release");

    // A field the engine has not announced yet defaults.
    let EngineUpdateView::Checked { status } =
        parse_check(ran(true, r#"{"current":"0.3.10"}"#, ""))
    else {
        panic!("not checked");
    };
    assert!(!status.update_available && status.latest.is_none());
}

#[test]
fn engines_before_update_checks_are_offered_an_update() {
    let stderr = "error: unexpected argument '--check' found\n\nUsage: dial update [OPTIONS]";
    assert!(matches!(
        parse_check(ran(false, "", stderr)),
        EngineUpdateView::Unsupported { .. }
    ));
}

#[test]
fn a_missing_engine_and_a_broken_answer_are_told_apart() {
    assert!(matches!(
        parse_check(Err(
            "could not start \"dial\": No such file or directory".into()
        )),
        EngineUpdateView::Missing { .. }
    ));
    for error in [
        "could not start dial: Permission denied",
        "dial did not finish within 30s",
    ] {
        assert!(matches!(
            parse_check(Err(error.into())),
            EngineUpdateView::Failed { .. }
        ));
    }
    assert!(matches!(
        parse_check(ran(true, "not json", "")),
        EngineUpdateView::Failed { .. }
    ));
    let EngineUpdateView::Failed { error } = parse_check(ran(false, "", "boom")) else {
        panic!("not failed");
    };
    assert_eq!(error, "boom");
}

#[test]
fn connect_failures_name_their_fix() {
    assert_eq!(
        classify_failure("could not start dial: No such file or directory (os error 2)"),
        EngineProblem::Missing
    );
    assert_eq!(
        classify_failure(
            "could not start dial.exe: The system cannot find the file specified. (os error 2)"
        ),
        EngineProblem::Missing
    );
    assert_eq!(
        classify_failure("handshake failed: engine serves protocol [3]; client implements [1, 2]"),
        EngineProblem::Incompatible
    );
    assert_eq!(
        classify_failure("handshake failed: engine exited before `ready`"),
        EngineProblem::Other
    );
    // A permission error starting the binary is not a missing engine.
    assert_eq!(
        classify_failure("could not start dial: Permission denied (os error 13)"),
        EngineProblem::Other
    );
}

#[test]
fn launch_refusals_the_system_names_get_a_next_step() {
    let hint = |error, windows| failure_hint(error, windows).unwrap_or_default();
    assert!(hint("could not start dial.exe: An Application Control policy has blocked this file. (os error 4551)", true).contains("Smart App Control"));
    assert!(hint("could not start dial.exe: %1 is not a valid Win32 application. (os error 193)", true).contains("different kind of computer"));
    assert!(hint("could not start dial: Exec format error (os error 8)", false).contains("different kind of computer"));
    assert!(hint("could not start dial: Bad CPU type in executable (os error 86)", false).contains("different kind of computer"));
    assert!(hint("could not start dial.exe: The requested operation requires elevation. (os error 740)", true).contains("administrator"));
    assert!(hint("could not start dial: Permission denied (os error 13)", false).contains("refused to run"));
    assert!(hint("could not start dial.exe: Access is denied. (os error 5)", true).contains("refused to run"));
    // The same numbers mean other things on the other platform, and on a running engine.
    assert_eq!(failure_hint("could not start dial: Input/output error (os error 5)", false), None);
    assert_eq!(failure_hint("could not start dial.exe: The data is invalid. (os error 13)", true), None);
    assert_eq!(failure_hint("handshake failed: Permission denied (os error 13)", false), None);
    assert_eq!(failure_hint("could not start dial: No such file or directory (os error 2)", false), None);
}

#[test]
fn the_installer_is_the_signed_release_installer_for_each_platform() {
    let (program, args) = install_command(false);
    assert_eq!(program, "sh");
    assert_eq!(
        args,
        ["-c", "curl -fsSL https://dial.rootatlas.dev/install | sh"]
    );
    let (program, args) = install_command(true);
    assert_eq!(program, "powershell");
    assert_eq!(
        args.last().copied(),
        Some("irm https://dial.rootatlas.dev/install | iex")
    );
}

#[test]
fn desktop_install_selects_only_its_own_completed_binary() {
    use crate::launcher::{DIAL_NAME, desktop_engine};
    let root = tempfile::tempdir().unwrap();
    let external = root.path().join(DIAL_NAME);
    std::fs::write(&external, b"existing").unwrap();
    let (installed, _) = install_separate(root.path(), |dir| {
        std::fs::write(dir.join(DIAL_NAME), b"release").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.join(DIAL_NAME), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Ok("installed".into())
    })
    .unwrap();
    assert_eq!(desktop_engine(root.path()), Some(installed.clone()));
    // A failed command and a successful command that didn't install here must leave the selection alone.
    for outcome in [
        Err("interrupted".to_owned()),
        Ok("nothing installed".to_owned()),
    ] {
        assert!(install_separate(root.path(), |_| outcome).is_err());
        assert_eq!(desktop_engine(root.path()), Some(installed.clone()));
    }
    #[cfg(unix)]
    {
        assert!(install_separate(root.path(), |dir| {
            std::fs::write(dir.join(DIAL_NAME), b"not executable").unwrap();
            Ok("installed".into())
        }).is_err());
        assert_eq!(desktop_engine(root.path()), Some(installed));
    }
    assert_eq!(std::fs::read(external).unwrap(), b"existing");
}

#[test]
fn long_output_keeps_whole_last_lines() {
    assert_eq!(last_lines("  short \n", 100), "short");
    let text = "first line\nsecond line\nthird";
    assert_eq!(last_lines(text, 14), "third");
    assert_eq!(last_lines("ééééé\nlast", 7), "last");
}

#[cfg(unix)]
#[test]
fn bounded_runs_capture_both_streams_and_stop_at_the_deadline() {
    let out = run_bounded(
        Command::new("sh").args(["-c", "echo out; echo err >&2; exit 3"]),
        Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(
        (out.success, out.stdout.trim(), out.stderr.trim()),
        (false, "out", "err")
    );
    let error = run_bounded(
        Command::new("sh").args(["-c", "sleep 5"]),
        Duration::from_millis(200),
    )
    .unwrap_err();
    assert!(error.contains("did not finish"), "{error}");
    assert!(
        run_bounded(
            &mut Command::new("/nonexistent/dial"),
            Duration::from_secs(1)
        )
        .unwrap_err()
        .starts_with("could not start")
    );
}

#[test]
fn only_a_configured_public_key_enables_app_updates() {
    let config = |value: serde_json::Value| updater_configured(Some(&value));
    assert!(config(serde_json::json!({ "pubkey": "dW50cnVzdGVk" })));
    assert!(!config(serde_json::json!({ "pubkey": "" })));
    assert!(!config(serde_json::json!({ "pubkey": "  " })));
    assert!(!config(serde_json::json!({ "endpoints": [] })));
    assert!(!updater_configured(None));
}
