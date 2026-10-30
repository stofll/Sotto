//! Main window size between launches.
//!
//! Writing is deliberately decoupled from the `Resized` event: one drag of the
//! frame makes Windows send dozens of events, and writing each would mean dozens
//! of consecutive disk writes — all with intermediate sizes nobody needs. The
//! current geometry lives in memory and reaches disk at most once a second; the
//! last size of a drag follows once that second is over. The updater ends the
//! process without closing the window, so waiting for the close would lose it.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{Manager, WindowEvent};

/// Matches `tauri.conf.json` → `app.windows[0]` `minWidth` and `minHeight`. A
/// frameless window can still save a pixel less; `restored_size` clamps it.
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
    /// A deferred write is already waiting for the interval to pass.
    flush_pending: bool,
    last_write: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    geometry: None,
    dirty: false,
    flush_pending: false,
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

/// Replaces the file through a sibling temporary one: the updater can end the
/// process mid-write, and a truncated file would lose the size altogether.
fn write(app: &tauri::AppHandle, geometry: &Geometry) {
    let Some(path) = path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(bytes) = serde_json::to_vec(geometry) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
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

/// The saved size within the window's minimum. A frameless window can report
/// a pixel less than `minWidth`, and rejecting that would drop the whole size.
fn restored_size(geometry: &Geometry) -> Option<(f64, f64)> {
    let valid = |value: f64| value.is_finite() && value > 0.0;
    (valid(geometry.width) && valid(geometry.height)).then(|| {
        (
            geometry.width.max(DEFAULT_WIDTH),
            geometry.height.max(DEFAULT_HEIGHT),
        )
    })
}

/// The restored size no larger than the work area of the monitor the window
/// opens on: it may have been saved on a bigger screen or at a lower scaling.
/// The minimum still wins, as the OS enforces it anyway.
fn fitted_size((width, height): (f64, f64), area: Option<(f64, f64)>) -> (f64, f64) {
    let Some((area_width, area_height)) = area else {
        return (width, height);
    };
    (
        width.min(area_width).max(DEFAULT_WIDTH),
        height.min(area_height).max(DEFAULT_HEIGHT),
    )
}

/// Where to move a window that sticks out of the work area: centred in it, or
/// at its top-left corner when the window is still the larger. `None` when the
/// window already fits. Physical pixels: `(x, y, width, height)`.
fn placed_within(window: (i64, i64, i64, i64), area: (i64, i64, i64, i64)) -> Option<(i64, i64)> {
    let (x, y, width, height) = window;
    let (area_x, area_y, area_width, area_height) = area;
    let fits = x >= area_x
        && y >= area_y
        && x + width <= area_x + area_width
        && y + height <= area_y + area_height;
    (!fits).then(|| {
        (
            area_x + ((area_width - width) / 2).max(0),
            area_y + ((area_height - height) / 2).max(0),
        )
    })
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
        if let Some(size) = restored_size(&g) {
            // Without a saved position the window opens where the OS puts it,
            // so the monitor is known only now.
            let monitor = window
                .current_monitor()
                .ok()
                .flatten()
                .or_else(|| window.primary_monitor().ok().flatten());
            let area = monitor.as_ref().map(|monitor| {
                let (rect, scale) = (monitor.work_area(), monitor.scale_factor());
                (
                    rect,
                    scale,
                    (
                        f64::from(rect.size.width) / scale,
                        f64::from(rect.size.height) / scale,
                    ),
                )
            });
            let (width, height) = fitted_size(size, area.map(|(_, _, logical)| logical));
            let _ = window.set_size(tauri::LogicalSize::new(width, height));
            if let (Some((rect, scale, _)), Ok(origin)) = (area, window.outer_position()) {
                let physical = |value: f64| (value * scale).round() as i64;
                let target = placed_within(
                    (
                        i64::from(origin.x),
                        i64::from(origin.y),
                        physical(width),
                        physical(height),
                    ),
                    (
                        i64::from(rect.position.x),
                        i64::from(rect.position.y),
                        i64::from(rect.size.width),
                        i64::from(rect.size.height),
                    ),
                );
                if let Some((x, y)) = target {
                    let _ = window.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
                }
            }
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
            let _ = state.audio.submit(move || {
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
            guard.dirty = true;
            if !guard.flush_pending {
                guard.flush_pending = true;
                let wait = guard.last_write.map_or(Duration::ZERO, |last| {
                    WRITE_INTERVAL.saturating_sub(now.duration_since(last))
                });
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(wait).await;
                    crate::mutex_recover::lock(&STATE).flush_pending = false;
                    flush(&app);
                });
            }
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
    fn a_size_just_under_the_minimum_is_kept_within_it() {
        let saved = |width, height| Geometry {
            width,
            height,
            maximized: false,
        };
        assert_eq!(
            restored_size(&saved(999.0, 721.0)),
            Some((DEFAULT_WIDTH, 721.0))
        );
        assert_eq!(restored_size(&saved(1280.0, 800.0)), Some((1280.0, 800.0)));
        assert_eq!(restored_size(&saved(f64::NAN, 800.0)), None);
        assert_eq!(restored_size(&saved(1280.0, 0.0)), None);
    }

    #[test]
    fn a_size_saved_on_a_larger_screen_fits_the_work_area() {
        assert_eq!(
            fitted_size((2400.0, 1500.0), Some((1536.0, 816.0))),
            (1536.0, 816.0)
        );
        assert_eq!(
            fitted_size((1280.0, 800.0), Some((1920.0, 1032.0))),
            (1280.0, 800.0)
        );
        // A screen below the minimum keeps the minimum: the OS would enforce it anyway.
        assert_eq!(
            fitted_size((1280.0, 800.0), Some((1093.0, 683.0))),
            (1093.0, DEFAULT_HEIGHT)
        );
        assert_eq!(fitted_size((2400.0, 1500.0), None), (2400.0, 1500.0));
    }

    #[test]
    fn a_window_sticking_out_of_the_work_area_is_centred_in_it() {
        let area = (0, 0, 1920, 1032);
        assert_eq!(placed_within((26, 26, 1280, 800), area), None);
        assert_eq!(placed_within((26, 26, 1920, 1032), area), Some((0, 0)));
        assert_eq!(placed_within((800, 400, 1280, 800), area), Some((320, 116)));
        // A secondary monitor left of the primary one has a negative origin.
        assert_eq!(
            placed_within((-100, 50, 1000, 710), (-1920, 0, 1920, 1040)),
            Some((-1460, 165))
        );
        // A window taller than the area is pinned to its top rather than pushed above it.
        assert_eq!(
            placed_within((0, 0, 1000, 710), (0, 0, 1093, 683)),
            Some((46, 0))
        );
    }

    #[test]
    fn writes_are_throttled_between_intervals() {
        let now = Instant::now();
        assert!(due_for_write(None, now));
        assert!(!due_for_write(Some(now), now + WRITE_INTERVAL / 2));
        assert!(due_for_write(Some(now), now + WRITE_INTERVAL));
    }
}
