//! Strip chain-of-thought blocks from an LLM completion.
//!
//! 1:1 port of `ai_processor._step.strip_reasoning`. Reasoning models
//! (DeepSeek-R1, Qwen QwQ, GLM-Zero, Kimi k1.5, o1-style, …) emit
//! `<think>…</think>` / `<reasoning>…</reasoning>` style wrappers
//! before the user-visible answer. We strip them so only the
//! final answer makes it to the clipboard.

use once_cell::sync::Lazy;
use regex::Regex;

const THINKING_TAGS: &[&str] = &["think", "thinking", "reasoning", "reflection", "scratchpad"];

/// Compiled regex: paired `<think>…</think>` blocks. `\b` + case
/// insensitive. `\s` is a Rust regex shorthand for whitespace.
static PAIRED_RE: Lazy<Regex> = Lazy::new(|| {
    let alternation = THINKING_TAGS.join("|");
    Regex::new(&format!(
        r"(?is)<(?:{alternation})\b[^>]*>.*?</(?:{alternation})>"
    ))
    .expect("valid paired reasoning tag regex")
});

/// Unclosed opener-to-EOF: drop everything from the opener to the
/// end of the string. Used when a model crashes mid-stream.
static UNCLOSED_RE: Lazy<Regex> = Lazy::new(|| {
    let alternation = THINKING_TAGS.join("|");
    Regex::new(&format!(r"(?is)<(?:{alternation})\b[^>]*>.*\z"))
        .expect("valid unclosed reasoning tag regex")
});

/// Meta-noop detection: a model that returns "the text is already
/// correct" / "no changes needed" is silently falling back. The
/// orchestrator (`step::ai_process_text_with_status`) drops the
/// answer in this case and surfaces `model_returned_meta_response`
/// to the dispatcher.
static META_NOOP_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(текст\s+не\s+содержит|ошиб(ок|ки)\s+нет|изменени[яй]\s+не\s+требу|исправлени[яй]\s+не\s+требу|не\s+требует\s+исправлен|no\s+changes?|nothing\s+to\s+fix)\b",
    )
    .expect("valid meta noop regex")
});

pub fn strip_reasoning(text: &str) -> String {
    if text.is_empty() {
        return text.to_string();
    }
    let mut cleaned = PAIRED_RE.replace_all(text, "").into_owned();
    cleaned = UNCLOSED_RE.replace_all(&cleaned, "").into_owned();
    // Tidy line ends: trim trailing whitespace, collapse 3+
    // consecutive newlines into exactly two (one blank line between
    // paragraphs, matching the Python implementation).
    cleaned = cleaned
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    cleaned = collapse_blank_lines(&cleaned);
    cleaned.trim().to_string()
}

pub fn is_meta_noop_response(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    META_NOOP_RE.is_match(text.trim())
}

fn collapse_blank_lines(text: &str) -> String {
    // Replace any run of 3+ newlines with exactly two newlines
    // (i.e. one blank line between paragraphs). This is the
    // byte-for-byte equivalent of Python's `re.sub(r"\n{3,}", "\n\n", cleaned)`.
    let mut out = String::with_capacity(text.len());
    let mut newline_run = 0_usize;
    for ch in text.chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                out.push('\n');
            }
        } else {
            newline_run = 0;
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_paired_thinking_block() {
        let input = "<think>chain of thought</think>Final answer.";
        let output = strip_reasoning(input);
        assert_eq!(output, "Final answer.");
    }

    #[test]
    fn strips_unclosed_thinking_block() {
        let input = "<thinking>partial answer that crashed";
        let output = strip_reasoning(input);
        assert_eq!(output, "");
    }

    #[test]
    fn leaves_text_without_tags_intact() {
        let input = "Just the answer.";
        assert_eq!(strip_reasoning(input), "Just the answer.");
    }

    #[test]
    fn meta_noop_detection() {
        assert!(is_meta_noop_response("Текст не содержит ошибок."));
        assert!(is_meta_noop_response("no changes needed"));
        assert!(!is_meta_noop_response("Normal answer with content."));
    }

    #[test]
    fn collapse_blank_lines_dedups_runs() {
        let input = "a\n\n\n\n\nb\n\n\nc";
        let output = collapse_blank_lines(input);
        assert_eq!(output, "a\n\nb\n\nc");
    }
}
