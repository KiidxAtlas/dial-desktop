use super::*;

#[test]
fn addresses_are_completed_and_other_input_searches() {
    assert_eq!(
        normalize_url("https://tauri.app/x").unwrap(),
        "https://tauri.app/x"
    );
    assert_eq!(
        normalize_url("  example.com ").unwrap(),
        "https://example.com/"
    );
    assert_eq!(
        normalize_url("example.com/docs?a=1").unwrap(),
        "https://example.com/docs?a=1"
    );
    assert_eq!(
        normalize_url("localhost:5173").unwrap(),
        "http://localhost:5173/"
    );
    assert_eq!(
        normalize_url("127.0.0.1:8080/app").unwrap(),
        "http://127.0.0.1:8080/app"
    );
    assert_eq!(normalize_url("[::1]:3000").unwrap(), "http://[::1]:3000/");
    assert_eq!(
        normalize_url("app.localhost:3000").unwrap(),
        "http://app.localhost:3000/"
    );
    assert_eq!(normalize_url("about:blank").unwrap(), "about:blank");
    assert_eq!(
        normalize_url("rust borrow checker").unwrap(),
        "https://duckduckgo.com/?q=rust+borrow+checker"
    );
    assert_eq!(
        normalize_url("vue").unwrap(),
        "https://duckduckgo.com/?q=vue"
    );
    assert!(normalize_url("   ").is_err());
}

#[test]
fn only_web_pages_open() {
    assert!(normalize_url("file:///etc/passwd").is_err());
    assert!(normalize_url("tauri://localhost").is_err());
    assert!(
        normalize_url("javascript:alert(1)")
            .unwrap()
            .starts_with(SEARCH)
    );
    for (url, ok) in [
        ("https://a.b/", true),
        ("http://localhost:1/", true),
        ("about:blank", true),
        ("file:///etc/hosts", false),
        ("tauri://localhost/", false),
        ("javascript:alert(1)", false),
    ] {
        assert_eq!(allowed(&Url::parse(url).unwrap()), ok, "{url}");
    }
}

#[test]
fn page_info_is_read_whether_or_not_it_is_wrapped() {
    let page =
        r#"{"url":"http://x/","title":"X","selection":"sel","text":"body","errors":["boom"]}"#;
    let direct = parse_page(page).unwrap();
    assert_eq!(direct.title, "X");
    assert_eq!(direct.errors, ["boom"]);
    let wrapped = serde_json::to_string(page).unwrap();
    assert_eq!(parse_page(&wrapped).unwrap(), direct);
    assert_eq!(parse_page(r#"{"url":"u"}"#).unwrap().url, "u");
    assert!(parse_page("null").is_err());
}

#[test]
fn searches_use_the_chosen_engine_template() {
    for (template, query, url) in [
        ("https://www.google.com/search?q=%s", "rust borrow", "https://www.google.com/search?q=rust+borrow"),
        ("https://kagi.com/search?q=%s", "a&b=c#d", "https://kagi.com/search?q=a%26b%3Dc%23d"),
        ("https://search.brave.com/search?q=%s", "ünï", "https://search.brave.com/search?q=%C3%BCn%C3%AF"),
    ] {
        assert_eq!(normalize_url_with(query, Some(template)).unwrap(), url, "{template}");
    }
    // Addresses are not searched, and no template falls back to DuckDuckGo.
    assert_eq!(normalize_url_with("example.com", Some("https://g.co/?q=%s")).unwrap(), "https://example.com/");
    assert_eq!(normalize_url_with("vue", Some("  ")).unwrap(), "https://duckduckgo.com/?q=vue");
    for bad in ["https://g.co/?q=", "javascript:%s", "file:///%s", "not a url %s"] {
        assert!(check_search_template(bad).is_err(), "{bad}");
        assert!(normalize_url_with("words here", Some(bad)).is_err(), "{bad}");
    }
}

#[test]
fn opening_a_file_serves_its_folder_until_the_page_leaves() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("my page.html"), "<p>hi</p>").unwrap();
    let browser = Browser::default();
    let url = serve_file(&browser, &dir.path().join("my page.html")).unwrap();
    assert!(url.starts_with("http://127.0.0.1:") && url.ends_with("/my%20page.html"), "{url}");
    assert!(allowed(&Url::parse(&url).unwrap()));
    browser.keep_local_for(&url);
    assert!(browser.local.lock().unwrap().is_some());
    browser.keep_local_for("https://example.com/");
    assert!(browser.local.lock().unwrap().is_none());
    assert!(serve_file(&browser, dir.path()).is_err());
}
