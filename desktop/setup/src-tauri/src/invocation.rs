//! Accept the existing Tauri updater's NSIS command line, including its escaped restart arguments.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Invocation {
    pub update: bool,
    pub preview: bool,
    restart: bool,
    arguments: String,
}

impl Invocation {
    pub fn parse(command_line: &str) -> Result<Self, &'static str> {
        let command_line = command_line.trim_start();
        let mut remaining = if let Some(quoted) = command_line.strip_prefix('"') {
            quoted.split_once('"').ok_or("invalid_arguments")?.1
        } else {
            command_line
                .split_once(char::is_whitespace)
                .map_or("", |(_, tail)| tail)
        };
        let mut invocation = Self::default();
        loop {
            remaining = remaining.trim_start();
            if remaining.is_empty() {
                break;
            }
            let (flag, tail) = remaining
                .split_once(char::is_whitespace)
                .unwrap_or((remaining, ""));
            match flag {
                "/UPDATE" => invocation.update = true,
                "/R" => invocation.restart = true,
                "/P" | "/S" => {}
                "--preview" => invocation.preview = true,
                "/ARGS" => {
                    // Do not parse/requote this tail: Tauri escapes it for NSIS, not CommandLineToArgvW.
                    invocation.arguments = tail.trim_start().to_owned();
                    break;
                }
                _ => return Err("invalid_arguments"),
            }
            remaining = tail;
        }
        Ok(invocation)
    }

    pub fn nsis_arguments(&self, silent: bool) -> String {
        format!(
            "{} {} /UPDATE /ARGS {}",
            if silent { "/S" } else { "/P" },
            if self.restart { "/R" } else { "" },
            self.arguments
        )
    }

    #[cfg(windows)]
    pub fn current() -> Result<Self, &'static str> {
        use windows_sys::Win32::System::Environment::GetCommandLineW;
        // Windows owns this null-terminated buffer for the lifetime of the process.
        let command_line = unsafe {
            let pointer = GetCommandLineW();
            let mut length = 0;
            while *pointer.add(length) != 0 {
                length += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length))
        };
        Self::parse(&command_line)
    }

    #[cfg(not(windows))]
    pub fn current() -> Result<Self, &'static str> {
        Self::parse(if std::env::args().any(|arg| arg == "--preview") {
            "setup --preview"
        } else {
            "setup"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_updaters_keep_restart_arguments_and_replace_passive_with_silent() {
        let tail = r#""some path" "/flag" "цитата \"text\"""#;
        let invocation = Invocation::parse(&format!(
            r#""C:\Мои файлы\setup.exe" /P /R /UPDATE /ARGS {tail}"#
        ))
        .unwrap();
        assert!(invocation.update);
        assert!(!invocation.preview);
        assert_eq!(
            invocation.nsis_arguments(true),
            format!("/S /R /UPDATE /ARGS {tail}")
        );
        assert_eq!(
            invocation.nsis_arguments(false),
            format!("/P /R /UPDATE /ARGS {tail}")
        );
    }

    #[test]
    fn application_arguments_cannot_change_setup_mode() {
        let invocation = Invocation::parse("setup.exe /UPDATE /ARGS --preview /R").unwrap();
        assert!(!invocation.preview);
        assert!(!invocation.restart);
        assert!(Invocation::parse("setup.exe /unexpected /UPDATE").is_err());
        assert!(Invocation::parse("\"unfinished.exe").is_err());
        assert_eq!(
            Invocation::parse("setup.exe").unwrap(),
            Invocation::default()
        );
        assert!(
            Invocation::parse("setup.exe --preview /UPDATE")
                .unwrap()
                .preview
        );
    }
}
