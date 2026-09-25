use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

/// Tray icon loaded from `icons/tray.png` (PNG decoded via the `image-png`
/// Tauri feature). Baked into the binary so there is no runtime file lookup.
fn tray_icon_image() -> Image<'static> {
    Image::from_bytes(include_bytes!("../icons/tray.png"))
        .expect("tray.png is a valid PNG embedded at build time")
}

/// Build the platform's tray menu using the current language.
pub(crate) fn tray_menu(
    app: &tauri::AppHandle,
) -> Result<Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    let quit_item = MenuItem::with_id(app, "quit", crate::ui_text::t("Выход"), true, None::<&str>)?;
    #[cfg(windows)]
    let menu = Menu::with_items(app, &[&quit_item])?;
    #[cfg(not(windows))]
    let menu = {
        let open_item = MenuItem::with_id(app, "open", "Sotto", true, None::<&str>)?;
        Menu::with_items(app, &[&open_item, &quit_item])?
    };
    Ok(menu)
}

/// Create the tray at startup, or refresh the native menu after a language change.
pub fn build_tray(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let menu = tray_menu(app)?;
    // Tauri retains each built icon, even when its ID already exists. Reuse
    // the icon and its event handlers instead of registering another copy.
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.set_menu(Some(menu))?;
        return Ok(());
    }

    let builder = TrayIconBuilder::with_id("main-tray")
        .icon(tray_icon_image())
        .tooltip("Sotto")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Err(error) =
                    crate::focus_main_window(tray.app_handle().clone(), "settings".into())
                {
                    log::warn!("open main window from tray: {error}");
                }
            }
        });

    let builder = builder.menu(&menu).on_menu_event(|app, event| {
        #[cfg(not(windows))]
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
