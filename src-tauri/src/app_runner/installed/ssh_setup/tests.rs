use std::net::Ipv4Addr;
use std::path::Path;

use super::*;

const MAC: [u8; 6] = [0x08, 0x00, 0x27, 0x9b, 0xf5, 0xe9];

#[test]
fn macs_parse_in_every_form() {
    for text in ["0800279BF5E9", "08:00:27:9b:f5:e9", "8:0:27:9b:f5:e9", "08-00-27-9B-F5-E9"] {
        assert_eq!(parse_mac(text), Some(MAC), "{text}");
    }
    assert_eq!(parse_mac("08:00:27:9b:f5"), None);
    assert_eq!(parse_mac("zz00279BF5E9"), None);
}

#[test]
fn bridged_nat_and_forward_are_told_apart() {
    let bridged = "name=\"kali\"\nnic1=\"bridged\"\nmacaddress1=\"0800279BF5E9\"\nnic2=\"none\"\n";
    assert_eq!(reach_from_vminfo(bridged), Ok(Reach::Lan { mac: MAC }));
    let nat = "nic1=\"nat\"\nmacaddress1=\"0800279BF5E9\"\n";
    assert_eq!(reach_from_vminfo(nat), Ok(Reach::NatNeedsForward { nic: 1 }));
    let fwd = "nic1=\"nat\"\nForwarding(0)=\"web,tcp,,8080,,80\"\nForwarding(1)=\"ssh,tcp,127.0.0.1,2222,,22\"\n";
    assert_eq!(reach_from_vminfo(fwd), Ok(Reach::Forward { port: 2222 }));
    assert!(reach_from_vminfo("nic1=\"none\"\n").is_err());
}

#[test]
fn arp_lookup_handles_macos_linux_and_windows_output() {
    let mac_os = "? (192.168.1.1) at 0:11:22:33:44:55 on en0 ifscope [ethernet]\n? (192.168.1.57) at 8:0:27:9b:f5:e9 on en0 ifscope [ethernet]\n";
    assert_eq!(ip_for_mac(mac_os, MAC), Some(Ipv4Addr::new(192, 168, 1, 57)));
    let linux = "IP address       HW type     Flags       HW address            Mask     Device\n10.0.0.9         0x1         0x2         08:00:27:9b:f5:e9     *        eth0\n";
    assert_eq!(ip_for_mac(linux, MAC), Some(Ipv4Addr::new(10, 0, 0, 9)));
    let windows = "  Internet Address      Physical Address      Type\n  192.168.0.4           08-00-27-9b-f5-e9     dynamic\n";
    assert_eq!(ip_for_mac(windows, MAC), Some(Ipv4Addr::new(192, 168, 0, 4)));
    assert_eq!(ip_for_mac(mac_os, [1, 2, 3, 4, 5, 6]), None);
}

#[test]
fn aliases_and_users_are_safe() {
    assert_eq!(alias_for("kali"), "kali");
    assert_eq!(alias_for("  Kali Linux 2025 (ARM) "), "kali-linux-2025-arm");
    assert_eq!(alias_for("***"), "vm");
    assert!(ssh::valid_alias(&alias_for("Ünïcode VM!")));
    assert!(valid_user("atlas") && valid_user("kali_user.1"));
    for bad in ["", "-oProxy", "a b", "a;b", "root'"] {
        assert!(!valid_user(bad), "{bad}");
    }
}

#[test]
fn the_guest_command_quotes_the_key_and_refuses_odd_keys() {
    let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample dial";
    let cmd = guest_command(key, Some(Ipv4Addr::new(192, 168, 1, 10)), None).unwrap();
    assert!(cmd.starts_with("sudo sh -c '"));
    assert!(cmd.contains(&format!("grep -qxF '{key}' ~/.ssh/authorized_keys")));
    assert!(cmd.contains("ping -c 3 -W 1 192.168.1.10"));
    assert!(cmd.ends_with("echo DIAL-SSH-READY\n"));
    assert_eq!(cmd.matches('\n').count(), 1);
    assert!(cmd.is_ascii());
    assert!(!guest_command(key, None, None).unwrap().contains("ping"));
    assert!(guest_command("ssh-ed25519 AAA x'; rm -rf ~", None, None).is_err());
    assert!(guest_command("ssh-rsa AAA", None, None).is_err());
}

#[test]
fn the_guest_reports_its_address_with_the_token_after_the_key_is_added() {
    let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample dial";
    let report = Report { addr: "10.0.0.92:50123".parse().unwrap(), token: "ab12".into() };
    let cmd = guest_command(key, Some(Ipv4Addr::new(10, 0, 0, 92)), Some(&report)).unwrap();
    let send = "&& (timeout 5 bash -c \"echo ab12 >/dev/tcp/10.0.0.92/50123\") >/dev/null 2>&1";
    assert!(cmd.contains(&format!("chmod 600 ~/.ssh/authorized_keys {send}; ping")), "{cmd}");
    assert_eq!(cmd.matches('\n').count(), 1);
    for bad in ["", "a b", "x\";rm", "t'"] {
        let report = Report { token: bad.into(), ..report.clone() };
        assert!(guest_command(key, None, Some(&report)).is_err(), "{bad}");
    }
    assert!(is_report(b"ab12\n", "ab12") && is_report(b"ab12", "ab12"));
    assert!(!is_report(b"ab13\n", "ab12") && !is_report(b"", "") && !is_report(b"\xffab12", "ab12"));
}

#[test]
fn the_report_listener_takes_the_peer_address_only_with_the_token() {
    use std::io::Write;
    let listener = ReportListener::open(Ipv4Addr::LOCALHOST).unwrap();
    assert_eq!(listener.poll(), None);
    let send = |text: &str| {
        let mut s = std::net::TcpStream::connect(listener.report.addr).unwrap();
        s.write_all(text.as_bytes()).unwrap();
    };
    send("wrong\n");
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(listener.poll(), None);
    send(&format!("{}\n", listener.report.token));
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(listener.poll(), Some(Ipv4Addr::LOCALHOST));
}

#[test]
fn the_config_block_is_prepended_replaced_and_leaves_the_rest() {
    let key = Path::new("/Users/me/.ssh/dial_kali");
    let user_config = "Host work\n    HostName work.example\n\nHost *\n    ServerAliveInterval 30\n";
    let first = upsert_block(user_config, "kali", &config_block("kali", "192.168.1.57", 22, "atlas", key));
    assert!(first.starts_with("# >>> dial-desktop vm kali "));
    assert!(first.contains("Host kali\n    HostName 192.168.1.57\n    Port 22\n    User atlas\n"));
    assert!(first.ends_with(user_config));
    assert_eq!(ssh::parse_aliases(&first), vec!["kali", "work"]);

    let second = upsert_block(&first, "kali", &config_block("kali", "10.0.0.9", 22, "atlas", key));
    assert_eq!(second.matches("Host kali").count(), 1);
    assert!(second.contains("HostName 10.0.0.9") && !second.contains("192.168.1.57"));
    assert!(second.ends_with(user_config));

    // Another VM's block, and a host that merely starts with the alias, are untouched.
    let other = upsert_block(&second, "kali2", &config_block("kali2", "10.0.0.10", 22, "u", key));
    let again = upsert_block(&other, "kali", &config_block("kali", "10.0.0.11", 22, "atlas", key));
    assert!(again.contains("Host kali2") && again.contains("HostName 10.0.0.11"));
    assert_eq!(upsert_block("", "kali", "B\n"), "B\n");
}

#[test]
fn an_alias_the_user_already_has_is_not_taken_over() {
    let key = Path::new("/k");
    assert_eq!(free_alias("", "kali"), "kali");
    assert_eq!(free_alias("Host kali\n  HostName x\n", "kali"), "dial-kali");
    // Dial's own block for the alias does not count as taken.
    let managed = upsert_block("", "kali", &config_block("kali", "1.2.3.4", 22, "a", key));
    assert_eq!(free_alias(&managed, "kali"), "kali");
}

#[test]
fn public_key_path_preserves_dots_in_vm_aliases() {
    assert_eq!(
        public_key_path(Path::new("/Users/me/.ssh/dial_ubuntu-24.04")),
        PathBuf::from("/Users/me/.ssh/dial_ubuntu-24.04.pub")
    );
    assert_eq!(
        public_key_path(Path::new("/Users/me/.ssh/dial_kali")),
        PathBuf::from("/Users/me/.ssh/dial_kali.pub")
    );
}

#[test]
fn guest_command_supports_multiple_linux_package_managers() {
    let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample dial";
    let cmd = guest_command(key, None, None).unwrap();
    for mgr in ["apt-get", "dnf", "yum", "pacman", "zypper", "apk"] {
        assert!(cmd.contains(mgr), "missing package manager {mgr}: {cmd}");
    }
}
