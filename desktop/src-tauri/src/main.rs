#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Run by the uninstaller when the user chooses to delete the app data.
    #[cfg(windows)]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--purge-user-data")
    {
        std::process::exit(sotto_lib::purge_user_data());
    }
    sotto_lib::run();
}
