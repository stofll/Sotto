//! Размер главного окна между запусками.
//!
//! Запись отделена от события `Resized` намеренно: за одно протягивание рамки
//! Windows присылает десятки событий, и запись каждого означала бы десятки
//! обращений к диску подряд — все с промежуточными размерами, которые никому
//! не нужны. Актуальная геометрия живёт в памяти, на диск уходит не чаще
//! раза в секунду, а последнее состояние досылается при закрытии окна.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{Manager, WindowEvent};

/// Совпадает с `tauri.conf.json` → `app.windows[0]`: там же заданы `minWidth`
/// и `minHeight`, поэтому меньшие размеры сохранёнными быть не могут.
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
    /// Изменения, которые ещё не дошли до диска.
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

/// Развёрнутое окно не сообщает размер, к которому оно вернётся, — свой
/// прошлый размер оно должно донести до файла нетронутым.
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

/// Досылает на диск то, что не успело уйти по таймеру.
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
    // Файл пишется без удержания замка: обращение к диску не должно
    // задерживать поток событий окна.
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
        // Прочитанное кладётся в память целиком, даже если размер отвергнут:
        // иначе первое же `Resized` пошло бы перечитывать тот же файл.
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
    // Окно закрывают не только крестиком: «Выход» в трее рушит окно, минуя
    // `CloseRequested`, и без этой ветки последний размер терялся бы.
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
            // Промежуточный размер остаётся в памяти: его допишет либо
            // следующее событие после паузы, либо закрытие окна.
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
