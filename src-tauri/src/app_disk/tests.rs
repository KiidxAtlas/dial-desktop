use super::*;

#[test]
fn copies_only_complete_confirmed_disk_and_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("guest.raw");
    for wrong_size in [2, 4] {
        assert!(copy_disk(&b"abc"[..], wrong_size, &dest, || Ok(())).is_err());
        assert!(!dest.exists());
    }
    assert!(
        copy_disk(&b"abc"[..], 3, &dest, || Err(io::Error::other(
            "different project"
        )))
        .is_err()
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    copy_disk(&b"abc"[..], 3, &dest, || Ok(())).unwrap();
    assert!(copy_disk(&b"xyz"[..], 3, &dest, || Ok(())).is_err());
    assert_eq!(fs::read(dest).unwrap(), b"abc");
}
