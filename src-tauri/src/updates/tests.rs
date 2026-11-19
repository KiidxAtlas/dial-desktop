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
