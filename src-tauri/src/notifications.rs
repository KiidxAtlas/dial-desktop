//! Opt-in native session alerts. The OS sees generic text, never prompts or project names.
use crate::tabs::{NoticeKind, SessionNotice};
use serde::Serialize;
use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(not(target_os = "windows"))]
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

#[cfg(not(target_os = "windows"))]
const LIFETIME: Duration = Duration::from_secs(120);

#[derive(Default)]
pub struct Notifications(AtomicU8);

fn flag(kind: NoticeKind) -> u8 {
    match kind {
        NoticeKind::Approval => 1,
        NoticeKind::Question => 2,
        NoticeKind::Finished => 4,
    }
}

#[tauri::command]
pub async fn set_notifications(
    state: State<'_, Notifications>,
    enabled: bool,
    interactive: bool,
    approval: bool,
    question: bool,
    finished: bool,
) -> Result<(), String> {
    if enabled {
        authorize(interactive).await?;
    }
    let mut mask = 0;
    if enabled {
        for (on, kind) in [(approval, NoticeKind::Approval), (question, NoticeKind::Question), (finished, NoticeKind::Finished)] {
            if on { mask |= flag(kind); }
        }
    }
    state.0.store(mask, Ordering::Relaxed);
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

/// What a click on a notification opens.
#[derive(Clone)]
enum Target {
    Session(Destination),
    /// A saved type (`types`), by id.
    Type(String),
}

/// Whether the OS lets Dial show alerts; asks the user when it has not decided yet. Type
/// reminders need this without turning on session alerts.
#[tauri::command]
pub async fn type_reminders_allow() -> Result<(), String> {
    authorize(true).await
}

/// A reminder the user set on an item of a type. The text is what they saved and chose to be
/// reminded of, unlike session alerts.
#[tauri::command]
pub async fn type_reminder(
    app: AppHandle,
    type_id: String,
    title: String,
    body: String,
) -> Result<(), String> {
    let title: String = title.chars().take(80).collect();
    let body: String = body.chars().take(240).collect();
    deliver(&app, &title, &body, Target::Type(type_id)).await
}

pub fn publish(app: &AppHandle, workspace_root: &str, notices: Vec<SessionNotice>) {
    if notices.is_empty() || app.state::<Notifications>().0.load(Ordering::Relaxed) == 0 {
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
            if app.state::<Notifications>().0.load(Ordering::Relaxed) & flag(notice.kind) == 0 {
                continue;
            }
            let body = match notice.kind {
                NoticeKind::Approval => "A session needs your approval.",
                NoticeKind::Question => "A session has a question for you.",
                NoticeKind::Finished => "A session's turn ended.",
            };
            let destination = Target::Session(Destination {
                workspace_root: root.clone(),
                session_id: notice.session_id,
            });
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = deliver(&app, "Dial", body, destination).await {
                    eprintln!("dial-desktop: could not deliver a notification: {error}");
                }
            });
        }
    });
}

fn activate(app: &AppHandle, target: Target) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            let _ = match target {
                Target::Session(destination) => window.emit("notification-open", destination),
                Target::Type(id) => window.emit("type-open", id),
            };
        }
    });
}

#[cfg(target_os = "macos")]
async fn deliver(
    app: &AppHandle,
    title: &str,
    body: &str,
    destination: Target,
) -> Result<(), String> {
    let notification = mac_usernotifications::Notification::new()
        .title(title)
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
async fn deliver(
    app: &AppHandle,
    title: &str,
    body: &str,
    destination: Target,
) -> Result<(), String> {
    let app = app.clone();
    let app_id = app.config().identifier.clone();
    tauri_winrt_notification::Toast::new(&app_id)
        .title(title)
        .text1(body)
        .on_activated(move |_| {
            activate(&app, destination.clone());
            Ok(())
        })
        .show()
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
async fn deliver(
    app: &AppHandle,
    title: &str,
    body: &str,
    destination: Target,
) -> Result<(), String> {
    use notify_rust::{Notification, NotificationResponse, Timeout};
    let handle = Notification::new()
        .appname("Dial")
        .summary(title)
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
