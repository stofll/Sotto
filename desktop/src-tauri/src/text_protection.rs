//! Text that automatic cleanup must preserve byte-for-byte.

use std::ops::Range;

use regex::Regex;
use std::sync::LazyLock;

// Protect ordinary email addresses, excluding surrounding prose punctuation.
static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\p{L}\p{N}_%+\-]+(?:\.[\p{L}\p{N}_%+\-]+)*@[\p{L}\p{N}](?:[\p{L}\p{N}\-]*[\p{L}\p{N}])?(?:\.[\p{L}\p{N}](?:[\p{L}\p{N}\-]*[\p{L}\p{N}])?)*")
        .expect("valid email pattern")
});

/// Recognize backtick code spans/fences and whitespace-delimited technical tokens.
/// An unfinished code span is protected to the end while the user is typing.
pub(crate) fn code_spans(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'`' {
            cursor += 1;
            continue;
        }
        let escaped = bytes[..cursor]
            .iter()
            .rev()
            .take_while(|&&c| c == b'\\')
            .count()
            % 2
            == 1;
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor] == b'`' {
            cursor += 1;
        }
        if escaped {
            continue;
        }
        let width = cursor - start;
        let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
        let indent = &text[line_start..start];
        let fenced = width >= 3 && indent.len() <= 3 && indent.chars().all(|c| c == ' ');
        while cursor < bytes.len() {
            if bytes[cursor] != b'`' {
                cursor += 1;
                continue;
            }
            let close = cursor;
            while cursor < bytes.len() && bytes[cursor] == b'`' {
                cursor += 1;
            }
            let close_width = cursor - close;
            if fenced {
                let line_start = text[..close].rfind('\n').map_or(0, |i| i + 1);
                let indent = &text[line_start..close];
                let rest = text[cursor..].split('\n').next().unwrap_or_default();
                if close_width >= width
                    && indent.len() <= 3
                    && indent.chars().all(|c| c == ' ')
                    && rest.trim().is_empty()
                {
                    break;
                }
            } else if close_width == width {
                break;
            }
        }
        spans.push(start..cursor);
    }

    spans
}

pub(crate) fn technical_spans(text: &str) -> Vec<Range<usize>> {
    let mut spans = code_spans(text);
    spans.extend(EMAIL.find_iter(text).map(|m| m.range()));
    let mut offset = 0;
    for part in text.split_inclusive(char::is_whitespace) {
        if part.contains(['/', '\\']) {
            protect_token(part, offset, &mut spans);
        } else {
            let mut component_offset = offset;
            for component in part.split_inclusive([',', ';', '!', '?', ':']) {
                protect_token(component, component_offset, &mut spans);
                component_offset += component.len();
            }
        }
        offset += part.len();
    }
    spans
}

fn protect_token(part: &str, offset: usize, spans: &mut Vec<Range<usize>>) {
    let token = part.trim_matches(|c: char| c.is_whitespace() || ",;:!?()[]{}<>«»\"'".contains(c));
    let token = token.trim_end_matches('.');
    let path = token.contains(['/', '\\']);
    if token.contains('@') && !path {
        return;
    }
    let dotted = token.split_once('.').is_some_and(|(left, right)| {
        !left.is_empty()
            && !right.is_empty()
            && (token.chars().any(|c| c.is_ascii_alphabetic())
                || right.chars().all(|c| matches!(c, 'а'..='я' | 'ё' | '.')))
    });
    if !token.is_empty() && (path || token.contains('_') || dotted) {
        let start = offset + part.find(token).expect("trimmed substring");
        spans.push(start..start + token.len());
    }
}

/// Mask spans while running an existing whole-text step, preserving its sentence
/// context. Unique, uppercase word tokens survive case/space cleanup. If a step
/// damages a marker, discard that step's output instead of losing protected data.
pub(crate) fn apply(
    text: &str,
    mut spans: Vec<Range<usize>>,
    transform: impl FnOnce(&str) -> String,
) -> String {
    if spans.is_empty() {
        return transform(text);
    }
    spans.sort_unstable_by_key(|span| span.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for span in spans.into_iter().filter(|span| !span.is_empty()) {
        if let Some(last) = merged.last_mut().filter(|last| last.end >= span.start) {
            last.end = last.end.max(span.end);
        } else {
            merged.push(span);
        }
    }
    let mut prefix = "\u{e000}SOTTO_PROTECTED_".to_string();
    while text.contains(&prefix) {
        prefix.push('_');
    }
    let mut masked = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut originals = Vec::with_capacity(merged.len());
    for (i, span) in merged.iter().enumerate() {
        let marker = format!("{prefix}{i}\u{e001}");
        masked.push_str(&text[cursor..span.start]);
        masked.push_str(&marker);
        originals.push((marker, &text[span.clone()]));
        cursor = span.end;
    }
    masked.push_str(&text[cursor..]);
    let mut result = transform(&masked);
    if originals
        .iter()
        .any(|(marker, _)| result.matches(marker).count() != 1)
    {
        return text.to_string();
    }
    for (marker, original) in originals {
        result = result.replacen(&marker, original, 1);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_nested_and_unfinished_code_without_losing_surrounding_context() {
        for text in [
            "до ``текст ` итогуу`` после",
            "до `код без закрытия",
            "```rs\nlet x = `данные`;\n````\nпосле",
        ] {
            let result = apply(text, technical_spans(text), |s| {
                s.replace("итогуу", "итогу")
            });
            assert_eq!(result, text);
        }
        let text = "до ``текст ` итогуу`` после итогуу";
        assert_eq!(
            apply(text, technical_spans(text), |s| s
                .replace("итогуу", "итогу")),
            "до ``текст ` итогуу`` после итогу"
        );
    }

    #[test]
    fn markers_cannot_collide_with_input_or_disappear_silently() {
        let text = "\u{e000}SOTTO_PROTECTED_0\u{e001} `код`";
        assert_eq!(apply(text, technical_spans(text), str::to_string), text);
        assert_eq!(apply(text, technical_spans(text), |_| String::new()), text);
    }
}
