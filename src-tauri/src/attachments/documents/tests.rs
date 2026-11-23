use super::*;

fn pasted(name: &str, bytes: &[u8]) -> Document {
    Document {
        name: name.into(),
        path: None,
        data: Some(STANDARD.encode(bytes)),
    }
}

#[test]
fn mixed_sources_are_readable_by_the_paths_in_the_actual_send_request() {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let source = external.path().join("résumé.pdf");
    let pdf = b"%PDF-1.7\n\0\xff";
    fs::write(&source, pdf).unwrap();
    let binary = vec![0xf7; 1_100_000]; // Must not put base64 in the 1 MiB RPC frame.
    let docs = vec![
        Document {
            name: "résumé.pdf".into(),
            path: Some(source.to_str().unwrap().into()),
            data: None,
        },
        pasted("../../résumé.pdf", pdf),
        pasted("keep @secrets `x`", &binary),
        pasted("empty", b""),
    ];
    let (text, batch) = stage(project.path(), "Review these".into(), docs).unwrap();
    let (_, method, params) = crate::engine::Command::Send {
        text,
        images: vec![],
        inline_images: vec![],
        type_context: None,
    }
    .request();
    assert_eq!(method, "send");
    assert!(params.get("images").is_none() && params.get("inline_images").is_none());
    assert!(params.to_string().len() < 1024);
    let text = params["text"].as_str().unwrap();
    assert!(text.starts_with("Review these\n\n"));
    assert!(!text.contains("@secrets") && !text.contains("../"));
    let batch = batch.unwrap();
    let refs: Vec<_> = text
        .lines()
        .filter_map(|s| s.strip_prefix("- `")?.strip_suffix('`'))
        .collect();
    assert_eq!(refs.len(), 4);
    for (name, expected) in refs.iter().zip([&pdf[..], pdf, &binary, b""]) {
        let path = project.path().join(name);
        assert_eq!(path.parent(), Some(batch.path()));
        assert_eq!(fs::read(path).unwrap(), expected);
    }
    assert!(refs[0].ends_with(".pdf") && refs[1].ends_with(".pdf") && refs[0] != refs[1]);
    assert_eq!(fs::read(batch.path().join(".gitignore")).unwrap(), b"*\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(batch.path()).unwrap().permissions().mode() & 0o077,
            0
        );
    }
}

#[test]
fn invalid_batches_leave_no_partial_copies() {
    let root = tempfile::tempdir().unwrap();
    let source = tempfile::NamedTempFile::new().unwrap();
    source.as_file().set_len(MAX_IMAGE_BYTES + 1).unwrap();
    for doc in [
        Document {
            name: "folder".into(),
            path: Some(root.path().to_str().unwrap().into()),
            data: None,
        },
        Document {
            name: "too-large".into(),
            path: Some(source.path().to_str().unwrap().into()),
            data: None,
        },
        Document {
            name: "malformed".into(),
            path: None,
            data: Some("?".into()),
        },
        Document {
            name: "ambiguous".into(),
            path: Some("file".into()),
            data: Some(String::new()),
        },
    ] {
        assert!(stage(root.path(), String::new(), vec![pasted("good", b"ok"), doc]).is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
    let (text, batch) = stage(
        root.path(),
        String::new(),
        vec![pasted("LICENSE", b"license")],
    )
    .unwrap();
    assert!(
        !text.trim().is_empty() && batch.is_some(),
        "files alone make a valid prompt"
    );
}
