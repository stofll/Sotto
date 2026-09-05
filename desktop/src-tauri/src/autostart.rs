use std::io::{self, Write};
use std::path::Path;

/// The plugin's LaunchAgent name and arguments, with an atomic file replacement.
pub fn write_launch_agent(path: &Path, name: &str, exe: &Path, arg: &str) -> io::Result<()> {
    let exe = exe.canonicalize()?;
    let exe = exe.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "executable path is not UTF-8")
    })?;
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict>\n\
         <key>Label</key><string>{}</string>\n\
         <key>ProgramArguments</key><array><string>{}</string><string>{}</string></array>\n\
         <key>RunAtLoad</key><true/>\n\
         </dict></plist>\n",
        escape_xml(name), escape_xml(exe), escape_xml(arg)
    );
    let dir = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "LaunchAgent path has no parent",
        )
    })?;
    std::fs::create_dir_all(dir)?;
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    file.write_all(xml.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_replaces_the_path_and_escapes_xml() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Sotto & test");
        std::fs::write(&exe, b"binary").unwrap();
        let path = dir.path().join("LaunchAgents/Sotto.plist");
        write_launch_agent(&path, "Sotto", &exe, "--autostart").unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("Sotto &amp; test"));
        let next = dir.path().join("Sotto new");
        std::fs::write(&next, b"binary").unwrap();
        write_launch_agent(&path, "Sotto <new>", &next, "--autostart").unwrap();
        let xml = std::fs::read_to_string(path).unwrap();
        assert!(xml.contains("Sotto &lt;new&gt;</string>"));
        assert!(!xml.contains("Sotto &amp; test"));
        assert!(xml.contains("<string>--autostart</string>"));
    }

    #[test]
    fn failed_refresh_preserves_the_existing_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Sotto.plist");
        std::fs::write(&path, b"existing entry").unwrap();
        assert!(
            write_launch_agent(&path, "Sotto", &dir.path().join("missing"), "--autostart").is_err()
        );
        assert_eq!(std::fs::read(path).unwrap(), b"existing entry");
    }
}
