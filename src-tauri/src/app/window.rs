//! Native window lifecycle. Keep the last *normal* bounds, not the dimensions of a maximized
//! or full-screen window. Coordinates are logical offsets in the monitor's usable work area.
use super::Shared;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, Window};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

pub(super) const MIN_WIDTH: f64 = 640.;
pub(super) const MIN_HEIGHT: f64 = 440.;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Placement {
    monitor: Option<String>,
    width: f64,
    height: f64,
    x: Option<f64>,
    y: Option<f64>,
    maximized: bool,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            monitor: None,
            width: 1380.,
            height: 880.,
            x: None,
            y: None,
            maximized: false,
        }
    }
}

impl Placement {
    pub(super) fn load(path: Option<&Path>) -> Self {
        path.and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    // Center an unsaved window; clamp one from a larger, moved or disconnected screen.
    fn fit(&self, width: f64, height: f64, frame_w: f64, frame_h: f64) -> (f64, f64, f64, f64) {
        let max_w = (width - frame_w).max(1.);
        let max_h = (height - frame_h).max(1.);
        let w = self.width.clamp(MIN_WIDTH.min(max_w), max_w);
        let h = self.height.clamp(MIN_HEIGHT.min(max_h), max_h);
        let x = self.x.unwrap_or((max_w - w) / 2.).clamp(0., max_w - w);
        let y = self.y.unwrap_or((max_h - h) / 2.).clamp(0., max_h - h);
        (w, h, x, y)
    }

    pub(super) fn restore(&self, window: &Window) {
        let monitor = window
            .available_monitors()
            .unwrap_or_default()
            .into_iter()
            .find(|m| self.monitor.is_some() && m.name() == self.monitor.as_ref())
            .or_else(|| window.primary_monitor().ok().flatten());
        if let Some(m) = monitor {
            let scale = m.scale_factor();
            let area = m.work_area();
            let (frame_w, frame_h) = window
                .outer_size()
                .ok()
                .zip(window.inner_size().ok())
                .map(|(o, i)| {
                    (
                        o.width.saturating_sub(i.width),
                        o.height.saturating_sub(i.height),
                    )
                })
                .unwrap_or_default();
            let current_scale = window.scale_factor().unwrap_or(scale);
            let (w, h, x, y) = self.fit(
                f64::from(area.size.width) / scale,
                f64::from(area.size.height) / scale,
                f64::from(frame_w) / current_scale,
                f64::from(frame_h) / current_scale,
            );
            // Set position in desktop physical coordinates: different monitors may have different scales.
            let _ = window.set_position(PhysicalPosition::new(
                area.position.x.saturating_add((x * scale).round() as i32),
                area.position.y.saturating_add((y * scale).round() as i32),
            ));
            let _ =
                window.set_min_size(Some(LogicalSize::new(w.min(MIN_WIDTH), h.min(MIN_HEIGHT))));
            let _ = window.set_size(LogicalSize::new(w, h));
        }
        if self.maximized {
            let _ = window.maximize();
        }
    }

    pub(super) fn observe(&mut self, window: &Window) {
        if window.is_minimized().unwrap_or(true) || window.is_fullscreen().unwrap_or(true) {
            return;
        }
        let Ok(maximized) = window.is_maximized() else {
            return;
        };
        self.maximized = maximized;
        if maximized {
            return;
        }
        let (Ok(size), Ok(Some(m))) = (window.inner_size(), window.current_monitor()) else {
            return;
        };
        if size.width == 0 || size.height == 0 {
            return;
        }
        let scale = m.scale_factor();
        self.width = f64::from(size.width) / scale;
        self.height = f64::from(size.height) / scale;
        self.monitor = m.name().cloned();
        let origin = m.work_area().position;
        let pos = window.outer_position().ok();
        self.x = pos.map(|p| (f64::from(p.x) - f64::from(origin.x)) / scale);
        self.y = pos.map(|p| (f64::from(p.y) - f64::from(origin.y)) / scale);
    }

    pub(super) fn save(&self, path: Option<&Path>) {
        let Some(path) = path else { return };
        fn write(p: &Path, placement: &Placement) -> std::io::Result<()> {
            let dir = p.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
            std::fs::create_dir_all(dir)?;
            let file = tempfile::NamedTempFile::new_in(dir)?;
            serde_json::to_writer(file.as_file(), placement)?;
            file.persist(p)?;
            Ok(())
        }
        if let Err(e) = write(path, self) {
            eprintln!("dial-desktop: could not save window placement: {e}");
        }
    }
}

/// Work owned by this process that exit would interrupt. Only query it when asked to quit.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Inventory {
    sessions: usize,
    shells: usize,
    apps: usize,
    scheduler: bool,
    download: bool,
    host_setup: bool,
}

impl Inventory {
    fn current(app: &AppHandle) -> Self {
        let shared = app.state::<Shared>();
        let sessions = {
            let inner = shared.lock();
            inner.tabs.active_session_count() + inner.parked.values().map(|tabs| tabs.active_session_count()).sum::<usize>()
        };
        let shells = shared.terminals().open_ids().len();
        Self {
            sessions,
            shells,
            apps: app.state::<crate::app_runner::AppRunner>().open_count(),
            scheduler: app.state::<crate::scheduler::Scheduler>().running(),
            download: app.state::<crate::model_library::Downloads>().running(),
            host_setup: crate::app_runner::host::setup_busy(),
        }
    }

    fn message(self) -> String {
        let mut lines = vec!["Quitting this window will interrupt:".to_owned()];
        for (n, label) in [
            (self.sessions, "active session"),
            (self.shells, "terminal session"),
            (self.apps, "App panel connection"),
        ] {
            if n > 0 {
                lines.push(format!("• {n} {label}{}", if n == 1 { "" } else { "s" }));
            }
        }
        for (on, label) in [
            (self.scheduler, "Background scheduler"),
            (self.download, "Model download"),
            (self.host_setup, "App host setup"),
        ] {
            if on {
                lines.push(format!("• {label}"));
            }
        }
        lines.join("\n")
    }
}

#[derive(Default)]
enum Consent {
    #[default]
    None,
    Asking,
    Granted(Inventory),
}

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Exit,
    Wait,
    Ask,
}

impl Consent {
    /// Repeated native requests share one prompt. If new work appeared during confirmation,
    /// show the updated inventory rather than consuming consent for different work.
    fn request(&mut self, work: Inventory) -> Decision {
        if matches!(self, Self::Asking) {
            return Decision::Wait;
        }
        if work == Inventory::default() || matches!(self, Self::Granted(old) if *old == work) {
            return Decision::Exit;
        }
        *self = Self::Asking;
        Decision::Ask
    }
}
#[derive(Default)]
pub(super) struct Quit(Arc<Mutex<Consent>>);
impl Quit {
    pub(super) fn prevent(&self, window: &Window) -> bool {
        let inventory = Inventory::current(window.app_handle());
        match self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .request(inventory)
        {
            Decision::Exit => return false,
            Decision::Wait => return true,
            Decision::Ask => {}
        }
        let state = self.0.clone();
        let app = window.app_handle().clone();
        app.dialog()
            .message(inventory.message())
            .title("Quit Dial?")
            .parent(window)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Quit Dial".into(),
                "Keep working".into(),
            ))
            .show(move |ok| {
                *state.lock().unwrap_or_else(|e| e.into_inner()) = if ok {
                    Consent::Granted(inventory)
                } else {
                    Consent::None
                };
                if ok {
                    app.exit(0);
                }
            });
        true
    }
}

/// The macOS default Quit item invokes NSApplication.terminate: directly, outside Tauri's
/// cancellable ExitRequested path. Keep the native menus but route that item (and Cmd-Q)
/// through the same exit request as a close button.
#[cfg(target_os = "macos")]
pub(super) fn menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem};
    let menu = Menu::default(app)?;
    if let Some(sub) = menu.items()?.first().and_then(|m| m.as_submenu()).cloned() {
        for (i, item) in sub.items()?.iter().enumerate() {
            if let Some(predefined) = item.as_predefined_menuitem() {
                let title = predefined.text()?;
                if title == "Quit" || title.starts_with("Quit ") {
                    let quit = MenuItem::with_id(app, "dial-quit", title, true, Some("Cmd+Q"))?;
                    sub.remove_at(i)?;
                    sub.insert(&quit, i)?;
                    break;
                }
            }
        }
    }
    Ok(menu)
}

#[cfg(test)]
mod tests {
    #[test]
    fn quit_shares_pending_dialog_and_rechecks_work_after_consent() {
        let mut consent = Consent::default();
        let idle = Inventory::default();
        let active = Inventory {
            sessions: 1,
            ..idle
        };
        assert_eq!(consent.request(idle), Decision::Exit);
        assert_eq!(consent.request(active), Decision::Ask);
        assert_eq!(consent.request(active), Decision::Wait);
        consent = Consent::Granted(active);
        assert_eq!(
            consent.request(Inventory {
                shells: 1,
                ..active
            }),
            Decision::Ask
        );
        consent = Consent::Granted(active);
        assert_eq!(consent.request(active), Decision::Exit);
    }
    use super::*;
    #[test]
    fn placement_respects_work_area_and_frame_after_display_changes() {
        let stored = Placement {
            x: Some(4000.),
            y: Some(-10.),
            width: 900.,
            height: 1200.,
            ..Placement::default()
        };
        assert_eq!(stored.fit(1200., 800., 10., 30.), (900., 770., 290., 0.));
        assert_eq!(stored.fit(520., 390., 20., 30.), (500., 360., 0., 0.));
        assert_eq!(
            Placement::default().fit(1920., 1080., 0., 0.),
            (1380., 880., 270., 100.)
        );
    }
}
