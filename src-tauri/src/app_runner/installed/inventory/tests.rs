use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;

const UUID_ALPHA: &str = "1f2e3d4c-5b6a-4789-8abc-def012345678";
const UUID_BETA: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// An executable shell script. Only the VBoxManage fixtures run it; nothing runs a real product.
#[cfg(unix)]
fn script(path: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    write(path, &format!("#!/bin/sh\n{body}\n"));
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    path.to_path_buf()
}

/// A stand-in tool that is an executable file on every platform.
fn tool(path: &Path) -> PathBuf {
    write(path, "tool");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    path.to_path_buf()
}

/// A host with nothing installed; each test adds only what it exercises.
fn empty_locations(_root: &Path) -> Locations {
    Locations {
        virtualbox_tools: Vec::new(),
        vbox_list_timeout: VBOX_LIST_TIMEOUT,
        vmware_inventories: Vec::new(),
        vmware_tools: Vec::new(),
    }
}

/// A `VBoxManage list vms` fixture: the expected arguments, then fixed output.
#[cfg(unix)]
fn list_script(root: &Path, output_lines: &[&str]) -> PathBuf {
    let quoted: Vec<String> = output_lines.iter().map(|l| format!("'{l}'")).collect();
    let body = format!(
        "if [ \"$1\" != list ] || [ \"$2\" != vms ]; then exit 2; fi\nprintf '%s\\n' {}",
        quoted.join(" ")
    );
    script(&root.join("bin/VBoxManage"), &body)
}

#[cfg(unix)]
#[test]
fn virtualbox_lists_registered_machines_once_by_uuid() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let vbox = list_script(
        root,
        &[
            "\"Alpha & Co\" {1F2E3D4C-5B6A-4789-8ABC-DEF012345678}",
            "not a machine line",
            "\"Beta\" {aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee}",
            "\"Alpha & Co\" {1f2e3d4c-5b6a-4789-8abc-def012345678}",
            "\"\" {cccccccc-dddd-4eee-8fff-000000000000}",
        ],
    );
    let mut locations = empty_locations(root);
    locations.virtualbox_tools = vec![vbox.clone()];

    let inventory = discover_in(&locations);

    let rows: Vec<(&str, &str, &str)> = inventory
        .machines
        .iter()
        .map(|m| (m.id.as_str(), m.name.as_str(), m.locator.as_str()))
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                "virtualbox:1f2e3d4c-5b6a-4789-8abc-def012345678",
                "Alpha & Co",
                UUID_ALPHA
            ),
            (
                "virtualbox:aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
                "Beta",
                UUID_BETA
            ),
        ]
    );
    for machine in &inventory.machines {
        assert_eq!(machine.provider, Provider::VirtualBox);
        assert_eq!(machine.tool, vbox);
    }
    // The garbage line and the unnamed entry are skipped and counted once, without paths.
    assert_eq!(inventory.notices.len(), 1);
    assert!(inventory.notices[0].starts_with("2 VirtualBox line(s)"));
    assert!(!inventory.notices[0].contains(&root.display().to_string()));
}

#[cfg(unix)]
#[test]
fn virtualbox_reports_which_machines_are_running() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let body = format!(
        "if [ \"$1\" != list ]; then exit 2; fi\n\
         if [ \"$2\" = vms ]; then printf '%s\\n' '\"Alpha\" {{{UUID_ALPHA}}}' '\"Beta\" {{{UUID_BETA}}}'; exit 0; fi\n\
         if [ \"$2\" = runningvms ]; then printf '%s\\n' '\"Beta\" {{{UUID_BETA}}}'; exit 0; fi\n\
         exit 2"
    );
    let vbox = script(&root.join("bin/VBoxManage"), &body);
    let mut locations = empty_locations(root);
    locations.virtualbox_tools = vec![vbox];

    let inventory = discover_in(&locations);

    let rows: Vec<(&str, Option<bool>)> = inventory
        .machines
        .iter()
        .map(|m| (m.name.as_str(), m.running))
        .collect();
    assert_eq!(rows, vec![("Alpha", Some(false)), ("Beta", Some(true))]);
}

#[cfg(unix)]
#[test]
fn virtualbox_running_state_is_unknown_when_its_list_fails() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    // `list_script` answers only `list vms`; `list runningvms` exits 2.
    let vbox = list_script(root, &["\"Alpha\" {1f2e3d4c-5b6a-4789-8abc-def012345678}"]);
    let mut locations = empty_locations(root);
    locations.virtualbox_tools = vec![vbox];

    let inventory = discover_in(&locations);

    assert_eq!(inventory.machines.len(), 1);
    assert_eq!(inventory.machines[0].running, None);
}

#[cfg(unix)]
#[test]
fn virtualbox_failed_list_is_reported_without_machines() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let mut locations = empty_locations(root);
    locations.virtualbox_tools = vec![script(
        &root.join("bin/VBoxManage"),
        "echo '\"A\" {1f2e3d4c-5b6a-4789-8abc-def012345678}'; exit 1",
    )];

    let inventory = discover_in(&locations);

    assert!(inventory.machines.is_empty());
    assert_eq!(inventory.notices.len(), 1);
    assert!(inventory.notices[0].contains("could not be read"));
}

#[cfg(unix)]
#[test]
fn virtualbox_list_is_stopped_at_the_time_limit() {
    use std::time::Instant;
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let mut locations = empty_locations(root);
    locations.virtualbox_tools = vec![script(&root.join("bin/VBoxManage"), "exec sleep 5")];
    locations.vbox_list_timeout = std::time::Duration::from_millis(300);

    let started = Instant::now();
    let inventory = discover_in(&locations);

    assert!(
        started.elapsed().as_secs() < 4,
        "the list must be stopped at the limit"
    );
    assert!(inventory.machines.is_empty());
    assert_eq!(inventory.notices.len(), 1);
}

#[cfg(unix)]
#[test]
fn virtualbox_list_output_is_capped() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let mut locations = empty_locations(root);
    locations.virtualbox_tools = vec![script(
        &root.join("bin/VBoxManage"),
        "head -c 70000 /dev/zero | tr '\\0' a",
    )];

    let inventory = discover_in(&locations);

    assert!(inventory.machines.is_empty());
    assert_eq!(inventory.notices.len(), 1);
}

#[test]
fn virtualbox_without_the_tool_is_silent() {
    let dir = TempDir::new().unwrap();
    let locations = empty_locations(dir.path());
    assert_eq!(discover_in(&locations), Inventory::default());
}

#[test]
fn list_lines_parse_only_the_documented_form() {
    let cases = [
        (
            "\"Alpha\" {1f2e3d4c-5b6a-4789-8abc-def012345678}",
            Some(("Alpha", UUID_ALPHA)),
        ),
        (
            "\"Q \"x\" {1F2E3D4C-5B6A-4789-8ABC-DEF012345678}",
            Some(("Q \"x", UUID_ALPHA)),
        ),
        (
            "  \"Spaced\"   {1f2e3d4c-5b6a-4789-8abc-def012345678}  ",
            Some(("Spaced", UUID_ALPHA)),
        ),
        ("Alpha {1f2e3d4c-5b6a-4789-8abc-def012345678}", None),
        ("\"\" {1f2e3d4c-5b6a-4789-8abc-def012345678}", None),
        ("\"Alpha\" {not-a-uuid}", None),
        ("\"Alpha\" 1f2e3d4c-5b6a-4789-8abc-def012345678", None),
        ("", None),
    ];
    for (line, expected) in cases {
        let expected = expected.map(|(name, uuid)| (name.to_owned(), uuid.to_owned()));
        assert_eq!(parse_list_line(line), expected, "line {line:?}");
    }
}

#[test]
fn vmware_workstation_inventory_lists_vmx_files_by_canonical_path() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let win = root.join("vms/Win/Win.vmx");
    let linux = root.join("vms/Linux/Linux.vmx");
    write(&win, "config");
    write(&linux, "config");
    let long = format!("/{}/Long.vmx", "d".repeat(MAX_PATH_BYTES));
    let vmls = format!(
        r#".encoding = "UTF-8"
vmlist1.config = "{win}"
vmlist1.DisplayName = "Windows Lab"
vmlist2.config = "{linux}"
vmlist3.config = "{gone}"
vmlist4.config = "{win}"
vmlist5.config = "relative/Rel.vmx"
vmlist6.config = "{long}"
"#,
        win = win.display(),
        linux = linux.display(),
        gone = root.join("vms/Gone/Gone.vmx").display(),
    );
    write(&root.join("inventory.vmls"), &vmls);
    let mut locations = empty_locations(root);
    locations.vmware_inventories = vec![root.join("inventory.vmls")];
    locations.vmware_tools = vec![tool(&root.join("bin/vmrun"))];

    let inventory = discover_in(&locations);

    let names: Vec<&str> = inventory.machines.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, vec!["Windows Lab", "Linux"]);
    let canonical_win = strip_verbatim(fs::canonicalize(&win).unwrap());
    assert_eq!(
        inventory.machines[0].locator,
        canonical_win.to_string_lossy()
    );
    for machine in &inventory.machines {
        assert_eq!(machine.provider, Provider::Vmware);
        assert!(machine.id.starts_with("vmware:"));
        assert_eq!(machine.id.len(), "vmware:".len() + 32);
        assert!(!machine.id.contains(&root.display().to_string()));
        assert_eq!(machine.tool, root.join("bin/vmrun"));
    }
    // The duplicate is one machine. The missing, relative and over-long entries are skipped and
    // counted in one notice.
    assert_eq!(inventory.notices.len(), 1);
    assert!(inventory.notices[0].starts_with("3 VMware machine(s)"));
    assert!(!inventory.notices[0].contains(&root.display().to_string()));
}

#[test]
fn fusion_inventories_in_both_locations_are_read_and_deduplicated() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let mac = root.join("Fusion/Mac.vmx");
    let two = root.join("Fusion/Mac & Two.vmx");
    write(&mac, "config");
    write(&two, "config");
    // Preferences uses the text form; Application Support uses an XML property list.
    let preferences = format!("vmlist1.config = \"{}\"\n", mac.display());
    let app_support = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>1</key><dict><key>path</key><string>{mac}</string></dict>
  <key>2</key><string>{notes}</string>
  <key>3</key><string>{two}</string>
</dict></plist>
"#,
        mac = mac.display(),
        notes = root.join("Fusion/Notes.txt").display(),
        two = two.display().to_string().replace('&', "&amp;"),
    );
    write(&root.join("preferences/vmInventory"), &preferences);
    write(&root.join("support/vmInventory"), &app_support);
    let mut locations = empty_locations(root);
    locations.vmware_inventories = vec![
        root.join("preferences/vmInventory"),
        root.join("support/vmInventory"),
    ];
    locations.vmware_tools = vec![tool(&root.join("bin/vmrun"))];

    let inventory = discover_in(&locations);

    let names: Vec<&str> = inventory.machines.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, vec!["Mac", "Mac & Two"]);
    assert!(inventory.notices.is_empty());
}

#[test]
fn vmware_read_budget_is_shared_across_inventory_files() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let vmx = root.join("vms/A.vmx");
    write(&vmx, "config");
    // Each file is just under half the budget. The second no longer fits and is reported.
    let padding = "# pad\n".repeat((MAX_INVENTORY_BYTES as usize / 2) / 6 + 1);
    let first = format!("vmlist1.config = \"{}\"\n{padding}", vmx.display());
    let second = format!("{padding}vmlist1.config = \"{}\"\n", vmx.display());
    write(&root.join("first.vmls"), &first);
    write(&root.join("second.vmls"), &second);
    let mut locations = empty_locations(root);
    locations.vmware_inventories = vec![root.join("first.vmls"), root.join("second.vmls")];
    locations.vmware_tools = vec![tool(&root.join("bin/vmrun"))];

    let inventory = discover_in(&locations);

    assert_eq!(inventory.machines.len(), 1);
    assert_eq!(inventory.notices.len(), 1);
    assert!(inventory.notices[0].starts_with("1 VMware inventory file(s)"));
}

#[test]
fn binary_fusion_inventory_is_reported_not_parsed() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(&root.join("vmInventory"), "bplist00\u{1}\u{2}");
    let mut locations = empty_locations(root);
    locations.vmware_inventories = vec![root.join("vmInventory")];
    locations.vmware_tools = vec![tool(&root.join("bin/vmrun"))];

    let inventory = discover_in(&locations);

    assert!(inventory.machines.is_empty());
    assert_eq!(inventory.notices.len(), 1);
    assert!(inventory.notices[0].contains("binary format"));
}

#[test]
fn vmware_registered_machines_are_hidden_without_vmrun() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let vmx = root.join("vms/Win/Win.vmx");
    write(&vmx, "config");
    write(
        &root.join("inventory.vmls"),
        &format!("vmlist1.config = \"{}\"\n", vmx.display()),
    );
    let mut locations = empty_locations(root);
    locations.vmware_inventories = vec![root.join("inventory.vmls")];

    let inventory = discover_in(&locations);

    assert!(inventory.machines.is_empty());
    assert_eq!(inventory.notices.len(), 1);
    assert!(inventory.notices[0].contains("vmrun was not found"));
}

#[test]
#[cfg(unix)]
fn tool_resolution_skips_plain_files_and_directories() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let plain = root.join("plain");
    write(&plain, "not executable");
    fs::set_permissions(&plain, fs::Permissions::from_mode(0o644)).unwrap();
    fs::create_dir_all(root.join("a-directory")).unwrap();
    let real = tool(&root.join("bin/VBoxManage"));

    let candidates = [
        root.join("missing"),
        plain,
        root.join("a-directory"),
        real.clone(),
    ];
    assert_eq!(first_executable(&candidates), Some(real));
    assert_eq!(first_executable(&candidates[..3]), None);
}

#[test]
fn environment_paths_are_used_only_when_absolute() {
    let absolute = if cfg!(windows) {
        r"C:\Users\ada"
    } else {
        "/home/ada"
    };
    let cases = [
        (None, None),
        (Some(""), None),
        (Some("relative/home"), None),
        (Some(absolute), Some(PathBuf::from(absolute))),
    ];
    for (value, expected) in cases {
        let value = value.map(OsString::from);
        assert_eq!(absolute_path(value.clone()), expected, "value {value:?}");
    }
}

#[test]
fn machines_serialize_without_tool_or_locator() {
    let machine = Machine {
        id: "virtualbox:1f2e3d4c-5b6a-4789-8abc-def012345678".into(),
        name: "Alpha".into(),
        provider: Provider::VirtualBox,
        running: Some(true),
        tool: PathBuf::from("/secret/VBoxManage"),
        locator: UUID_ALPHA.into(),
    };
    let inventory = Inventory {
        machines: vec![machine],
        notices: vec!["note".into()],
    };

    let json = serde_json::to_value(&inventory).unwrap();

    assert_eq!(
        json,
        serde_json::json!({
            "machines": [{
                "id": "virtualbox:1f2e3d4c-5b6a-4789-8abc-def012345678",
                "name": "Alpha",
                "provider": "virtualbox",
                "running": true,
            }],
            "notices": ["note"],
        })
    );
    let vmware = serde_json::to_value(Provider::Vmware).unwrap();
    assert_eq!(vmware, serde_json::json!("vmware"));
}

#[test]
fn entities_decode_in_names_and_fall_back_when_unknown() {
    let cases = [
        ("a &amp; b", "a & b"),
        ("&lt;x&gt; &quot;q&quot; &apos;a&apos;", "<x> \"q\" 'a'"),
        ("&#65;&#x42;&#X43;", "ABC"),
        ("&bogus; &", "&bogus; &"),
        ("&#xD800;", "&#xD800;"),
        ("plain", "plain"),
    ];
    for (input, expected) in cases {
        assert_eq!(decode_entities(input), expected, "input {input:?}");
    }
}

#[test]
fn uuids_normalize_to_lowercase_hyphenated_text() {
    let cases = [
        ("{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}", Some(UUID_BETA)),
        (" aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee ", Some(UUID_BETA)),
        ("aaaaaaaabbbb4ccc8dddeeeeeeeeeeee", None),
        ("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeeg", None),
        ("", None),
    ];
    for (input, expected) in cases {
        assert_eq!(
            normalize_uuid(input).as_deref(),
            expected,
            "input {input:?}"
        );
    }
}
