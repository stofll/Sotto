//! The name of the application that will receive the dictated text, for the
//! overlay's `target` element.
//!
//! It is read only when an overlay that shows the element asks, as the
//! recording starts: on Windows from the paste target the pipeline already
//! captures (`clipboard::capture_target`), on macOS from the frontmost
//! application, which is also where the text will be pasted. Nothing is kept
//! here, logged, stored or sent anywhere but to that overlay (docs/privacy.md).

/// The application the next text will go to, or `None` when the system gave no name.
#[tauri::command]
pub fn paste_target_name() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        frontmost_application_name()
    }
    #[cfg(windows)]
    {
        captured_application_name()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn frontmost_application_name() -> Option<String> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;

    let workspace_class = AnyClass::get(c"NSWorkspace")?;
    // SAFETY: each selector is sent to the object NSWorkspace documents it
    // for, with no arguments, and every returned pointer is checked for nil
    // before it is used. `localizedName` is an NSString, which is converted
    // to an owned String before the autoreleased object can go away.
    unsafe {
        let workspace: *mut AnyObject = msg_send![workspace_class, sharedWorkspace];
        if workspace.is_null() {
            return None;
        }
        let application: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if application.is_null() {
            return None;
        }
        let name: *mut NSString = msg_send![application, localizedName];
        let name = name.as_ref()?.to_string();
        let name = name.trim();
        (!name.is_empty()).then(|| name.to_string())
    }
}

#[cfg(windows)]
fn captured_application_name() -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let hwnd = crate::windows_util::get_captured_hwnd()?;
    let mut process_id = 0u32;
    // SAFETY: `process_id` is a live u32 the callee writes to; a stale HWND
    // yields a zero id, never a memory fault. The process handle is closed on
    // every path and the buffer length passed matches the buffer.
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut process_id);
        if process_id == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if process == 0 {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let found = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length);
        CloseHandle(process);
        if found == 0 {
            return None;
        }
        application_name(&String::from_utf16_lossy(&buffer[..length as usize]))
    }
}

/// A display name from an executable path: the file name without `.exe`,
/// with a lower-case name capitalised (`chrome` → `Chrome`). Windows shows
/// no friendlier name without reading version resources, and a packaged
/// app's window belongs to a host process that says nothing about the app.
#[cfg(any(windows, test))]
fn application_name(path: &str) -> Option<String> {
    let file = path.rsplit(['\\', '/']).next()?;
    let stem = match file.len().checked_sub(4) {
        Some(at) if file.is_char_boundary(at) && file[at..].eq_ignore_ascii_case(".exe") => {
            &file[..at]
        }
        _ => file,
    };
    let stem = stem.trim();
    if stem.is_empty() || stem.eq_ignore_ascii_case("ApplicationFrameHost") {
        return None;
    }
    let mut chars = stem.chars();
    let first = chars.next()?;
    Some(if stem.chars().all(|c| !c.is_uppercase()) {
        first.to_uppercase().chain(chars).collect()
    } else {
        stem.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_an_application_by_its_executable() {
        assert_eq!(
            application_name(r"C:\Program Files\Google\Chrome\Application\chrome.exe").as_deref(),
            Some("Chrome")
        );
        assert_eq!(
            application_name(r"C:\Windows\notepad.EXE").as_deref(),
            Some("Notepad")
        );
        assert_eq!(
            application_name(r"C:\Apps\Telegram.exe").as_deref(),
            Some("Telegram")
        );
        assert_eq!(application_name("/opt/code").as_deref(), Some("Code"));
        assert_eq!(
            application_name(r"C:\Apps\WINWORD.EXE").as_deref(),
            Some("WINWORD")
        );
    }

    #[test]
    fn says_nothing_when_the_path_names_no_application() {
        assert_eq!(application_name(""), None);
        assert_eq!(application_name(r"C:\Apps\.exe"), None);
        assert_eq!(
            application_name(r"C:\Windows\System32\ApplicationFrameHost.exe"),
            None
        );
    }

    #[test]
    fn keeps_a_non_latin_name_whole() {
        assert_eq!(
            application_name(r"C:\Apps\блокнот.exe").as_deref(),
            Some("Блокнот")
        );
    }
}
