#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use super::super::runtime::{self, MAC_BASE_FILES, MACOS_HELPER, MACOS_IMAGE, RuntimeError};
use super::{MacRuntime, args, copy_small, discover, stage};

/// A packaged helper and a base folder with all four files; each file's content is its name.
struct Tree {
    resource: tempfile::TempDir,
    data: tempfile::TempDir,
}

fn tree() -> Tree {
    let resource = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    fs::create_dir_all(resource.path().join(runtime::HELPER_DIR)).unwrap();
    let helper = helper_path(&resource);
    fs::write(&helper, b"helper").unwrap();
    set_mode(&helper, 0o755);
    let base = runtime::macos_base_dir(data.path());
    fs::create_dir_all(&base).unwrap();
    for name in MAC_BASE_FILES {
        fs::write(base.join(name), name.as_bytes()).unwrap();
    }
    Tree { resource, data }
}

fn helper_path(resource: &tempfile::TempDir) -> PathBuf {
    resource
        .path()
        .join(runtime::HELPER_DIR)
        .join(runtime::HELPER_NAME)
}

fn base_of(t: &Tree) -> PathBuf {
    runtime::macos_base_dir(t.data.path())
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

enum Expect {
    Missing(&'static str),
    Refused,
}

fn check(result: Result<MacRuntime, RuntimeError>, expect: &Expect, label: &str) {
    match (result, expect) {
        (Err(RuntimeError::Missing(got)), Expect::Missing(want)) => {
            assert_eq!(got, *want, "{label}");
        }
        (Err(RuntimeError::Refused(_)), Expect::Refused) => {}
        (other, _) => panic!("{label}: unexpected {other:?}"),
    }
}

#[test]
fn packaged_helper_and_base_files_are_found() {
    let t = tree();
    let found = discover(t.resource.path(), t.data.path()).unwrap();
    assert_eq!(found.helper, helper_path(&t.resource));
    assert_eq!(found.base, base_of(&t));
}

#[test]
fn each_missing_part_is_named_by_its_component() {
    for name in MAC_BASE_FILES {
        let t = tree();
        fs::remove_file(base_of(&t).join(name)).unwrap();
        check(
            discover(t.resource.path(), t.data.path()),
            &Expect::Missing(MACOS_IMAGE),
            name,
        );
    }
    type Damage = fn(&Tree);
    let cases: [(&str, Damage, Expect); 6] = [
        (
            "helper missing",
            |t| fs::remove_file(helper_path(&t.resource)).unwrap(),
            Expect::Missing(MACOS_HELPER),
        ),
        (
            "helper folder missing",
            |t| fs::remove_dir_all(t.resource.path().join(runtime::HELPER_DIR)).unwrap(),
            Expect::Missing(MACOS_HELPER),
        ),
        (
            "helper not executable",
            |t| set_mode(&helper_path(&t.resource), 0o644),
            Expect::Refused,
        ),
        (
            "base folder missing",
            |t| fs::remove_dir_all(runtime::macos_base_dir(t.data.path())).unwrap(),
            Expect::Missing(MACOS_IMAGE),
        ),
        (
            "base disk replaced by a folder",
            |t| {
                let p = runtime::macos_base_dir(t.data.path()).join("Disk.img");
                fs::remove_file(&p).unwrap();
                fs::create_dir(&p).unwrap();
            },
            Expect::Refused,
        ),
        (
            "base disk replaced by a link",
            |t| {
                let p = runtime::macos_base_dir(t.data.path()).join("Disk.img");
                fs::remove_file(&p).unwrap();
                std::os::unix::fs::symlink(helper_path(&t.resource), &p).unwrap();
            },
            Expect::Refused,
        ),
    ];
    for (label, damage, expect) in cases {
        let t = tree();
        damage(&t);
        check(discover(t.resource.path(), t.data.path()), &expect, label);
    }
}

#[test]
fn the_helper_gets_run_and_an_optional_project_iso_only() {
    let dir = Path::new("/tmp/run dir,with comma");
    let iso = Path::new("/tmp/project.iso");
    let cases: [(Option<&Path>, Vec<&str>); 2] = [
        (None, vec!["run", "/tmp/run dir,with comma"]),
        (
            Some(iso),
            vec!["run", "/tmp/run dir,with comma", "/tmp/project.iso"],
        ),
    ];
    for (project_iso, want) in cases {
        let got: Vec<String> = args(dir, project_iso)
            .into_iter()
            .map(|arg| arg.into_string().unwrap())
            .collect();
        assert_eq!(got, want);
    }
}

#[test]
fn an_existing_clone_target_is_refused_and_left_alone() {
    let t = tree();
    let mac = discover(t.resource.path(), t.data.path()).unwrap();
    let run = t.data.path().join("run");
    fs::create_dir(&run).unwrap();
    fs::write(run.join("Disk.img"), b"keep").unwrap();
    assert!(stage(&mac, &run).is_err());
    assert_eq!(fs::read(run.join("Disk.img")).unwrap(), b"keep");
}

#[test]
fn an_oversized_identity_file_is_refused_without_a_copy() {
    let t = tree();
    let src = base_of(&t).join("MachineIdentifier");
    fs::write(&src, vec![0u8; (1 << 20) + 1]).unwrap();
    let dst = t.data.path().join("copy");
    assert!(copy_small(&src, &dst).is_err());
    assert!(!dst.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn a_run_gets_private_clones_and_the_base_is_never_written() {
    let t = tree();
    let mac = discover(t.resource.path(), t.data.path()).unwrap();
    let run = t.data.path().join("run");
    fs::create_dir(&run).unwrap();
    stage(&mac, &run).unwrap();
    for name in MAC_BASE_FILES {
        assert_eq!(
            fs::read(run.join(name)).unwrap(),
            fs::read(base_of(&t).join(name)).unwrap()
        );
    }
    fs::write(run.join("Disk.img"), b"changed").unwrap();
    assert_eq!(fs::read(base_of(&t).join("Disk.img")).unwrap(), b"Disk.img");
}

#[test]
fn run_media_directory_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let dir = super::super::private_run_dir(root.path()).unwrap();
    let mode = fs::metadata(dir.path()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
}
