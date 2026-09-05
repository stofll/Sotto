//! Main window size between launches.
//!
//! Writing is deliberately decoupled from the `Resized` event: one drag of the
//! frame makes Windows send dozens of events, and writing each would mean dozens
//! of consecutive disk writes — all with intermediate sizes nobody needs. The
//! current geometry lives in memory, reaches disk at most once a second, and the
//! final state is flushed when the window closes.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{Manager, WindowEvent};

/// Matches `tauri.conf.json` → `app.windows[0]`: `minWidth` and `minHeight` are
/// set there too, so smaller sizes can never be the saved ones.
const DEFAULT_WIDTH: f64 = 1000.0;
const DEFAULT_HEIGHT: f64 = 710.0;
const WRITE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Geometry {
    width: f64,
    height: f64,
    maximized: bool,
}

struct State {
    geometry: Option<Geometry>,
    /// Changes that have not reached disk yet.
    dirty: bool,
    last_write: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    geometry: None,
    dirty: false,
    last_write: None,
});

fn path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    crate::config::config_path(app)
        .ok()
        .map(|p| p.with_file_name("window.json"))
}

fn read(app: &tauri::AppHandle) -> Option<Geometry> {
    path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice::<Geometry>(&bytes).ok())
}

fn write(app: &tauri::AppHandle, geometry: &Geometry) {
    let Some(path) = path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec(geometry) {
        let _ = std::fs::write(path, bytes);
    }
}

/// A maximized window does not report the size it will return to — it must
/// carry its previous size through to the file untouched.
fn merge(old: Option<Geometry>, width: f64, height: f64, maximized: bool) -> Geometry {
    if maximized {
        let old = old.unwrap_or(Geometry {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            maximized: false,
        });
        Geometry { maximized, ..old }
    } else {
        Geometry {
            width,
            height,
            maximized,
        }
    }
}

fn due_for_write(last_write: Option<Instant>, now: Instant) -> bool {
    match last_write {
        None => true,
        Some(previous) => now.duration_since(previous) >= WRITE_INTERVAL,
    }
}

/// Flushes whatever the timer did not manage to write.
fn flush(app: &tauri::AppHandle) {
    let mut guard = crate::mutex_recover::lock(&STATE);
    if !guard.dirty {
        return;
    }
    let Some(geometry) = guard.geometry.clone() else {
        return;
    };
    guard.dirty = false;
    guard.last_write = Some(Instant::now());
    // The file is written without holding the lock: disk access must not stall
    // the window event thread.
    drop(guard);
    write(app, &geometry);
}

pub fn restore(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let geometry = read(app);
    if let Some(g) = geometry {
        if g.width.is_finite()
            && g.height.is_finite()
            && g.width >= DEFAULT_WIDTH
            && g.height >= DEFAULT_HEIGHT
        {
            let _ = window.set_size(tauri::LogicalSize::new(g.width, g.height));
            if g.maximized {
                let _ = window.maximize();
            }
        }
        // What was read goes into memory in full, even if the size was rejected:
        // otherwise the very first `Resized` would go and re-read the same file.
        crate::mutex_recover::lock(&STATE).geometry = Some(g);
    }
}

pub fn handle(window: &tauri::Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let _ = window.hide();
        flush(window.app_handle());
        if let Some(state) = window.try_state::<crate::state::AppState>() {
            let test = state.microphone_test.clone();
            let app = window.app_handle().clone();
            state.audio.submit(move || {
                let _ = test.stop(&app);
            });
        }
    }
    // Windows are not closed only by the X: "Quit" in the tray destroys the
    // window bypassing `CloseRequested`, and without this branch the last size
    // would be lost.
    if let WindowEvent::Destroyed = event {
        flush(window.app_handle());
    }
    if let WindowEvent::Resized(size) = event {
        if size.width == 0 || size.height == 0 || window.is_minimized().unwrap_or(false) {
            return;
        }
        let app = window.app_handle();
        let maximized = window.is_maximized().unwrap_or(false);
        let logical = size.to_logical::<f64>(window.scale_factor().unwrap_or(1.0));
        let mut guard = crate::mutex_recover::lock(&STATE);
        let old = guard.geometry.clone().or_else(|| read(app));
        let geometry = merge(old, logical.width, logical.height, maximized);
        if guard.geometry.as_ref() == Some(&geometry) {
            return;
        }
        let now = Instant::now();
        guard.geometry = Some(geometry.clone());
        if !due_for_write(guard.last_write, now) {
            // The intermediate size stays in memory: it will be written either
            // by the next event after a pause, or by the window closing.
            guard.dirty = true;
            return;
        }
        guard.dirty = false;
        guard.last_write = Some(now);
        drop(guard);
        write(app, &geometry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximizing_keeps_the_size_the_window_will_return_to() {
        let old = Geometry {
            width: 1280.0,
            height: 800.0,
            maximized: false,
        };
        let merged = merge(Some(old), 2560.0, 1440.0, true);
        assert_eq!(merged.width, 1280.0);
        assert_eq!(merged.height, 800.0);
        assert!(merged.maximized);
    }

    #[test]
    fn maximizing_without_a_previous_size_falls_back_to_the_window_defaults() {
        let merged = merge(None, 2560.0, 1440.0, true);
        assert_eq!(merged.width, DEFAULT_WIDTH);
        assert_eq!(merged.height, DEFAULT_HEIGHT);
    }

    #[test]
    fn a_normal_resize_records_the_new_size() {
        let old = Geometry {
            width: 1280.0,
            height: 800.0,
            maximized: true,
        };
        let merged = merge(Some(old), 1400.0, 900.0, false);
        assert_eq!(merged.width, 1400.0);
        assert_eq!(merged.height, 900.0);
        assert!(!merged.maximized);
    }

    #[test]
    fn writes_are_throttled_between_intervals() {
        let now = Instant::now();
        assert!(due_for_write(None, now));
        assert!(!due_for_write(Some(now), now + WRITE_INTERVAL / 2));
        assert!(due_for_write(Some(now), now + WRITE_INTERVAL));
    }
}
