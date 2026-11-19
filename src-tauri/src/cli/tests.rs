use super::*;

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

#[test]
fn defaults_to_a_live_engine_in_the_current_directory() {
    assert_eq!(parse(args(&[])).unwrap(), Cli::default());
}

#[test]
fn reads_every_option_and_forwards_rpc_arguments() {
    let cli = parse(args(&[
        "--dial",
        "/opt/dial",
        "--preview=empty",
        "~/code/app",
        "--",
        "--model",
        "local",
    ]))
    .unwrap();
    assert_eq!(cli.dial, Some("/opt/dial".into()));
    assert_eq!(cli.preview, Some(Preview::Empty));
    assert_eq!(cli.workspace, Some("~/code/app".into()));
    assert_eq!(cli.rpc_args, args(&["--model", "local"]));
}

#[test]
fn rejects_unknown_options_missing_values_and_extra_paths() {
    assert!(parse(args(&["--nope"])).unwrap_err().contains("--nope"));
    assert!(parse(args(&["--dial"])).is_err());
    assert!(parse(args(&["a", "b"])).unwrap_err().contains('b'));
}
