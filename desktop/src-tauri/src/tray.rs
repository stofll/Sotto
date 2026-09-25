#[cfg(not(windows))]
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

/// Create the tray at startup, or refresh the native menu after a language change.
pub fn build_tray(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(not(windows))]
    let menu = {
        let quit_item =
            MenuItem::with_id(app, "quit", crate::ui_text::t("Выход"), true, None::<&str>)?;
        let open_item = MenuItem::with_id(app, "open", "Sotto", true, None::<&str>)?;
        Menu::with_items(app, &[&open_item, &quit_item])?
    };

    // Tauri retains each built icon, even when its ID already exists. Reuse
    // the icon and its event handlers instead of registering another copy.
    #[cfg(windows)]
    if app.tray_by_id("main-tray").is_some() {
        return Ok(());
    }
    #[cfg(not(windows))]
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.set_menu(Some(menu))?;
        return Ok(());
    }

    let builder = TrayIconBuilder::with_id("main-tray")
        .icon(tray_icon_image())
        .tooltip("Sotto")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            if let TrayIconEvent::Click {
                button,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                #[cfg(windows)]
                {
                    let app = app.clone();
                    // WebView2 creation must not block the Windows event callback.
                    std::thread::spawn(move || match button {
                        MouseButton::Left => {
                            if let Err(error) = hide_tray_popup(app.clone()) {
                                log::warn!("hide tray popup: {error}");
                            }
                            if let Err(error) = crate::focus_main_window(app, "settings".into()) {
                                log::warn!("open main window from tray: {error}");
                            }
                        }
                        MouseButton::Right => {
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
                            }
                        }
                        _ => {}
                    });
                }
                #[cfg(not(windows))]
                {
                    if button == MouseButton::Left {
                        let _ = crate::focus_main_window(app.clone(), "settings".into());
                    }
                }
            }
        });

    #[cfg(not(windows))]
    let builder = builder.menu(&menu).on_menu_event(|app, event| {
        if event.id.as_ref() == "open" {
            let _ = crate::focus_main_window(app.clone(), "settings".into());
        }
        if event.id.as_ref() == "quit" {
            app.exit(0);
        }
    });

    builder.build(app)?;

    Ok(())
}

#[cfg(windows)]
#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}
