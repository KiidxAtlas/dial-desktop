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
fn opening_a_file_serves_its_folder_until_the_tab_leaves() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("my page.html"), "<p>hi</p>").unwrap();
    let browser = Browser::default();
    let (id, _) = browser.tabs().ensure();
    let url = serve_file(&browser, id, &dir.path().join("my page.html")).unwrap();
    assert!(url.starts_with("http://127.0.0.1:") && url.ends_with("/my%20page.html"), "{url}");
    assert!(allowed(&Url::parse(&url).unwrap()));
    browser.tabs().get_mut(id).unwrap().keep_local(&url);
    assert!(browser.tabs().get_mut(id).unwrap().local.is_some());
    browser.tabs().get_mut(id).unwrap().keep_local("https://example.com/");
    assert!(browser.tabs().get_mut(id).unwrap().local.is_none());
    assert!(serve_file(&browser, id, dir.path()).is_err());
    assert!(serve_file(&browser, id + 1, &dir.path().join("my page.html")).is_err());
}

#[test]
fn browser_features_enables_devtools() {
    assert!(browser_features().devtools);
}

/// A tab list with `urls` as tabs, the last one active; ids are 1, 2, ….
fn tabs_with(urls: &[&str]) -> Tabs {
    let mut tabs = Tabs::default();
    for url in urls {
        tabs.add(url).unwrap();
    }
    tabs
}

#[test]
fn tabs_are_listed_in_order_with_the_active_one_named() {
    let mut tabs = tabs_with(&["https://a.test/", "https://b.test/"]);
    let state = tabs.state();
    assert_eq!(state.active_tab, 2);
    assert_eq!(state.url, "https://b.test/");
    assert_eq!(
        state.tabs,
        [
            TabState { id: 1, url: "https://a.test/".into(), title: String::new() },
            TabState { id: 2, url: "https://b.test/".into(), title: String::new() },
        ]
    );
    tabs.select(1).unwrap();
    assert_eq!(tabs.state().url, "https://a.test/");
    assert!(tabs.select(9).is_err());
    assert_eq!(tabs.state().active_tab, 1, "a failed select changes nothing");
}

#[test]
fn closing_a_tab_keeps_the_others_and_activates_a_neighbour() {
    let mut tabs = tabs_with(&["a", "b", "c"]);
    tabs.select(2).unwrap();
    tabs.remove(3).unwrap();
    assert_eq!(tabs.state().active_tab, 2, "closing another tab keeps the active one");
    tabs.remove(2).unwrap();
    assert_eq!(tabs.state().active_tab, 1, "the tab before takes over");
    assert_eq!(tabs.state().tabs.len(), 1);
    assert!(tabs.remove(2).is_err(), "closing twice is refused");
}

#[test]
fn closing_the_last_tab_leaves_a_blank_one() {
    let mut tabs = tabs_with(&["https://only.test/"]);
    tabs.remove(1).unwrap();
    let state = tabs.state();
    assert_eq!(state.tabs.len(), 1);
    assert_eq!(state.active_tab, state.tabs[0].id);
    assert_eq!(state.url, BLANK);
    assert_ne!(state.active_tab, 1, "ids are never reused");
}

#[test]
fn the_tab_count_is_capped() {
    let mut tabs = Tabs::default();
    for _ in 0..MAX_TABS {
        tabs.add(BLANK).unwrap();
    }
    assert!(tabs.add(BLANK).is_err());
    assert_eq!(tabs.state().tabs.len(), MAX_TABS);
}

#[test]
fn an_empty_browser_has_no_active_tab_until_one_is_ensured() {
    let mut tabs = Tabs::default();
    let state = tabs.state();
    assert_eq!((state.active_tab, state.tabs.len()), (0, 0));
    assert_eq!(tabs.ensure(), (1, true));
    assert_eq!(tabs.ensure(), (1, false));
}

#[test]
fn a_navigation_counts_only_when_it_is_the_active_tabs_exact_finished_address() {
    let url = Url::parse("http://localhost:3000/").unwrap();
    let mut tabs = tabs_with(&["http://localhost:3000/", "https://x.test/"]);
    let finished = |tabs: &mut Tabs, id: u64, started: u64, page: &str, loading: bool| {
        let tab = tabs.get_mut(id).unwrap();
        tab.started = started;
        tab.started_url = page.to_owned();
        tab.loading = loading;
    };
    for (page, started, loading, since, expected, case) in [
        // A same-origin page from before the call is not the page the agent asked for.
        ("http://localhost:3000/", 5, false, 5, false, "started before the call"),
        ("http://localhost:3000/other", 6, false, 5, false, "same origin, other path"),
        ("http://localhost:3000/", 6, true, 5, false, "still loading"),
        ("http://localhost:3000/", 6, false, 5, true, "exact and finished"),
    ] {
        finished(&mut tabs, 2, started, page, loading);
        let nav = tabs.opened(&url, since);
        assert_eq!(nav.is_some(), expected, "{case}");
    }
    // A finished exact navigation in a background tab does not count while another tab is active.
    finished(&mut tabs, 2, 6, "https://x.test/", false);
    finished(&mut tabs, 1, 9, "http://localhost:3000/", false);
    assert!(tabs.opened(&url, 5).is_none(), "background tab");
}

#[test]
fn an_active_tab_redirect_counts_by_the_address_it_started_with() {
    let mut tabs = tabs_with(&["https://x.test/"]);
    let tab = tabs.get_mut(1).unwrap();
    tab.started = 3;
    tab.started_url = "https://example.com/".into();
    tab.url = "https://www.example.com/".into();
    assert!(tabs.opened(&Url::parse("https://example.com/").unwrap(), 0).is_some());
}
