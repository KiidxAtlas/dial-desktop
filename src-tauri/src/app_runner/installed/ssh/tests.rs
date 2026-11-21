use std::collections::BTreeMap;

use super::*;

#[test]
fn aliases_are_concrete_host_names_only() {
    let text = "# comment\nHost kali kali-vm\n  HostName 10.0.0.5\nHost *.lan !bad web?\nhost=Lab\nHOST \"quoted\" -oProxy\nMatch host x\nHost kali\n";
    assert_eq!(parse_aliases(text), vec!["kali", "kali-vm", "Lab", "quoted"]);
}

#[test]
fn alias_rules_refuse_options_patterns_and_spaces() {
    for ok in ["kali", "atlas@kali", "vm_1.local", "host:2222"] {
        assert!(valid_alias(ok), "{ok}");
    }
    for bad in ["", "-oProxyCommand=x", "a b", "*", "web?", "!x", "a;b", &"x".repeat(256)] {
        assert!(!valid_alias(bad), "{bad}");
    }
}

#[test]
fn a_saved_link_wins_and_a_name_match_is_automatic() {
    let mut links = BTreeMap::new();
    links.insert("virtualbox:1".to_owned(), "lab".to_owned());
    let aliases = vec!["Kali".to_owned(), "lab".to_owned()];
    assert_eq!(
        resolve(&links, &aliases, "virtualbox:1", Some("kali")),
        Some(SshLink { host: "lab".into(), source: Source::Linked })
    );
    assert_eq!(
        resolve(&links, &aliases, "virtualbox:2", Some("kali")),
        Some(SshLink { host: "Kali".into(), source: Source::Auto })
    );
    assert_eq!(resolve(&links, &aliases, "virtualbox:2", Some("other")), None);
    assert_eq!(resolve(&links, &aliases, "virtualbox:2", None), None);
    let ambiguous = vec!["kali".to_owned(), "KALI".to_owned()];
    assert_eq!(resolve(&BTreeMap::new(), &ambiguous, "v", Some("kali")), None);
}

#[test]
fn links_round_trip_and_unlink() {
    let dir = tempfile::tempdir().unwrap();
    assert!(load(dir.path()).is_empty());
    save(dir.path(), "virtualbox:1", Some("kali")).unwrap();
    save(dir.path(), "vmware:2", Some("win")).unwrap();
    assert_eq!(load(dir.path()).get("virtualbox:1").map(String::as_str), Some("kali"));
    save(dir.path(), "virtualbox:1", None).unwrap();
    let links = load(dir.path());
    assert!(!links.contains_key("virtualbox:1"));
    assert_eq!(links.len(), 1);
    assert!(save(dir.path(), "virtualbox:1", Some("-oProxyCommand=x")).is_err());
    assert!(save(dir.path(), "", Some("kali")).is_err());
}

#[test]
fn a_corrupt_or_tampered_file_yields_only_valid_links() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("app-runner").join("vm-ssh.json");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "not json").unwrap();
    assert!(load(dir.path()).is_empty());
    fs::write(&file, r#"{"virtualbox:1":"kali","virtualbox:2":"a b"}"#).unwrap();
    let links = load(dir.path());
    assert_eq!(links.len(), 1);
    assert_eq!(links["virtualbox:1"], "kali");
}
