//! Opt-in native title-bar regression. Opens only an isolated blank window.
//! Run: cargo run --locked --example macos_window_layout

#[cfg(target_os = "macos")]
fn check_buttons(window: &tauri::WebviewWindow) -> Result<(), String> {
    use objc2::{msg_send, runtime::AnyObject};
    use objc2_foundation::NSRect;

    let ns = window.ns_window().map_err(|error| error.to_string())?;
    // SAFETY: called on the event-loop thread with a live window; AppKit owns
    // the content view and standard buttons throughout these measurements.
    unsafe {
        let ns = ns.cast::<AnyObject>();
        let content: *mut AnyObject = msg_send![ns, contentView];
        let bounds: NSRect = msg_send![content, bounds];
        let flipped: bool = msg_send![content, isFlipped];
        let mut previous_right = 0.0;
        for kind in 0..3usize {
            let button: *mut AnyObject = msg_send![ns, standardWindowButton: kind];
            if button.is_null() {
                return Err(format!("missing native window button {kind}"));
            }
            let button_bounds: NSRect = msg_send![button, bounds];
            let rect: NSRect = msg_send![button, convertRect: button_bounds, toView: content];
            let middle = rect.origin.y + rect.size.height / 2.0;
            let centre_y = if flipped {
                middle
            } else {
                bounds.size.height - middle
            };
            let right = rect.origin.x + rect.size.width;
            println!("button {kind}: x={}, centre_y={centre_y}", rect.origin.x);
            if centre_y - rect.size.height / 2.0 < 0.0
                || centre_y + rect.size.height / 2.0 > 32.0
                || rect.size.width <= 0.0
                || rect.size.height <= 0.0
                || rect.origin.x < previous_right
                || right > bounds.size.width
            {
                return Err(format!(
                    "button {kind} does not fit the native title-bar row"
                ));
            }
            previous_right = right;
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn main() {
    use tauri::{LogicalSize, Manager, RunEvent, WebviewUrl};

    // No production setup or plugins: no configuration, history, recordings,
    // models, hotkeys or microphone are opened.
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = "com.sotto.window-layout-check".into();
    for window in &mut context.config_mut().app.windows {
        window.url = WebviewUrl::External("about:blank".parse().unwrap());
        window.title = "Sotto window layout check".into();
    }
    let app = tauri::Builder::default().build(context).unwrap();
    let mut resized = false;
    let started = std::time::Instant::now();
    let result = std::rc::Rc::new(std::cell::Cell::new(None));
    let outcome = result.clone();
    app.run_return(move |handle, event| {
        if outcome.get().is_some() {
            return;
        }
        if !matches!(event, RunEvent::MainEventsCleared) {
            return;
        }
        let window = handle.get_webview_window("main").unwrap();
        if started.elapsed() > std::time::Duration::from_secs(10) {
            eprintln!("native window did not finish resizing");
            outcome.set(Some(1));
            handle.exit(1);
            return;
        }
        if resized
            && window
                .inner_size()
                .unwrap()
                .to_logical::<f64>(window.scale_factor().unwrap())
                .width
                != 1200.0
        {
            return;
        }
        if let Err(error) = check_buttons(&window) {
            eprintln!("{error}");
            outcome.set(Some(1));
            handle.exit(1);
        } else if resized {
            outcome.set(Some(0));
            handle.exit(0);
        } else {
            window.set_size(LogicalSize::new(1200.0, 900.0)).unwrap();
            resized = true;
        }
    });
    std::process::exit(result.get().unwrap_or(1));
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This check requires a macOS desktop session.");
    std::process::exit(1);
}
