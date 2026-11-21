use super::*;
use std::os::unix::fs::symlink;

fn fixture() -> tempfile::TempDir {
    let src = tempfile::tempdir().unwrap();
    for name in MAC_BASE_FILES {
        fs::write(src.path().join(name), [7; 512]).unwrap()
    }
    src
}

#[test]
fn confirmed_base_is_independent_and_contains_only_named_parts() {
    let src = fixture();
    let target = tempfile::tempdir().unwrap();
    let dest = target.path().join("macos");
    fs::write(src.path().join("not part of this VM"), b"ignore").unwrap();
    let base = Base::open(src.path()).unwrap();
    assert_eq!(base.size().unwrap(), 2048);
    base.install(&dest, || Ok(())).unwrap();
    assert_eq!(fs::read_dir(&dest).unwrap().count(), 4);
    fs::write(src.path().join("Disk.img"), [8; 512]).unwrap();
    assert!(
        Base::open(src.path())
            .unwrap()
            .install(&dest, || Ok(()))
            .is_err()
    );
    assert_eq!(fs::read(dest.join("Disk.img")).unwrap(), [7; 512]);
}

#[test]
fn only_complete_unlinked_files_and_fresh_confirmation_can_publish() {
    let src = fixture();
    let target = tempfile::tempdir().unwrap();
    let dest = target.path().join("macos");
    let res = Base::open(src.path())
        .unwrap()
        .install(&dest, || Err(io::Error::other("stale project")));
    assert!(res.is_err());
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    let disk = src.path().join("Disk.img");
    fs::remove_file(&disk).unwrap();
    assert!(Base::open(src.path()).is_err());
    symlink(src.path().join("MachineIdentifier"), &disk).unwrap();
    assert!(Base::open(src.path()).is_err());
}

#[test]
fn concurrent_empty_destination_is_not_replaced() {
    let src = fixture();
    let target = tempfile::tempdir().unwrap();
    let dest = target.path().join("macos");
    let result = Base::open(src.path())
        .unwrap()
        .install(&dest, || fs::create_dir(&dest));
    assert!(result.is_err());
    assert_eq!(fs::read_dir(dest).unwrap().count(), 0);
}

#[test]
fn oversized_hardware_model_is_refused_before_import() {
    let src = fixture();
    fs::write(
        src.path().join("HardwareModel"),
        vec![0; MAC_IDENTITY_LIMIT as usize + 1],
    )
    .unwrap();
    assert!(Base::open(src.path()).is_err());
}
