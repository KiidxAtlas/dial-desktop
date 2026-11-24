//! Session-scoped Activity controls. Only the local app webview may read or post board messages.
use super::{Shared, State, check_session, rpc_reply};
use crate::engine::watch::Request;
use serde_json::Value;
use std::time::Duration;

async fn call(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    root: String,
    session: String,
    build: impl FnOnce(bool) -> Request,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("Only the app UI can access session Activity".into());
    }
    let reply = {
        let inner = shared.lock();
        inner.tabs.allow_work()?;
        check_session(&inner, &root, Some(&session))?;
        let tab = inner.tabs.active();
        if tab.opening_session() || tab.model.recovering {
            return Err("Wait for the conversation to open".into());
        }
        let request = build(tab.model.conversation.turn_running());
        tab.engine.as_ref().ok_or(super::PREVIEW_NOTICE)?.watch(session, request)?
    };
    rpc_reply(reply, Some(Duration::from_secs(20))).await
}

#[tauri::command]
pub(super) async fn watch_workers(webview: tauri::Webview, shared: State<'_, Shared>, workspace_root: String, session_id: String, id: Option<String>) -> Result<Value, String> {
    call(webview, shared, workspace_root, session_id, |running| Request::Workers { id, running }).await
}

#[tauri::command]
pub(super) async fn watch_message(webview: tauri::Webview, shared: State<'_, Shared>, workspace_root: String, session_id: String, id: String, text: String, watch_token: String) -> Result<Value, String> {
    call(webview, shared, workspace_root, session_id, |_| Request::Message { id, text, token: watch_token }).await
}

#[tauri::command]
pub(super) async fn watch_peers(webview: tauri::Webview, shared: State<'_, Shared>, workspace_root: String, session_id: String) -> Result<Value, String> {
    call(webview, shared, workspace_root, session_id, |_| Request::Peers).await
}

#[tauri::command]
pub(super) async fn watch_peer_message(webview: tauri::Webview, shared: State<'_, Shared>, workspace_root: String, session_id: String, id: String, text: String) -> Result<Value, String> {
    call(webview, shared, workspace_root, session_id, |_| Request::PeerMessage { id, text }).await
}
