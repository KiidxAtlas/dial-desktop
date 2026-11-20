use tauri::ipc::Origin;

#[test]
fn desktop_plugins_are_granted_only_to_the_local_app_webview() {
    let mut context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    let acl = context.runtime_authority_mut();
    let remote = Origin::Remote {
        url: "https://example.org/".parse().unwrap(),
    };
    for command in [
        "plugin:event|listen",
        "plugin:event|unlisten",
        "plugin:app|version",
        "plugin:dialog|message",
        "plugin:dialog|open",
        "plugin:dialog|save",
        "plugin:opener|open_url",
        "plugin:opener|reveal_item_in_dir",
        "plugin:window|start_dragging",
        "plugin:window|toggle_maximize",
        "plugin:webview|set_webview_zoom",
        "plugin:updater|check",
        "plugin:process|restart",
    ] {
        let allowed = |view, origin| acl.resolve_access(command, "main", view, origin).is_some();
        assert!(allowed("main", &Origin::Local), "missing {command}");
        assert!(!allowed("main", &remote), "remote grant: {command}");
        assert!(
            !allowed(dial_desktop::browser::LABEL, &Origin::Local),
            "browser grant: {command}"
        );
    }
}
