use super::*;

#[test]
fn copies_only_complete_confirmed_disk_and_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("guest.raw");
    for wrong_size in [2, 4] {
        assert!(copy_disk(&b"abc"[..], wrong_size, &dest, |_| {}, || false, || Ok(())).is_err());
        assert!(!dest.exists());
    }
    assert!(
        copy_disk(&b"abc"[..], 3, &dest, |_| {}, || false, || Err(io::Error::other(
            "different project"
        )))
        .is_err()
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    let mut seen = Vec::new();
    copy_disk(&b"abc"[..], 3, &dest, |n| seen.push(n), || false, || Ok(())).unwrap();
    assert_eq!(seen.last(), Some(&3));
    assert!(copy_disk(&b"xyz"[..], 3, &dest, |_| {}, || false, || Ok(())).is_err());
    assert_eq!(fs::read(dest).unwrap(), b"abc");
}

#[test]
fn sizes_read_like_people_write_them() {
    for (bytes, text) in [(512, "512 B"), (1536, "1.5 KB"), (20 << 30, "20.0 GB")] {
        assert_eq!(human_size(bytes), text);
    }
}

#[test]
fn a_cancelled_import_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("guest.raw");
    let calls = std::cell::Cell::new(0);
    // Cancelled after the first chunk was written.
    let err = copy_disk(&vec![0u8; 8 << 20][..], 8 << 20, &dest, |_| {}, || { calls.set(calls.get() + 1); calls.get() > 1 }, || Ok(())).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn convertible_disks_and_qemu_img_progress_are_recognised() {
    for (name, format) in [("a.qcow2", Some("qcow2")), ("b.VMDK", Some("vmdk")), ("c.vdi", Some("vdi")), ("d.raw", None), ("e.img", None), ("f", None)] {
        assert_eq!(convertible_format(Path::new(name)), format, "{name}");
    }
    for (line, percent) in [("    (0.00/100%)", Some(0.0)), ("(42.17/100%)", Some(42.17)), ("    (100.00/100%)", Some(100.0)), ("qemu-img: error", None), ("(150/100%)", None)] {
        assert_eq!(qemu_progress(line), percent, "{line}");
    }
    assert!(no_qemu_img_message("qcow2").contains("qemu-img convert -p -O raw input.qcow2 guest.raw"));
}

#[cfg(unix)]
#[test]
fn conversion_publishes_only_a_finished_disk() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    // A stand-in qemu-img: writes 1024 bytes to its last argument, printing progress.
    let tool = dir.path().join("qemu-img");
    fs::write(&tool, "#!/bin/sh\nfor last; do :; done\nprintf '    (50.00/100%%)\\r'\nhead -c 1024 /dev/zero > \"$last\"\nprintf '    (100.00/100%%)\\r'\n").unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    let src = dir.path().join("in.qcow2");
    fs::write(&src, b"x").unwrap();
    let out = dir.path().join("out");
    let dest = out.join("guest.raw");
    let mut seen = Vec::new();
    convert_disk(&tool, &src, &dest, |p| seen.push(p), || false, || Ok(())).unwrap();
    assert_eq!(fs::metadata(&dest).unwrap().len(), 1024);
    assert!(seen.contains(&50.0) && seen.last() == Some(&100.0), "{seen:?}");
    assert_eq!(fs::read_dir(&out).unwrap().count(), 1);
    // Never overwrites; a cancelled run leaves no temp file.
    assert!(convert_disk(&tool, &src, &dest, |_| {}, || false, || Ok(())).is_err());
    let other = dir.path().join("other").join("guest.raw");
    let err = convert_disk(&tool, &src, &other, |_| {}, || true, || Ok(())).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
    assert_eq!(fs::read_dir(other.parent().unwrap()).unwrap().count(), 0);
}
