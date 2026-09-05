#![cfg(windows)]

use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use tauri::utils::config::Color;
use tauri::{
    command, AppHandle, Manager, PhysicalPosition, Position, WebviewUrl, WebviewWindowBuilder,
};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

const TRAY_POPUP_LABEL: &str = "tray-popup";
static OUTSIDE_CLICK_WATCHER_RUNNING: AtomicBool = AtomicBool::new(false);

fn watch_outside_click(window: tauri::WebviewWindow) {
    if OUTSIDE_CLICK_WATCHER_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    thread::spawn(move || {
        let mut was_down = unsafe { GetAsyncKeyState(VK_LBUTTON as i32) } < 0;
        loop {
            if !window.is_visible().unwrap_or(false) {
                break;
            }

            let is_down = unsafe { GetAsyncKeyState(VK_LBUTTON as i32) } < 0;
            if is_down && !was_down {
                let mut point = POINT { x: 0, y: 0 };
                if unsafe { GetCursorPos(&mut point) } != 0 {
                    if let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size())
                    {
                        let outside = point.x < position.x
                            || point.x >= position.x + size.width as i32
                            || point.y < position.y
                            || point.y >= position.y + size.height as i32;
                        if outside {
                            let _ = window.hide();
                            break;
                        }
                    }
                }
            }

            was_down = is_down;
            thread::sleep(Duration::from_millis(40));
        }
        OUTSIDE_CLICK_WATCHER_RUNNING.store(false, Ordering::Release);
    });
}

#[command]
pub fn show_tray_popup(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(TRAY_POPUP_LABEL) {
        position_tray_popup(&window, &app)?;
        if !window.is_visible().map_err(|e| e.to_string())? {
            window.show().map_err(|e| e.to_string())?;
            watch_outside_click(window.clone());
        }
        return Ok(());
    }

    let window =
        WebviewWindowBuilder::new(&app, TRAY_POPUP_LABEL, WebviewUrl::App("tray.html".into()))
            .title("TrayPopup")
            .inner_size(300.0, 440.0)
            .resizable(false)
            .decorations(false)
            .shadow(false)
            .transparent(true)
            .background_color(Color(0, 0, 0, 0))
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(true)
            .visible(false)
            .build()
            .map_err(|e| e.to_string())?;

    position_tray_popup(&window, &app)?;
    window.show().map_err(|e| e.to_string())?;
    watch_outside_click(window.clone());

    Ok(())
}

#[command]
pub fn hide_tray_popup(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(TRAY_POPUP_LABEL) {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn position_tray_popup(window: &tauri::WebviewWindow, app: &AppHandle) -> Result<(), String> {
    let tray = app.tray_by_id("main-tray").ok_or("Tray not found")?;
    let rect = tray
        .rect()
        .map_err(|e| e.to_string())?
        .ok_or("Tray rect unavailable")?;
    let scale = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("No monitor")?
        .scale_factor();
    let win_size = window.inner_size().map_err(|e| e.to_string())?;

    let (tray_x, tray_y) = match rect.position {
        Position::Physical(p) => (p.x, p.y),
        Position::Logical(p) => ((p.x * scale) as i32, (p.y * scale) as i32),
    };
    let (tray_w, tray_h) = match rect.size {
        tauri::Size::Physical(s) => (s.width as i32, s.height as i32),
        tauri::Size::Logical(s) => ((s.width * scale) as i32, (s.height * scale) as i32),
    };

    let tray_center_x = tray_x + tray_w / 2;

    let monitor = window
        .monitor_from_point(tray_center_x as f64, tray_y as f64)
        .map_err(|e| e.to_string())?
        .ok_or("No monitor")?;
    let monitor_pos = monitor.position();
    let monitor_size = monitor.size();
    let work_area = monitor.work_area();

    let work_left = work_area.position.x;
    let work_top = work_area.position.y;
    let work_right = work_left + work_area.size.width as i32;
    let work_bottom = work_top + work_area.size.height as i32;

    let monitor_right = monitor_pos.x + monitor_size.width as i32;
    let taskbar_left = work_left > monitor_pos.x;
    let taskbar_right = work_right < monitor_right;
    let taskbar_top = work_top > monitor_pos.y;

    let (mut x, mut y) = if taskbar_left {
        (
            tray_x + tray_w + 8,
            tray_y + tray_h / 2 - win_size.height as i32 / 2,
        )
    } else if taskbar_right {
        (
            tray_x - win_size.width as i32 - 8,
            tray_y + tray_h / 2 - win_size.height as i32 / 2,
        )
    } else if taskbar_top {
        (
            tray_center_x - win_size.width as i32 / 2,
            tray_y + tray_h + 8,
        )
    } else {
        (
            tray_center_x - win_size.width as i32 / 2,
            tray_y - win_size.height as i32 - 8,
        )
    };

    if y < work_top && !taskbar_left && !taskbar_right {
        y = tray_y + tray_h + 8;
    }

    if x + win_size.width as i32 > work_right {
        x = work_right - win_size.width as i32 - 8;
    }
    if x < work_left {
        x = work_left + 8;
    }
    if y + win_size.height as i32 > work_bottom {
        y = work_bottom - win_size.height as i32 - 8;
    }
    if y < work_top {
        y = work_top + 8;
    }

    window
        .set_position(Position::Physical(PhysicalPosition::new(x, y)))
        .map_err(|e| e.to_string())?;

    Ok(())
}
