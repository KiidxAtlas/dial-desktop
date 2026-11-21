use super::*;

#[test]
fn only_supported_image_types_are_accepted() {
    assert_eq!(image_mime(Path::new("a/shot.PNG")), Some("image/png"));
    assert_eq!(image_mime(Path::new("b.jpeg")), Some("image/jpeg"));
    assert_eq!(image_mime(Path::new("c.webp")), Some("image/webp"));
    assert_eq!(image_mime(Path::new("notes.txt")), None);
    assert_eq!(image_mime(Path::new("png")), None);
}

#[test]
fn reading_an_image_gives_a_data_url_preview() {
    let dir = std::env::temp_dir().join(format!("dial-desktop-img-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("dot.png");
    std::fs::write(&png, b"\x89PNG").unwrap();
    let image = read_image(&png).unwrap();
    assert_eq!(image.name, "dot.png");
    assert_eq!(image.path, png.to_string_lossy());
    assert_eq!(image.url, "data:image/png;base64,iVBORw==");

    let text = dir.join("notes.txt");
    std::fs::write(&text, b"hi").unwrap();
    assert!(read_image(&text).unwrap_err().contains("not a PNG"));
    assert!(
        read_image(&dir.join("gone.png"))
            .unwrap_err()
            .starts_with("Could not read gone.png")
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn attachments_are_checked_before_sending() {
    assert!(check(&[], &[]).is_ok());
    assert!(check(&["/a/b.png".into()], &["iVBORw==".into()]).is_ok());
    assert!(check(&["/a/b.pdf".into()], &[]).is_err());
    assert!(check(&[], &["not base64!".into()]).is_err());
    assert!(check(&[], &[String::new()]).is_err());
    let many = vec!["x.png".to_owned(); MAX_IMAGES + 1];
    assert!(check(&many, &[]).unwrap_err().contains("at most"));
}

#[test]
fn desktop_accepts_the_engine_source_size_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("screenshot.png");
    let file = std::fs::File::create(&path).unwrap();
    let engine_limit = 64 * 1024 * 1024;
    file.set_len(engine_limit + 1).unwrap();
    assert!(read_image(&path).is_err());
    file.set_len(engine_limit).unwrap();
    assert!(read_image(&path).is_ok());
}

#[test]
fn inline_images_have_the_same_size_limit_as_files() {
    let too_large = STANDARD.encode(vec![0; MAX_IMAGE_BYTES as usize + 1]);
    assert!(
        check(&[], &[too_large])
            .unwrap_err()
            .contains("at most 64 MiB")
    );
}

#[test]
fn image_types_are_sniffed_from_their_bytes() {
    assert_eq!(sniff_extension(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
    assert_eq!(sniff_extension(&[0xff, 0xd8, 0xff, 0xe0]), Some("jpg"));
    assert_eq!(sniff_extension(b"GIF89a.."), Some("gif"));
    assert_eq!(sniff_extension(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
    assert_eq!(sniff_extension(b"%PDF-1.7"), None);
    assert_eq!(sniff_extension(b""), None);
}

#[test]
fn a_large_pasted_image_becomes_a_private_file_with_the_same_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("pasted-images");
    // Larger than the engine's 1 MiB RPC limit once base64-encoded.
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(std::iter::repeat_n(7u8, 2 * 1024 * 1024));
    let paths = save_pasted(&target, &[STANDARD.encode(&png)]).unwrap();
    assert_eq!(paths.len(), 1);
    let path = Path::new(&paths[0]);
    assert_eq!(path.parent(), Some(target.as_path()));
    assert_eq!(path.extension().and_then(|e| e.to_str()), Some("png"));
    assert_eq!(std::fs::read(path).unwrap(), png);
    assert!(image_mime(path).is_some(), "the engine accepts it as an image path");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(path), 0o600);
        assert_eq!(mode(&target), 0o700);
    }
}

#[test]
fn pasting_nothing_writes_nothing_and_unknown_bytes_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("pasted-images");
    assert!(save_pasted(&target, &[]).unwrap().is_empty());
    assert!(!target.exists());
    let error = save_pasted(&target, &[STANDARD.encode(b"%PDF-1.7")]).unwrap_err();
    assert!(error.contains("not a PNG"));
}

#[test]
fn only_old_pasted_files_are_pruned() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("pasted-old.png");
    let fresh = dir.path().join("pasted-new.png");
    let other = dir.path().join("keep.png");
    for p in [&old, &fresh, &other] {
        std::fs::write(p, b"x").unwrap();
    }
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 24 * 3600);
    for p in [&old, &other] {
        std::fs::File::options().write(true).open(p).unwrap().set_modified(long_ago).unwrap();
    }
    prune(dir.path(), PASTED_KEEP);
    assert!(!old.exists());
    assert!(fresh.exists());
    assert!(other.exists(), "only Dial's own pasted-* files are removed");
}
