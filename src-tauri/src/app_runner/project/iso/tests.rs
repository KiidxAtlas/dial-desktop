use super::*;

/// Reads one volume back the way an ISO reader would: `joliet` picks the Joliet tree, otherwise
/// the ISO 9660 tree with the `;1` version stripped. Returns path -> contents.
pub(in crate::app_runner::project) fn read_volume(
    image: &[u8],
    joliet: bool,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let descriptor = sector(image, if joliet { 17 } else { 16 })?;
    let expected_kind = if joliet { 2 } else { 1 };
    if descriptor[0] != expected_kind || &descriptor[1..6] != STANDARD_ID {
        return Err("bad volume descriptor".to_owned());
    }
    let mut files = BTreeMap::new();
    walk(image, &descriptor[156..190], "", joliet, &mut files)?;
    Ok(files)
}

/// Number of records in the L (`big` false) or M path table of one volume.
pub(in crate::app_runner::project) fn path_table_count(
    image: &[u8],
    joliet: bool,
    big: bool,
) -> Result<usize, String> {
    let descriptor = sector(image, if joliet { 17 } else { 16 })?;
    let size = be_or_le32(descriptor, 132, false) as usize;
    let at = if big {
        be_or_le32(descriptor, 148, true)
    } else {
        be_or_le32(descriptor, 140, false)
    } as usize;
    let table = image
        .get(at * SECTOR..at * SECTOR + size)
        .ok_or("path table outside the image")?;
    let mut count = 0;
    let mut pos = 0;
    while pos < table.len() {
        let id_len = table[pos] as usize;
        pos += 8 + id_len + id_len % 2;
        count += 1;
    }
    Ok(count)
}

pub(in crate::app_runner::project) fn volume_space_sectors(image: &[u8]) -> u32 {
    be_or_le32(image, 16 * SECTOR + 80, false)
}

fn be_or_le32(buf: &[u8], at: usize, big: bool) -> u32 {
    let bytes: [u8; 4] = buf[at..at + 4].try_into().expect("four bytes");
    if big {
        u32::from_be_bytes(bytes)
    } else {
        u32::from_le_bytes(bytes)
    }
}

fn sector(image: &[u8], n: usize) -> Result<&[u8], String> {
    image
        .get(n * SECTOR..(n + 1) * SECTOR)
        .ok_or_else(|| format!("sector {n} is outside the image"))
}

fn walk(
    image: &[u8],
    dir: &[u8],
    prefix: &str,
    joliet: bool,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    let lba = be_or_le32(dir, 2, false) as usize;
    let len = be_or_le32(dir, 10, false) as usize;
    let body = image
        .get(lba * SECTOR..lba * SECTOR + len)
        .ok_or("directory outside the image")?;
    let mut pos = 0;
    while pos < body.len() {
        let rec_len = body[pos] as usize;
        if rec_len == 0 {
            pos = (pos / SECTOR + 1) * SECTOR;
            continue;
        }
        let rec = body
            .get(pos..pos + rec_len)
            .ok_or("record runs past its directory")?;
        pos += rec_len;
        let id_len = rec[32] as usize;
        let id = &rec[33..33 + id_len];
        if matches!(id, [0] | [1]) {
            continue;
        }
        let is_dir = rec[25] & 2 != 0;
        let name = if joliet {
            let units: Vec<u16> = id
                .chunks(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16(&units).map_err(|_| "bad Joliet name".to_owned())?
        } else {
            let text = String::from_utf8(id.to_vec()).map_err(|_| "bad ISO name".to_owned())?;
            if is_dir {
                text
            } else {
                text.strip_suffix(";1")
                    .ok_or("file version missing")?
                    .to_owned()
            }
        };
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if is_dir {
            walk(image, rec, &path, joliet, files)?;
        } else {
            let flba = be_or_le32(rec, 2, false) as usize;
            let flen = be_or_le32(rec, 10, false) as usize;
            let data = image
                .get(flba * SECTOR..flba * SECTOR + flen)
                .ok_or("file outside the image")?;
            files.insert(path, data.to_vec());
        }
    }
    Ok(())
}

fn image_of(entries: &[(&str, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let files: Vec<ImageFile> = entries
        .iter()
        .map(|(path, data)| ImageFile {
            path: (*path).to_owned(),
            size: data.len() as u64,
        })
        .collect();
    let mut out = Vec::new();
    write_image(
        &files,
        &mut |index: usize, w: &mut dyn Write| w.write_all(&entries[index].1).map_err(io_failed),
        &mut out,
    )?;
    Ok(out)
}

/// True for an ISO 9660 Level-1 name: up to 8 characters, optional `.` and up to 3 more, all of
/// A-Z, 0-9 and `_`.
fn is_level1(segment: &str) -> bool {
    let mut parts = segment.split('.');
    let stem = parts.next().unwrap_or("");
    let ext = parts.next();
    let ok_chars = |s: &str| {
        s.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    };
    parts.next().is_none()
        && (1..=8).contains(&stem.len())
        && ok_chars(stem)
        && ext.is_none_or(|e| e.len() <= 3 && ok_chars(e))
}

#[test]
fn nested_names_round_trip_through_joliet_and_iso_names_stay_level1() {
    let entries = vec![
        ("README.md", b"readme".to_vec()),
        ("src/lib/mod.rs", b"mod".to_vec()),
        ("src/lib/Long File Name With Spaces.txt", b"long".to_vec()),
        ("empty.txt", Vec::new()),
        ("big.bin", vec![7u8; 5000]),
        ("alpha-beta.txt", b"a".to_vec()),
        ("alpha_beta.txt", b"b".to_vec()),
        ("\u{fc}n\u{ef}/\u{6587}\u{4ef6}.txt", b"unicode".to_vec()),
        (".gitignore", b"x".to_vec()),
    ];
    let image = image_of(&entries).expect("image");
    assert_eq!(image.len() % SECTOR, 0);
    assert_eq!(volume_space_sectors(&image) as usize * SECTOR, image.len());

    let expected: BTreeMap<String, Vec<u8>> = entries
        .iter()
        .map(|(path, data)| ((*path).to_owned(), data.clone()))
        .collect();
    assert_eq!(read_volume(&image, true).expect("joliet"), expected);

    let iso = read_volume(&image, false).expect("iso");
    assert_eq!(iso.len(), entries.len());
    let mut iso_values: Vec<Vec<u8>> = iso.values().cloned().collect();
    let mut expected_values: Vec<Vec<u8>> = expected.values().cloned().collect();
    iso_values.sort();
    expected_values.sort();
    assert_eq!(iso_values, expected_values);
    for path in iso.keys() {
        assert!(path.split('/').all(is_level1), "not Level 1: {path}");
    }
}

#[test]
fn identical_input_gives_identical_bytes() {
    let entries = vec![("a/b.txt", b"one".to_vec()), ("c.txt", b"two".to_vec())];
    assert_eq!(
        image_of(&entries).expect("first"),
        image_of(&entries).expect("second")
    );
}

#[test]
fn many_records_in_one_directory_pack_without_crossing_sectors() {
    let names: Vec<String> = (0..300)
        .map(|i| format!("a-rather-long-directory-entry-name-{i:03}.dat"))
        .collect();
    let entries: Vec<(&str, Vec<u8>)> = names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), vec![i as u8; 3]))
        .collect();
    let image = image_of(&entries).expect("image");
    let joliet = read_volume(&image, true).expect("joliet");
    assert_eq!(joliet.len(), 300);
    for (name, data) in &entries {
        assert_eq!(joliet.get(*name), Some(data), "{name}");
    }
}

#[test]
fn descriptors_and_path_tables_describe_the_tree() {
    let image = image_of(&[("a/b.txt", b"x".to_vec()), ("c", b"y".to_vec())]).expect("image");
    assert_eq!(&image[16 * SECTOR + 1..16 * SECTOR + 6], STANDARD_ID);
    assert_eq!(image[16 * SECTOR], 1);
    assert_eq!(image[17 * SECTOR], 2);
    assert_eq!(&image[17 * SECTOR + 88..17 * SECTOR + 91], b"%/E");
    assert_eq!(image[18 * SECTOR], 255);
    for joliet in [false, true] {
        for big in [false, true] {
            assert_eq!(path_table_count(&image, joliet, big).expect("table"), 2);
        }
    }
}

#[test]
fn empty_file_list_is_a_valid_empty_image() {
    let image = image_of(&[]).expect("image");
    assert!(read_volume(&image, true).expect("joliet").is_empty());
    assert!(read_volume(&image, false).expect("iso").is_empty());
    assert_eq!(path_table_count(&image, true, true).expect("table"), 1);
}

#[test]
fn unrepresentable_names_are_refused_not_altered() {
    let long = "x".repeat(65);
    let cases: [(&str, &str); 6] = [
        ("bad:name", "unsupported_name"),
        ("back\\slash", "unsupported_name"),
        ("what?", "unsupported_name"),
        ("tab\tname", "unsupported_name"),
        (long.as_str(), "name_too_long"),
        ("dir/\u{1f600}/star*", "unsupported_name"),
    ];
    for (name, code) in cases {
        let err = image_of(&[(name, b"x".to_vec())]).expect_err(name);
        assert!(err.starts_with(code), "{name}: {err}");
    }
}

#[test]
fn malformed_or_conflicting_paths_are_refused() {
    let cases: [(&[&str], &str); 6] = [
        (&["a", "a/b"], "path_conflict"),
        (&["a/b", "a"], "duplicate_path"),
        (&["a", "a"], "duplicate_path"),
        (&["a//b"], "invalid_path"),
        (&["../x"], "invalid_path"),
        (&["a/./b"], "invalid_path"),
    ];
    for (paths, code) in cases {
        let entries: Vec<(&str, Vec<u8>)> = paths.iter().map(|p| (*p, b"x".to_vec())).collect();
        let err = image_of(&entries).expect_err(code);
        assert!(err.starts_with(code), "{paths:?}: {err}");
    }
}

#[test]
fn content_must_match_the_declared_size() {
    let files = [ImageFile {
        path: "a.txt".to_owned(),
        size: 4,
    }];
    let short = write_image(
        &files,
        &mut |_: usize, w: &mut dyn Write| w.write_all(b"abc").map_err(io_failed),
        &mut Vec::new(),
    );
    assert!(
        short
            .expect_err("short")
            .starts_with("content_size_mismatch")
    );
    let long = write_image(
        &files,
        &mut |_: usize, w: &mut dyn Write| w.write_all(b"abcdef").map_err(io_failed),
        &mut Vec::new(),
    );
    assert!(long.is_err(), "content longer than declared must fail");
}

#[test]
fn depth_and_joliet_path_length_limits_are_enforced() {
    let fit = format!(
        "{}/{}/{}/{}",
        "a".repeat(59),
        "b".repeat(59),
        "c".repeat(59),
        "d".repeat(60)
    );
    let over = format!(
        "{}/{}/{}/{}",
        "a".repeat(59),
        "b".repeat(59),
        "c".repeat(59),
        "d".repeat(61)
    );
    let deepest = "a/b/c/d/e/f/g/file.txt".to_owned();
    let too_deep = "a/b/c/d/e/f/g/h/file.txt".to_owned();
    let cases: [(String, Option<&str>); 4] = [
        (deepest, None),
        (too_deep, Some("too_deep")),
        (fit, None),
        (over, Some("path_too_long")),
    ];
    for (path, code) in cases {
        let result = image_of(&[(path.as_str(), b"x".to_vec())]);
        match code {
            None => {
                let image = result.expect("accepted");
                assert!(
                    read_volume(&image, true)
                        .expect("joliet")
                        .contains_key(&path)
                );
            }
            Some(code) => assert!(result.expect_err("refused").starts_with(code), "{code}"),
        }
    }
}

#[test]
fn directory_count_beyond_the_16_bit_path_table_is_refused() {
    // The path tables number parents in 16 bits, so the root plus 65,535 directories is too many.
    let names: Vec<String> = (0..usize::from(u16::MAX))
        .map(|i| format!("d{i}/f"))
        .collect();
    let entries: Vec<(&str, Vec<u8>)> = names.iter().map(|n| (n.as_str(), b"x".to_vec())).collect();
    let err = image_of(&entries).expect_err("too many directories");
    assert!(err.starts_with("too_many_directories"), "{err}");
}

#[test]
fn declared_sizes_beyond_the_format_are_refused_before_any_output() {
    let mut out = Vec::new();
    let mut never = |_: usize, _: &mut dyn Write| -> Result<(), String> {
        Err("content must not be requested".to_owned())
    };
    let huge = [ImageFile {
        path: "huge.bin".to_owned(),
        size: u64::from(u32::MAX) + 1,
    }];
    let err = write_image(&huge, &mut never, &mut out).expect_err("file too large");
    assert!(err.starts_with("file_too_large"), "{err}");
    // 2,600 files of 4 GiB - 1 each need more sectors than a 32-bit extent can address.
    let many: Vec<ImageFile> = (0..2600)
        .map(|i| ImageFile {
            path: format!("f{i}.bin"),
            size: u64::from(u32::MAX),
        })
        .collect();
    let err = write_image(&many, &mut never, &mut out).expect_err("image too large");
    assert!(err.starts_with("image_too_large"), "{err}");
    assert!(
        out.is_empty(),
        "nothing is written before the layout is known"
    );
}
