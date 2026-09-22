use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Ready,
    Preparing,
    Installing,
    Complete,
    Failed,
}

#[derive(Clone, Serialize)]
pub struct Status {
    pub phase: Phase,
    pub revision: u64,
    pub version: &'static str,
    pub preview: bool,
    pub error: Option<String>,
}

impl Status {
    pub fn new(preview: bool) -> Self {
        Self {
            phase: Phase::Ready,
            revision: 0,
            version: env!("SOTTO_APP_VERSION"),
            preview,
            error: None,
        }
    }
    pub fn busy(&self) -> bool {
        matches!(self.phase, Phase::Preparing | Phase::Installing)
    }
    pub fn begin(&mut self) -> Result<(), &'static str> {
        if self.preview {
            return Err("preview_only");
        }
        if self.busy() || self.phase == Phase::Complete {
            return Err("already_started");
        }
        self.set(Phase::Preparing, None);
        Ok(())
    }
    pub fn set(&mut self, phase: Phase, error: Option<&str>) {
        self.phase = phase;
        self.error = error.map(str::to_owned);
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_duplicate_install_and_allows_retry_after_failure() {
        let mut state = Status::new(false);
        state.begin().unwrap();
        assert!(state.busy());
        assert_eq!(state.begin(), Err("already_started"));
        state.set(Phase::Failed, Some("install_failed"));
        assert!(!state.busy());
        state.begin().unwrap();
        assert_eq!(state.error, None);
        assert_eq!(state.revision, 3);
        state.set(Phase::Complete, None);
        assert_eq!(state.begin(), Err("already_started"));
    }
    #[test]
    fn preview_cannot_install() {
        let mut state = Status::new(true);
        assert_eq!(state.begin(), Err("preview_only"));
        assert_eq!(state.phase, Phase::Ready);
    }
}
