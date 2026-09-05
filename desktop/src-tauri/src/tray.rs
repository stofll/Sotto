use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{image::Image, Manager};

#[cfg(windows)]
use crate::windows::tray_popup::{hide_tray_popup, show_tray_popup};

/// Tray icon loaded from `icons/tray.png` (PNG decoded via the `image-png`
/// Tauri feature). Baked into the binary so there is no runtime file lookup.
fn tray_icon_image() -> Image<'static> {
    Image::from_bytes(include_bytes!("../icons/tray.png"))
        .expect("tray.png is a valid PNG embedded at build time")
}

pub fn build_tray(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    // Right-click context menu with a single "Quit" entry.
    let quit_item = MenuItem::with_id(app, "quit", crate::ui_text::t("Выход"), true, None::<&str>)?;
    let open_item = MenuItem::with_id(app, "open", "Sotto", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_item, &quit_item])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(tray_icon_image())
        .tooltip("Sotto")
        .menu(&menu)
        // Keep left-click for the popup toggle on Windows; show the menu on right-click only.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if event.id.as_ref() == "open" {
                let _ = crate::focus_main_window(app.clone(), "settings".into());
            }
            if event.id.as_ref() == "quit" {
                app.exit(0);
            }
        })
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                #[cfg(windows)]
                {
                    let app = app.clone();
                    // WebView2 creation must not block the Windows event callback.
                    std::thread::spawn(move || {
                        let result = if app
                            .get_webview_window("tray-popup")
                            .is_some_and(|p| p.is_visible().unwrap_or(false))
                        {
                            hide_tray_popup(app.clone())
                        } else {
                            show_tray_popup(app.clone())
                        };
                        if let Err(error) = result {
                            log::warn!("tray popup: {error}");
                            let _ = crate::focus_main_window(app, "settings".into());
                        }
                    });
                }
                #[cfg(not(windows))]
                {
                    // TODO(macOS): native tray popup — Phase 6.
                    // For Phase 1, left-click shows or focuses the main window,
                    // which is the least surprising behavior on macOS where the
                    // tray menu already provides settings access.
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}
