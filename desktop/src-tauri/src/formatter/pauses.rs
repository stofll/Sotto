//! Optional cleanup of ellipses used by recognizers to mark pauses.

use super::FormatStep;
use regex::Regex;
use std::{ops::Range, sync::LazyLock};

static ELLIPSIS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[ \t]*(?:\.{3,}|…)+[ \t]*").expect("valid ellipsis pattern"));

pub(super) struct PauseNormalizer {
    pub enabled: bool,
}

impl FormatStep for PauseNormalizer {
    fn name(&self) -> &str {
        "Normalize pause ellipses"
    }

    fn description(&self) -> &str {
        "Replace pause ellipses with spacing or a final period"
    }

    fn permits_empty_output(&self) -> bool {
        true
    }

    fn protected_spans(&self, text: &str) -> Vec<Range<usize>> {
        // Preserve offsets while separating prose such as «можно...добавить»
        // before the shared dotted-name detector sees it as a filename.
        let shadow = ELLIPSIS.replace_all(text, |caps: &regex::Captures| " ".repeat(caps[0].len()));
        let mut spans = crate::text_protection::cleanup_spans(&shadow);
        spans.extend(crate::text_protection::code_spans(text));
        spans.extend(
            crate::text_protection::cleanup_spans(text)
                .into_iter()
                .filter(|span| {
                    let token = &text[span.clone()];
                    token.contains(['/', '\\', '@', '_'])
                        || token.chars().all(|c| {
                            c.is_ascii_digit() || c.is_whitespace() || matches!(c, '.' | ',' | '…')
                        })
                }),
        );
        spans
    }

    fn apply_unprotected(&self, text: &str) -> String {
        if !self.enabled || !ELLIPSIS.is_match(text) {
            return text.to_owned();
        }
        if text
            .chars()
            .all(|c| c.is_whitespace() || matches!(c, '.' | '…'))
        {
            return String::new();
        }
        ELLIPSIS
            .replace_all(text, |caps: &regex::Captures| {
                let matched = caps.get(0).unwrap();
                let before = text[..matched.start()].trim_end();
                let after = &text[matched.end()..];
                let next = after.chars().next();
                if before.is_empty() || next.is_some_and(|c| ".,;:!?»”’\"')]}".contains(c)) {
                    ""
                } else if after.trim().is_empty() || next.is_some_and(|c| matches!(c, '\n' | '\r'))
                {
                    if before.ends_with(['.', '!', '?', ':', ';', ',']) {
                        ""
                    } else {
                        "."
                    }
                } else {
                    " "
                }
            })
            .trim()
            .to_owned()
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}
