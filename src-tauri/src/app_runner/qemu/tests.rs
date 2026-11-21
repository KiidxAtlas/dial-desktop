use super::super::runtime::{Arch, Runtime};
use super::{Accel, Launch, args};
use std::path::{Path, PathBuf};

fn runtime(root: &Path) -> Runtime {
    Runtime {
        arch: Arch::X86_64,
        qemu: root.join("bin/qemu-system-x86_64"),
        firmware_code: root.join("share/code.fd"),
        firmware_vars: root.join("share/vars.fd"),
        guest_disk: root.join("guest.raw"),
    }
}

fn argv(runtime: &Runtime, iso: Option<&Path>) -> Vec<String> {
    argv_with(runtime, iso, None)
}

fn argv_with(runtime: &Runtime, iso: Option<&Path>, vnc: Option<&Path>) -> Vec<String> {
    let vars = runtime.firmware_vars.clone();
    let share = runtime.firmware_code.parent().unwrap().to_path_buf();
    args(&Launch {
        runtime,
        vars_copy: &vars,
        share_dir: &share,
        accel: Accel {
            name: "tcg",
            cpu: "qemu64",
        },
        share_iso: iso,
        vnc_socket: vnc,
        memory_mb: 2048,
        cpus: 2,
    })
}

#[test]
fn a_project_snapshot_attaches_read_only_through_an_explicit_scsi_cd() {
    let root = PathBuf::from("/run/dir");
    let argv = argv(&runtime(&root), Some(Path::new("/run/dir/project.iso")));
    let joined = argv.join(" ");
    assert!(joined.contains("-device virtio-scsi-pci,id=scsi0"));
    assert!(joined.contains(
        "if=none,id=projectiso,format=raw,media=cdrom,readonly=on,file=/run/dir/project.iso"
    ));
    assert!(joined.contains("-device scsi-cd,bus=scsi0.0,drive=projectiso"));
    assert!(
        !joined.contains("if=virtio,format=raw,media=cdrom"),
        "no virtio CD"
    );
}

#[test]
fn no_snapshot_means_no_cd_device_at_all() {
    let argv = argv(&runtime(Path::new("/r")), None).join(" ");
    assert!(!argv.contains("scsi-cd"));
    assert!(!argv.contains("virtio-scsi-pci"));
    assert!(!argv.contains("media=cdrom"));
}

#[test]
fn the_share_directory_is_a_plain_path_but_drive_files_escape_commas() {
    let root = PathBuf::from("/opt/a,b");
    let argv = argv(&runtime(&root), None);
    let position = argv.iter().position(|a| a == "-L").expect("-L present");
    assert_eq!(
        argv[position + 1],
        "/opt/a,b/share",
        "-L takes the path unescaped"
    );
    assert!(
        argv.iter().any(|a| a.contains("file=/opt/a,,b/guest.raw")),
        "drive paths double commas"
    );
}

#[test]
fn hardware_first_on_windows_and_linux_keeps_the_same_isolated_guest_for_tcg() {
    for (os, machine) in [
        ("windows", "q35,accel=whpx:tcg"),
        ("linux", "q35,accel=kvm:tcg"),
    ] {
        let mut runtime = runtime(Path::new("/private"));
        runtime.guest_disk = PathBuf::from(format!("/private/{os}.raw"));
        let argv = args(&Launch {
            runtime: &runtime,
            vars_copy: Path::new("/private/run/vars.fd"),
            share_dir: Path::new("/private/share"),
            accel: Accel::for_host(os, runtime.arch),
            share_iso: None,
            vnc_socket: None,
            memory_mb: 2048,
            cpus: 2,
        });
        for pair in [
            ["-machine", machine],
            ["-cpu", "max"],
            ["-nic", "none"],
            ["-display", "none"],
        ] {
            assert!(argv.windows(2).any(|w| w == pair), "{os}: missing {pair:?}");
        }
        assert!(argv.contains(&"-snapshot".into()));
        assert!(argv.contains(&format!("if=virtio,format=raw,file=/private/{os}.raw")));
    }
}

#[test]
fn the_display_is_served_only_on_the_run_folder_socket() {
    let root = PathBuf::from("/run/dir");
    let plain = argv(&runtime(&root), None);
    assert!(!plain.iter().any(|a| a == "-vnc"), "no display unless asked");
    let served = argv_with(&runtime(&root), None, Some(Path::new("/run/dir/vnc.sock")));
    let at = served.iter().position(|a| a == "-vnc").expect("-vnc");
    assert_eq!(served[at + 1], "unix:/run/dir/vnc.sock");
    assert_eq!(served.iter().filter(|a| *a == "-vnc").count(), 1);
    // Never a TCP display, and still no window.
    assert!(!served.iter().any(|a| a.contains(":5900") || a.starts_with("tcp")));
    let display = served.iter().position(|a| a == "-display").unwrap();
    assert_eq!(served[display + 1], "none");
}
