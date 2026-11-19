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
fn inline_images_have_the_same_size_limit_as_files() {
    let too_large = STANDARD.encode(vec![0; MAX_IMAGE_BYTES as usize + 1]);
    assert!(
        check(&[], &[too_large])
            .unwrap_err()
            .contains("at most 20 MB")
    );
}
