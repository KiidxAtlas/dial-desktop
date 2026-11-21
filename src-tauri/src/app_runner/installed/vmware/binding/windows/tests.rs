//! Fixture tests for the Windows probe's parsing and identity rules. Nothing here starts a system
//! tool: `netstat`, PowerShell and CIM are only exercised through their captured output.

use super::*;

const NETSTAT_FIXTURE: &str = "\
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       900
  TCP    127.0.0.1:903          0.0.0.0:0              LISTENING       4321
  TCP    [::1]:903              [::]:0                 LISTENING       4321
  TCP    127.0.0.1:903          127.0.0.1:50123        ESTABLISHED     4321
  TCP    127.0.0.1:50123        127.0.0.1:903          ESTABLISHED     777
  TCP    127.0.0.1:9000         0.0.0.0:0              LISTENING       555
  UDP    0.0.0.0:5353           *:*                                    1234
";

fn client() -> SocketAddr {
    "127.0.0.1:50123".parse().unwrap()
}

#[test]
fn netstat_rows_skip_headers_and_udp() {
    let sockets = parse_netstat(NETSTAT_FIXTURE).unwrap();
    assert_eq!(sockets.len(), 6);
    assert!(sockets.iter().all(|s| s.local.port() != 5353));
}

#[test]
fn malformed_tcp_row_is_an_error_not_a_skip() {
    let rows = [
        "  TCP    127.0.0.1:903          0.0.0.0:0              LISTENING",
        "  TCP    not-an-address:903     0.0.0.0:0              LISTENING       4321",
        "  TCP    127.0.0.1:903          0.0.0.0:0              LISTENING       pid",
    ];
    for row in rows {
        assert!(parse_netstat(row).is_err(), "accepted {row:?}");
    }
}

#[test]
fn listeners_are_every_row_on_exactly_the_port() {
    let sockets = parse_netstat(NETSTAT_FIXTURE).unwrap();
    let listeners = listeners_in(&sockets, 903);
    assert_eq!(
        listeners,
        vec![
            Listener {
                pid: 4321,
                host: "127.0.0.1".into()
            },
            Listener {
                pid: 4321,
                host: "::1".into()
            },
        ]
    );
    assert!(listeners_in(&sockets, 9001).is_empty());
}

#[test]
fn connection_owner_is_the_server_end_for_the_exact_client() {
    let sockets = parse_netstat(NETSTAT_FIXTURE).unwrap();
    let cases = [
        // (port, client, expected owner)
        (903, client(), Some(4321)),
        (903, "127.0.0.1:50124".parse().unwrap(), None),
        (904, client(), None),
    ];
    for (port, peer, expected) in cases {
        assert_eq!(
            owner_of(&sockets, port, peer),
            expected,
            "port {port}, {peer}"
        );
    }
}

#[test]
fn connection_owner_refuses_a_non_loopback_server_end() {
    let text = "\
  TCP    10.0.0.5:903           10.0.0.9:50123         ESTABLISHED     4321
";
    let sockets = parse_netstat(text).unwrap();
    let peer: SocketAddr = "10.0.0.9:50123".parse().unwrap();
    assert_eq!(owner_of(&sockets, 903, peer), None);
}

#[test]
fn connection_owner_refuses_two_different_owners() {
    let text = "\
  TCP    127.0.0.1:903          127.0.0.1:50123        ESTABLISHED     4321
  TCP    127.0.0.1:903          127.0.0.1:50123        ESTABLISHED     4322
";
    let sockets = parse_netstat(text).unwrap();
    assert_eq!(owner_of(&sockets, 903, client()), None);
}

#[test]
fn process_json_parses_paths_with_spaces_and_parentheses() {
    let text = r#"{"ExecutablePath":"C:\\Program Files (x86)\\VMware\\VMware Workstation\\vmware-vmx.exe","CommandLine":"\"C:\\Program Files (x86)\\VMware\\VMware Workstation\\vmware-vmx.exe\" \"D:\\VMs\\my vm\\a.vmx\""}"#;
    let process = parse_process(text).unwrap().unwrap();
    assert_eq!(
        process.executable_path.as_deref(),
        Some(r"C:\Program Files (x86)\VMware\VMware Workstation\vmware-vmx.exe")
    );
    assert!(
        process
            .command_line
            .unwrap()
            .ends_with(r#""D:\VMs\my vm\a.vmx""#)
    );
}

#[test]
fn process_json_tolerates_withheld_fields_and_missing_process() {
    let withheld = parse_process(r#"{"ExecutablePath":null,"CommandLine":null}"#)
        .unwrap()
        .unwrap();
    assert_eq!(withheld.executable_path, None);
    assert_eq!(withheld.command_line, None);
    assert_eq!(parse_process("").unwrap(), None);
    assert_eq!(parse_process("\r\n").unwrap(), None);
    assert!(parse_process("Get-CimInstance : Access denied").is_err());
}

#[test]
fn executable_with_parent_segment_is_refused() {
    let cases = [
        (
            r"C:\Program Files (x86)\VMware\VMware Workstation\vmware-vmx.exe",
            false,
        ),
        (
            r"C:\Program Files (x86)\VMware\VMware Workstation\..\..\evil\vmware-vmx.exe",
            true,
        ),
        (r"C:/Program Files/VMware/../evil/vmware-vmx.exe", true),
        (r"C:\VMware..backup\vmware-vmx.exe", false),
    ];
    for (path, refused) in cases {
        assert_eq!(has_parent_segment(path), refused, "{path}");
    }
}

#[test]
fn argument_match_is_one_whole_token() {
    let cases = [
        // (command line, vmx, matches)
        (
            r#""C:\VMware\vmware-vmx.exe" -# product=1 "D:\VMs\a.vmx""#,
            r"D:\VMs\a.vmx",
            true,
        ),
        // A quoted path with spaces is one token.
        (
            r#""C:\VMware\vmware-vmx.exe" "D:\My VMs\web box\a.vmx""#,
            r"D:\My VMs\web box\a.vmx",
            true,
        ),
        // Case and separator differences name the same file on Windows.
        (r#"vmware-vmx.exe "d:/vms/A.VMX""#, r"D:\VMs\a.vmx", true),
        // Substrings of a token never match.
        (
            r#"vmware-vmx.exe "D:\VMs\a.vmx.bak""#,
            r"D:\VMs\a.vmx",
            false,
        ),
        (
            r#"vmware-vmx.exe -fileName=D:\VMs\a.vmx"#,
            r"D:\VMs\a.vmx",
            false,
        ),
        (
            r#"vmware-vmx.exe "D:\VMs\old\a.vmx""#,
            r"D:\VMs\a.vmx",
            false,
        ),
        (r#"vmware-vmx.exe "D:\VMs\a.vmx""#, r"D:\VMs", false),
    ];
    for (command_line, vmx, matches) in cases {
        let found = argv(command_line)
            .unwrap()
            .iter()
            .any(|arg| same_path(arg, vmx));
        assert_eq!(found, matches, "{command_line} / {vmx}");
    }
}

#[test]
fn argv_splits_quotes_and_backslashes_like_the_c_runtime() {
    let args = argv(r#"tool "a b" c\"d e\\ "f\\\"g""#).unwrap();
    assert_eq!(args, vec!["tool", "a b", r#"c"d"#, r"e\\", r#"f\"g"#]);
}
