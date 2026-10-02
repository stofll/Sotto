#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod installation_lock;
mod invocation;
mod options;
mod payload;
mod state;

use state::{Phase, Status};
use std::sync::Mutex;
use tauri::{Emitter, Manager};

struct Setup(Mutex<Status>);

fn publish(app: &tauri::AppHandle, phase: Phase, error: Option<&str>) {
    let setup = app.state::<Setup>();
    let mut status = setup.0.lock().unwrap();
    status.set(phase, error);
    let _ = app.emit("setup-status", status.clone());
}

#[tauri::command]
fn setup_status(state: tauri::State<'_, Setup>) -> Status {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
async fn setup_update(app: tauri::AppHandle) -> Result<(), String> {
    let invocation = app.state::<invocation::Invocation>().inner().clone();
    if !invocation.update {
        return Err("invalid_arguments".into());
    }
    {
        let state = app.state::<Setup>();
        let mut status = state.0.lock().unwrap();
        status.begin().map_err(str::to_owned)?;
        let _ = app.emit("setup-status", status.clone());
    }
    let worker_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _installation = installation_lock::InstallationLock::acquire()?;
        options::registered_directory().ok_or("install_failed")?;
        options::prevent_downgrade()?;
        let (_directory, path) = payload::stage()?;
        publish(&worker_app, Phase::Installing, None);
        payload::update(&path, &invocation, true)?;
        installed_binary()?;
        Ok::<_, &'static str>(())
    })
    .await;
    match result {
        Ok(Ok(())) => app.exit(0), // NSIS honors /R and the original /ARGS after replacing files.
        Ok(Err(error)) => publish(&app, Phase::Failed, Some(error)),
        Err(_) => publish(&app, Phase::Failed, Some("install_failed")),
    }
    Ok(())
}

#[tauri::command]
async fn setup_install(
    app: tauri::AppHandle,
    options: options::InstallOptions,
) -> Result<(), String> {
    if app.state::<invocation::Invocation>().update {
        return Err("invalid_arguments".into());
    }
    {
        let state = app.state::<Setup>();
        let mut status = state.0.lock().unwrap();
        status.begin().map_err(str::to_owned)?;
        let _ = app.emit("setup-status", status.clone());
    }
    let worker_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _installation = installation_lock::InstallationLock::acquire()?;
        let default = options::default_directory(&worker_app)?;
        let destination = options::validate_directory(
            &options,
            options::registered_directory().as_deref(),
            &default,
        )?;
        options::prevent_downgrade()?;
        let (_directory, path) = payload::stage()?;
        publish(&worker_app, Phase::Installing, None);
        payload::install(&path, Some(&options))?;
        let binary = installed_binary()?;
        let installed = binary
            .parent()
            .ok_or("install_failed")?
            .canonicalize()
            .map_err(|_| "install_failed")?;
        if installed != destination.canonicalize().map_err(|_| "install_failed")? {
            return Err("install_failed");
        }
        Ok(())
    })
    .await;
    match result {
        Ok(Ok(())) => publish(&app, Phase::Complete, None),
        Ok(Err(error)) => publish(&app, Phase::Failed, Some(error)),
        Err(_) => publish(&app, Phase::Failed, Some("install_failed")),
    }
    Ok(())
}

#[tauri::command]
fn setup_options(app: tauri::AppHandle) -> Result<options::Defaults, String> {
    options::defaults(&app).map_err(str::to_owned)
}

#[tauri::command]
async fn setup_choose_directory(
    app: tauri::AppHandle,
    directory: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    if app.state::<Setup>().0.lock().unwrap().busy() {
        return Err("installation_running".into());
    }
    if options::registered_directory().is_some() {
        return Err("install_directory_locked".into());
    }
    let mut dialog = app.dialog().file();
    if let Ok(path) = options::parse_directory(&directory) {
        if path.is_dir() {
            dialog = dialog.set_directory(path);
        }
    }
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    tauri::async_runtime::spawn_blocking(move || {
        dialog
            .blocking_pick_folder()
            .map(|file| {
                file.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|_| "invalid_install_directory".to_string())
            })
            .transpose()
    })
    .await
    .map_err(|_| "options_unavailable".to_string())?
}

#[cfg(windows)]
fn installed_binary() -> Result<std::path::PathBuf, &'static str> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Sotto")
        .map_err(|_| "install_failed")?;
    let version: String = key
        .get_value("DisplayVersion")
        .map_err(|_| "install_failed")?;
    if version != env!("SOTTO_APP_VERSION") {
        return Err("install_failed");
    }
    let location: String = key
        .get_value("InstallLocation")
        .map_err(|_| "install_failed")?;
    let binary: String = key
        .get_value("MainBinaryName")
        .map_err(|_| "install_failed")?;
    if binary.contains(['/', '\\']) || binary == "." || binary == ".." {
        return Err("install_failed");
    }
    let path = std::path::PathBuf::from(location.trim_matches('"')).join(binary);
    if !path.is_absolute() || !path.is_file() {
        return Err("install_failed");
    }
    Ok(path)
}

#[cfg(not(windows))]
fn installed_binary() -> Result<std::path::PathBuf, &'static str> {
    Err("unsupported_platform")
}

#[tauri::command]
fn setup_launch(app: tauri::AppHandle, state: tauri::State<'_, Setup>) -> Result<(), String> {
    let status = state.0.lock().unwrap();
    if status.preview || status.phase != Phase::Complete {
        return Err("not_installed".into());
    }
    let path = installed_binary().map_err(str::to_owned)?;
    std::process::Command::new(&path)
        .current_dir(path.parent().unwrap())
        .spawn()
        .map_err(|_| "launch_failed".to_string())?;
    drop(status);
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn setup_close(app: tauri::AppHandle, state: tauri::State<'_, Setup>) -> Result<(), String> {
    if state.0.lock().unwrap().busy() {
        return Err("installation_running".into());
    }
    app.exit(0);
    Ok(())
}

#[cfg(windows)]
fn native_message(russian: &str, english: &str) {
    use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
    // Without WebView2 the frontend translations are unavailable; 0x19 is LANG_RUSSIAN.
    let message = if unsafe { GetUserDefaultUILanguage() } & 0x3ff == 0x19 {
        russian
    } else {
        english
    };
    let title: Vec<u16> = "Sotto Setup\0".encode_utf16().collect();
    let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(
            0,
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

fn main() {
    let invocation = invocation::Invocation::current().unwrap_or_else(|_| std::process::exit(2));
    let preview = payload::BYTES.is_empty() || invocation.preview;
    #[cfg(windows)]
    if !preview && tauri::webview_version().is_err() {
        match installation_lock::InstallationLock::acquire().and_then(|_installation| {
            native_message(
                "WebView2 недоступен. Sotto откроет стандартный установщик, чтобы установить нужные компоненты.",
                "WebView2 is unavailable. Sotto will open the standard installer to install the required components.",
            );
            let (_directory, path) = payload::stage()?;
            if invocation.update {
                options::prevent_downgrade()?;
                payload::update(&path, &invocation, false)
            } else {
                payload::install(&path, None)
            }
        }) {
            Ok(()) => std::process::exit(0),
            Err("installation_running") => {
                native_message(
                    "Другая установка Sotto уже выполняется. Дождитесь её завершения и откройте этот установщик снова.",
                    "Another Sotto installation is running. Wait for it to finish and open this installer again.",
                );
                std::process::exit(1);
            }
            Err(_) => std::process::exit(1),
        }
    }
    let mut status = Status::new(preview);
    status.update = invocation.update;
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Setup(Mutex::new(status)))
        .manage(invocation)
        .invoke_handler(tauri::generate_handler![
            setup_status,
            setup_options,
            setup_choose_directory,
            setup_install,
            setup_update,
            setup_launch,
            setup_close,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.state::<Setup>().0.lock().unwrap().busy() {
                    api.prevent_close();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("Sotto setup could not start");
}
