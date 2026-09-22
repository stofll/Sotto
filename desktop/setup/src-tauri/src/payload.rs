use sha2::{Digest, Sha256};
use std::{fs::OpenOptions, io::Write, path::Path, process::Command};

pub const BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.exe"));
const HASH: &str = env!("SOTTO_PAYLOAD_SHA256");

pub fn extract(bytes: &[u8], expected_hash: &str, destination: &Path) -> Result<(), &'static str> {
    if bytes.is_empty() {
        return Err("payload_missing");
    }
    if !bytes.starts_with(b"MZ") || format!("{:x}", Sha256::digest(bytes)) != expected_hash {
        return Err("payload_invalid");
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| "prepare_failed")?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "prepare_failed")
}

pub fn stage() -> Result<(tempfile::TempDir, std::path::PathBuf), &'static str> {
    let directory = tempfile::Builder::new()
        .prefix("sotto-setup-")
        .tempdir()
        .map_err(|_| "prepare_failed")?;
    let path = directory.path().join("Sotto-install.exe");
    extract(BYTES, HASH, &path)?;
    Ok((directory, path))
}

pub fn install(
    path: &Path,
    options: Option<&crate::options::InstallOptions>,
) -> Result<(), &'static str> {
    let mut command = Command::new(path);
    if let Some(options) = options {
        let directory = crate::options::parse_directory(&options.install_dir)?;
        command.arg("/S");
        command.arg(format!(
            "/SOTTODESKTOP={}",
            u8::from(options.desktop_shortcut)
        ));
        command.arg(format!(
            "/SOTTOSTARTMENU={}",
            u8::from(options.start_menu_shortcut)
        ));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // NSIS requires /D= last and unquoted, including paths with spaces.
            command.raw_arg(format!("/D={}", directory.display()));
        }
        #[cfg(not(windows))]
        {
            let _ = directory;
            return Err("unsupported_platform");
        }
    }
    // Keep the NSIS process attached until completion so its payload is not
    // removed early. No forced cancellation during file replacement.
    let status = command.status().map_err(|_| "install_failed")?;
    if status.success() {
        Ok(())
    } else {
        Err("install_failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    #[ignore = "run scripts/test-setup-options.py to compile the isolated NSIS fixture"]
    fn native_nsis_options() {
        let fixture =
            std::path::PathBuf::from(std::env::var_os("SOTTO_TEST_NSIS_FIXTURE").unwrap());
        assert_eq!(fixture.file_name().unwrap(), "setup-options-fixture.exe");
        let temporary = tempfile::tempdir().unwrap();
        for (desktop, start_menu) in [(false, false), (true, false), (false, true), (true, true)] {
            let destination = temporary
                .path()
                .join(format!("Мои программы {desktop} {start_menu}"));
            let mut options = crate::options::InstallOptions {
                install_dir: destination.to_string_lossy().into_owned(),
                desktop_shortcut: desktop,
                start_menu_shortcut: start_menu,
            };
            install(&fixture, Some(&options)).unwrap();
            assert!(destination.join("Sotto-fixture.exe").is_file());
            let desktop_link = destination.join("desktop/Sotto-fixture.lnk");
            let menu_link = destination.join("startmenu/Sotto-fixture.lnk");
            assert_eq!(desktop_link.exists(), desktop);
            assert_eq!(menu_link.exists(), start_menu);
            options.desktop_shortcut = false;
            options.start_menu_shortcut = false;
            install(&fixture, Some(&options)).unwrap();
            assert!(!desktop_link.exists());
            assert!(!menu_link.exists());
            std::fs::write(destination.join("foreign"), b"fixture").unwrap();
            install(&fixture, Some(&options)).unwrap();
            assert!(desktop_link.exists(), "preserve unrelated shortcut");
            assert!(menu_link.exists(), "preserve unrelated shortcut");
        }
        use std::os::windows::process::CommandExt;
        let default_dir = temporary.path().join("default choices");
        assert!(Command::new(&fixture)
            .arg("/S")
            .raw_arg(format!("/D={}", default_dir.display()))
            .status()
            .unwrap()
            .success());
        assert!(default_dir.join("desktop/Sotto-fixture.lnk").exists());
        assert!(default_dir.join("startmenu/Sotto-fixture.lnk").exists());
        let invalid_dir = temporary.path().join("invalid choices");
        assert!(!Command::new(&fixture)
            .args(["/S", "/SOTTODESKTOP=invalid"])
            .raw_arg(format!("/D={}", invalid_dir.display()))
            .status()
            .unwrap()
            .success());
        assert!(!invalid_dir.exists());
    }
    #[test]
    fn rejects_corruption_before_writing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("payload.exe");
        assert_eq!(
            extract(b"MZ-corrupt", "wrong", &path),
            Err("payload_invalid")
        );
        assert!(!path.exists());
    }
    #[test]
    fn never_overwrites_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("payload.exe");
        let bytes = b"MZ-synthetic-test-fixture";
        let hash = format!("{:x}", Sha256::digest(bytes));
        extract(bytes, &hash, &path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(extract(bytes, &hash, &path), Err("prepare_failed"));
    }
    #[test]
    fn empty_preview_has_no_installable_payload() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            extract(b"", "", &directory.path().join("payload.exe")),
            Err("payload_missing")
        );
    }
}
