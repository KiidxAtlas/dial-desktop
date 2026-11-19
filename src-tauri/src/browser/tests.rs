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
