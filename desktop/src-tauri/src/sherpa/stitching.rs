//! Recover only boundary punctuation and initial case from a short audio recheck.

use std::sync::LazyLock;

use regex::Regex;

static WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\p{L}\p{N}]+(?:-[\p{L}\p{N}]+)*").expect("valid boundary word pattern")
});

pub(super) fn needs_context(left: &str, right: &str) -> bool {
    let Some(first) = WORD.find(right).filter(|word| word.start() == 0) else {
        return false;
    };
    let mut letters = first.as_str().chars();
    if !letters.next().is_some_and(|c| matches!(c, 'А'..='Я' | 'Ё'))
        || !letters.all(|c| c.is_lowercase() || c == '-')
        || right[first.end()..].starts_with('.')
        || !left.chars().next_back().is_some_and(char::is_alphabetic)
    {
        return false;
    }
    // An unfinished code span and a trailing technical token are not prose.
    if crate::text_protection::technical_spans(left)
        .iter()
        .any(|span| span.end == left.len())
        || crate::text_protection::technical_spans(right)
            .iter()
            .any(|span| span.start == 0)
    {
        return false;
    }
    WORD.find_iter(left).take(2).count() == 2 && WORD.find_iter(right).take(2).count() == 2
}

pub(super) struct Boundary<'a> {
    pub punctuation: &'a str,
    pub initial: char,
}

pub(super) fn recover<'a>(left: &str, right: &str, context: &'a str) -> Option<Boundary<'a>> {
    if !needs_context(left, right) {
        return None;
    }
    let left_words: Vec<_> = WORD.find_iter(left).collect();
    let right_words: Vec<_> = WORD.find_iter(right).take(2).collect();
    let anchors: Vec<_> = left_words[left_words.len() - 2..]
        .iter()
        .chain(&right_words)
        .map(|word| word.as_str().to_lowercase())
        .collect();
    let words: Vec<_> = WORD.find_iter(context).collect();
    let mut matches = words.windows(4).filter(|window| {
        window
            .iter()
            .zip(&anchors)
            .all(|(word, anchor)| word.as_str().to_lowercase() == *anchor)
    });
    let matched = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    // The anchors must keep their own punctuation, including a deliberate dash.
    let last = left_words.len() - 1;
    if context[matched[0].end()..matched[1].start()].trim()
        != left[left_words[last - 1].end()..left_words[last].start()].trim()
        || context[matched[2].end()..matched[3].start()].trim()
            != right[right_words[0].end()..right_words[1].start()].trim()
    {
        return None;
    }
    let punctuation = context[matched[1].end()..matched[2].start()].trim();
    if !matches!(punctuation, "" | "," | "." | "!" | "?" | ";" | ":") {
        return None;
    }
    let initial = matched[2].as_str().chars().next()?;
    // A recheck is evidence for one initial, never permission to respell a word.
    let original = right_words[0].as_str();
    if original
        .chars()
        .skip(1)
        .ne(matched[2].as_str().chars().skip(1))
    {
        return None;
    }
    Some(Boundary {
        punctuation,
        initial,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(left: &str, right: &str, context: &str) -> String {
        match recover(left, right, context) {
            Some(boundary) => format!(
                "{left}{} {}{}",
                boundary.punctuation,
                boundary.initial,
                &right[right.chars().next().unwrap().len_utf8()..]
            ),
            None => format!("{left} {right}"),
        }
    }

    #[test]
    fn repairs_only_the_seam_with_matching_words_on_both_sides() {
        assert_eq!(
            joined(
                "Остались новые",
                "Записи для проверки.",
                "Остались новые записи для проверки"
            ),
            "Остались новые записи для проверки."
        );
        assert_eq!(
            joined(
                "Работаем с планшета",
                "Теперь проверим результат.",
                "Работаем с планшета. Теперь проверим результат"
            ),
            "Работаем с планшета. Теперь проверим результат."
        );
        assert_eq!(
            joined(
                "С нами приехала",
                "Анна вчера вечером.",
                "С нами приехала Анна вчера вечером"
            ),
            "С нами приехала Анна вчера вечером."
        );
        assert_eq!(
            joined(
                "Это очень очень",
                "Важное замечание.",
                "Очень очень важное замечание"
            ),
            "Это очень очень важное замечание."
        );
        assert_eq!(
            joined(
                "Проверим новый",
                "Интерфейс — это следующий шаг.",
                "Проверим новый интерфейс — это следующий шаг"
            ),
            "Проверим новый интерфейс — это следующий шаг."
        );
    }

    #[test]
    fn disagreements_and_ambiguous_matches_preserve_the_main_transcript() {
        for context in [
            "Остались старые записи для проверки",
            "Остались новые записи для проверки. Остались новые записи для проверки",
            "Остались новые... Записи для проверки",
            "Остались. Новые записи для проверки",
            "Остались новые записи. Для проверки",
            "Остались новые «записи для проверки»",
        ] {
            assert_eq!(
                joined("Остались новые", "Записи для проверки.", context),
                "Остались новые Записи для проверки."
            );
        }
    }

    #[test]
    fn avoids_closed_sentences_initials_acronyms_and_technical_text() {
        for (left, right) in [
            ("Работа завершена.", "Теперь проверим результат"),
            ("Работа завершена,", "Теперь проверим результат"),
            ("Участник встречи", "А. Иванов выступит первым"),
            ("Проверим работу", "API нашего приложения"),
            ("Пример `остались новые", "Записи для проверки`"),
            ("Сайт https://example.org/test", "Записи для проверки"),
            ("Остались новые", "Записи/каталог для проверки"),
            ("Новые", "Записи для проверки"),
        ] {
            assert!(!needs_context(left, right), "{left} | {right}");
        }
    }
}
