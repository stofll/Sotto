use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InstallOptions {
    pub install_dir: String,
    pub desktop_shortcut: bool,
    pub start_menu_shortcut: bool,
}

#[derive(Serialize)]
pub struct Defaults {
    pub options: InstallOptions,
    pub directory_locked: bool,
}

#[cfg(windows)]
pub fn registered_directory() -> Option<PathBuf> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Sotto")
        .ok()?;
    let directory: String = key.get_value("InstallLocation").ok()?;
    parse_directory(directory.trim_matches('"')).ok()
}

#[cfg(not(windows))]
pub fn registered_directory() -> Option<PathBuf> {
    None
}

fn check_candidate_version(installed: &str, candidate: &str) -> Result<(), &'static str> {
    let installed =
        semver::Version::parse(installed).map_err(|_| "installed_version_unavailable")?;
    let candidate =
        semver::Version::parse(candidate).map_err(|_| "installed_version_unavailable")?;
    if candidate < installed {
        Err("installed_version_newer")
    } else {
        Ok(())
    }
}

#[cfg(windows)]
pub fn prevent_downgrade() -> Result<(), &'static str> {
    use std::io::ErrorKind;
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let key = match RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Sotto")
    {
        Ok(key) => key,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("installed_version_unavailable"),
    };
    let installed: String = key
        .get_value("DisplayVersion")
        .map_err(|_| "installed_version_unavailable")?;
    check_candidate_version(&installed, env!("SOTTO_APP_VERSION"))
}

#[cfg(not(windows))]
pub fn prevent_downgrade() -> Result<(), &'static str> {
    Err("unsupported_platform")
}

pub fn default_directory(app: &tauri::AppHandle) -> Result<PathBuf, &'static str> {
    Ok(app
        .path()
        .local_data_dir()
        .map_err(|_| "options_unavailable")?
        .join("Sotto"))
}

pub fn defaults(app: &tauri::AppHandle) -> Result<Defaults, &'static str> {
    let registered = registered_directory();
    let directory = match &registered {
        Some(path) => path.clone(),
        None => default_directory(app)?,
    };
    Ok(Defaults {
        options: InstallOptions {
            install_dir: directory.to_string_lossy().into_owned(),
            desktop_shortcut: true,
            start_menu_shortcut: true,
        },
        directory_locked: registered.is_some(),
    })
}

pub fn parse_directory(input: &str) -> Result<PathBuf, &'static str> {
    // Restrict /D= to a local absolute directory. It is passed unquoted at the
    // end of the NSIS command line, so control characters and quotes are invalid.
    let path = input.replace('/', "\\");
    let bytes = path.as_bytes();
    if bytes.len() < 4
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1..3] != *b":\\"
        || path
            .chars()
            .any(|c| c.is_control() || matches!(c, '"' | '<' | '>' | '|' | '?' | '*'))
        || path[3..].contains(':')
    {
        return Err("invalid_install_directory");
    }
    for part in path[3..].split('\\') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with([' ', '.'])
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err("invalid_install_directory");
        }
    }
    Ok(PathBuf::from(path))
}

fn same_directory(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
        || a.canonicalize()
            .ok()
            .zip(b.canonicalize().ok())
            .is_some_and(|(a, b)| a == b)
}

pub fn validate_directory(
    options: &InstallOptions,
    registered: Option<&Path>,
    default: &Path,
) -> Result<PathBuf, &'static str> {
    let directory = parse_directory(&options.install_dir)?;
    // The default folder is Sotto's own: leftovers of a removed installation
    // must not block a fresh one. Only a custom destination has to be empty.
    if let Some(existing) = registered {
        if !same_directory(&directory, existing) {
            return Err("install_directory_locked");
        }
    } else if !same_directory(&directory, default) && directory.exists() {
        let mut entries = directory
            .read_dir()
            .map_err(|_| "invalid_install_directory")?;
        if entries.next().is_some() {
            return Err("install_directory_not_empty");
        }
    }
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allows_spaces_and_unicode_in_full_paths() {
        assert_eq!(
            parse_directory(r"D:\Мои программы\Sotto").unwrap(),
            PathBuf::from(r"D:\Мои программы\Sotto")
        );
        assert_eq!(
            parse_directory("C:/Apps/Sotto").unwrap(),
            PathBuf::from(r"C:\Apps\Sotto")
        );
    }
    #[test]
    fn rejects_roots_relative_paths_and_command_line_injection() {
        for path in [
            "",
            r"%LOCALAPPDATA%\Sotto",
            r"C:\",
            r"C:Apps\Sotto",
            r"\\server\Sotto",
            r"C:\Apps\..\Sotto",
            "C:\\Apps\\Sotto\" /S",
            "C:\\Apps\n",
            r"C:\Apps\CON",
            r"C:\Apps\Sotto.",
            r"C:\Apps\Sotto:stream",
        ] {
            assert!(parse_directory(path).is_err(), "{path:?}");
        }
    }
    #[test]
    fn does_not_relocate_an_existing_install() {
        let options = InstallOptions {
            install_dir: r"D:\Apps\Sotto".into(),
            desktop_shortcut: true,
            start_menu_shortcut: false,
        };
        assert_eq!(
            validate_directory(
                &options,
                Some(Path::new(r"C:\Apps\Sotto")),
                Path::new(r"D:\Apps\Sotto")
            ),
            Err("install_directory_locked")
        );
    }
    #[test]
    fn refuses_older_installer_and_unreadable_registered_version() {
        assert_eq!(
            check_candidate_version("0.1.4", "0.1.3"),
            Err("installed_version_newer")
        );
        assert_eq!(
            check_candidate_version("0.1.4", "0.1.4-beta.1"),
            Err("installed_version_newer")
        );
        assert_eq!(check_candidate_version("0.1.3", "0.1.3"), Ok(()));
        assert_eq!(check_candidate_version("0.1.3", "0.1.4"), Ok(()));
        assert_eq!(
            check_candidate_version("invalid", "0.1.4"),
            Err("installed_version_unavailable")
        );
    }
    #[cfg(windows)]
    #[test]
    fn fresh_install_requires_empty_custom_directory_but_update_keeps_files() {
        let temporary = tempfile::tempdir().unwrap();
        let options = InstallOptions {
            install_dir: temporary.path().to_string_lossy().into_owned(),
            desktop_shortcut: true,
            start_menu_shortcut: true,
        };
        let default = Path::new(r"C:\Unused\Sotto");
        assert!(validate_directory(&options, None, default).is_ok());
        std::fs::write(temporary.path().join("keep.txt"), b"existing content").unwrap();
        assert_eq!(
            validate_directory(&options, None, default),
            Err("install_directory_not_empty")
        );
        assert!(validate_directory(&options, Some(temporary.path()), default).is_ok());
        // Leftovers in Sotto's own default folder do not block a fresh install.
        assert!(validate_directory(&options, None, temporary.path()).is_ok());
        assert_eq!(
            std::fs::read(temporary.path().join("keep.txt")).unwrap(),
            b"existing content"
        );
    }
}
