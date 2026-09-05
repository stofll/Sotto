use std::path::{Path, PathBuf};

fn data_dir_at(executable: &Path) -> Option<PathBuf> {
    let parent = executable.parent()?;
    parent
        .join("portable.flag")
        .is_file()
        .then(|| parent.join("data"))
}

/// The marker travels with the application; no machine-specific absolute paths.
pub fn data_dir() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    std::env::current_exe().ok().and_then(|p| data_dir_at(&p))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_marker_selects_adjacent_data_directory() {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("Sotto.exe");
        assert_eq!(data_dir_at(&executable), None);
        std::fs::write(dir.path().join("portable.flag"), "").unwrap();
        assert_eq!(data_dir_at(&executable), Some(dir.path().join("data")));
    }
}
