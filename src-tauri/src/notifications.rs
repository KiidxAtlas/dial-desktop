//! Opt-in native session alerts. The OS sees generic text, never prompts or project names.
use crate::tabs::{NoticeKind, SessionNotice};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_os = "windows"))]
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

#[cfg(not(target_os = "windows"))]
const LIFETIME: Duration = Duration::from_secs(120);

#[derive(Default)]
pub struct Notifications(AtomicBool);

#[tauri::command]
pub async fn set_notifications(
    state: State<'_, Notifications>,
    enabled: bool,
    interactive: bool,
) -> Result<(), String> {
    if enabled {
        authorize(interactive).await?;
    }
    state.0.store(enabled, Ordering::Relaxed);
    Ok(())
}

#[cfg(target_os = "macos")]
async fn authorize(interactive: bool) -> Result<(), String> {
    use mac_usernotifications::AuthorizationStatus::{Authorized, Provisional};
    let granted = if interactive {
        mac_usernotifications::request_auth().await
    } else {
        mac_usernotifications::get_notification_settings()
            .await
            .map(|settings| matches!(settings.authorization_status, Authorized | Provisional))
    };
    match granted {
        Ok(true) => Ok(()),
        Ok(false) => Err("Allow Dial notifications in macOS System Settings first.".into()),
        Err(error) => Err(format!("Could not enable desktop notifications: {error}")),
    }
}

#[cfg(not(target_os = "macos"))]
async fn authorize(_: bool) -> Result<(), String> {
    Ok(())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Destination {
    workspace_root: String,
    session_id: String,
}

pub fn publish(app: &AppHandle, workspace_root: &str, notices: Vec<SessionNotice>) {
    if notices.is_empty() || !app.state::<Notifications>().0.load(Ordering::Relaxed) {
        return;
    }
    let app = app.clone();
    let root = workspace_root.to_owned();
    let _ = app.clone().run_on_main_thread(move || {
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        if window.is_focused().unwrap_or(true) {
            return;
        }
        for notice in notices {
            let body = match notice.kind {
                NoticeKind::Approval => "A session needs your approval.",
                NoticeKind::Question => "A session has a question for you.",
                NoticeKind::Finished => "A session's turn ended.",
            };
            let destination = Destination {
                workspace_root: root.clone(),
                session_id: notice.session_id,
            };
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = deliver(&app, body, destination).await {
                    eprintln!("dial-desktop: could not deliver a notification: {error}");
                }
            });
        }
    });
}

fn activate(app: &AppHandle, destination: Destination) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            let _ = window.emit("notification-open", destination);
        }
    });
}

#[cfg(target_os = "macos")]
async fn deliver(app: &AppHandle, body: &str, destination: Destination) -> Result<(), String> {
    let notification = mac_usernotifications::Notification::new()
        .title("Dial")
        .message(body)
        .timeout(LIFETIME);
    let response = mac_usernotifications::send_with_actions(notification)
        .await
        .map_err(|error| error.to_string())?;
    if response.is_default_action() {
        activate(app, destination);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
async fn deliver(app: &AppHandle, body: &str, destination: Destination) -> Result<(), String> {
    let app = app.clone();
    let app_id = app.config().identifier.clone();
    tauri_winrt_notification::Toast::new(&app_id)
        .title("Dial")
        .text1(body)
        .on_activated(move |_| {
            activate(&app, destination.clone());
            Ok(())
        })
        .show()
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
async fn deliver(app: &AppHandle, body: &str, destination: Destination) -> Result<(), String> {
    use notify_rust::{Notification, NotificationResponse, Timeout};
    let handle = Notification::new()
        .appname("Dial")
        .summary("Dial")
        .body(body)
        .action("default", "Open session")
        .action("open", "Open session")
        .timeout(Timeout::Milliseconds(LIFETIME.as_millis() as u32))
        .show_async()
        .await
        .map_err(|error| error.to_string())?;
    let wait = handle.wait_for_action_async(|response| {
        if matches!(response, NotificationResponse::Default)
            || matches!(response, NotificationResponse::Action(action) if action == "open")
        {
            activate(app, destination);
        }
    });
    if tokio::time::timeout(LIFETIME, wait).await.is_err() {
        handle.close_async().await;
    }
    Ok(())
}
