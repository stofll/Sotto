//! Opening links outside the app: web pages in the system browser, and on
//! macOS the Privacy & Security panes the permission banners point at.

/// The macOS System Settings scheme, and the one pane family the app opens.
const MAC_SETTINGS_SCHEME: &str = "x-apple.systempreferences";
const MAC_PRIVACY_PANE: &str = "com.apple.preference.security";

/// The link to hand to the system, or why it is refused.
///
/// Checked here rather than trusted from the webview: `ShellExecuteW` and
/// `open` start whatever they are given, so a `file:` URL or a bare path would
/// launch a program. Only web pages and, on macOS, the privacy panes pass;
/// the result is the parsed URL re-serialized, never the caller's string.
fn allowed_link(raw: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "Invalid URL".to_string())?;
    let allowed = match url.scheme() {
        "https" | "http" => url.host_str().is_some_and(|host| !host.is_empty()),
        MAC_SETTINGS_SCHEME => {
            cfg!(target_os = "macos") && url.path().starts_with(MAC_PRIVACY_PANE)
        }
        _ => false,
    };
    if allowed {
        Ok(url.into())
    } else {
        Err("Unsupported link".to_string())
    }
}

#[tauri::command]
pub(crate) fn open_url(url: String) -> Result<(), String> {
    let url = allowed_link(&url)?;
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("open failed: {e}"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("xdg-open failed: {e}"))?;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        // A serialized URL never contains a raw NUL: control characters are
        // percent-encoded or rejected by the parser.
        let url: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
        // Launch directly: cmd.exe interprets query-string ampersands as commands.
        //
        // SAFETY: `url` is NUL-terminated above and outlives the call; every
        // other pointer argument is the null the API accepts for "unused".
        let result = unsafe {
            ShellExecuteW(
                0,
                std::ptr::null(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if result as isize <= 32 {
            return Err("Could not open URL".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::allowed_link;

    #[test]
    fn web_pages_pass_normalized() {
        assert_eq!(
            allowed_link("https://github.com/stofll/Sotto/issues/new?labels=bug&body=a b").unwrap(),
            "https://github.com/stofll/Sotto/issues/new?labels=bug&body=a%20b"
        );
        assert_eq!(
            allowed_link("HTTP://Example.com").unwrap(),
            "http://example.com/"
        );
    }

    #[test]
    fn anything_that_could_start_a_program_is_refused() {
        for link in [
            "file:///C:/Windows/System32/calc.exe",
            "C:\\Windows\\System32\\calc.exe",
            "\\\\server\\share\\tool.exe",
            "/Applications/Calculator.app",
            "calc.exe",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "ms-settings:privacy",
            "smb://server/share",
            "https://",
            "",
        ] {
            assert!(allowed_link(link).is_err(), "{link:?} was allowed");
        }
    }

    #[test]
    fn privacy_panes_open_only_on_macos() {
        let pane = "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
        assert_eq!(allowed_link(pane).is_ok(), cfg!(target_os = "macos"));
        assert!(allowed_link("x-apple.systempreferences:com.apple.Terminal").is_err());
    }
}
