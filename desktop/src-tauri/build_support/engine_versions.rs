/// Read the exact version from Cargo's generated package blocks. Multiple
/// versions are rejected because they do not identify one speech engine.
pub fn locked_version<'a>(lockfile: &'a str, package: &str) -> Option<&'a str> {
    let mut versions = lockfile.split("[[package]]").filter_map(|block| {
        let field = |name: &str| {
            block.lines().find_map(|line| {
                let (key, value) = line.trim().split_once('=')?;
                if key.trim() != name {
                    return None;
                }
                value.trim().strip_prefix('"')?.strip_suffix('"')
            })
        };
        if field("name") != Some(package) {
            return None;
        }
        field("version")
    });
    let version = versions.next()?;
    versions.next().is_none().then_some(version)
}

pub fn versions<'a>(lockfile: &'a str, runtime_lock: &str) -> Result<(&'a str, &'a str), String> {
    let whisper =
        locked_version(lockfile, "whisper-rs").ok_or("Expected one locked Whisper version")?;
    let sherpa =
        locked_version(lockfile, "sherpa-onnx").ok_or("Expected one locked Sherpa version")?;
    let mut native = runtime_lock
        .lines()
        .filter_map(|line| line.trim().strip_prefix("version "));
    if native.next() != Some(sherpa) || native.next().is_some() {
        return Err("Sherpa crate and native runtime versions must agree".into());
    }
    Ok((whisper, sherpa))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = "version = 4\n[[package]]\nname = \"whisper-rs\"\nversion = \"0.14.4\"\n[[package]]\nversion = \"1.13.8\"\nname = \"sherpa-onnx\"\n";

    #[test]
    fn exact_package_versions_ignore_header_and_field_order() {
        assert_eq!(
            versions(LOCK, "# pinned\nversion 1.13.8\narchive hash\n").unwrap(),
            ("0.14.4", "1.13.8")
        );
        assert_eq!(locked_version(LOCK, "whisper"), None);
        assert_eq!(
            locked_version("[[package]]\nname = \"whisper-rs\"", "whisper-rs"),
            None
        );
    }

    #[test]
    fn ambiguous_or_mismatched_versions_fail_closed() {
        let duplicate = format!("{LOCK}[[package]]\nname = \"whisper-rs\"\nversion = \"0.15.0\"\n");
        assert!(versions(&duplicate, "version 1.13.8").is_err());
        for native in ["", "version 1.13.7", "version 1.13.8\nversion 1.13.8"] {
            assert!(versions(LOCK, native).is_err());
        }
    }
}
