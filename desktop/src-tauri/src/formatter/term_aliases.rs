//! Reviewed recognition spellings, enabled only with their dictionary term.

use std::collections::HashMap;

// Literal aliases deliberately do not use phonetic folding or edit distance.
// Ordinary words (e.g. «ритме», Maine, cloud, code) are not aliases.
const TERMS: &[(&str, &[&str])] = &[
    (
        "GitHub",
        &[
            "гитхаб",
            "гитхаба",
            "гитхабе",
            "гидхаб",
            "гидхабе",
            "гидхап",
            "гитхапе",
            "gidhap",
            "githabe",
        ],
    ),
    ("Cloudflare", &["cloud flayer", "клаудфлер", "клаудфлейр"]),
    ("PostHog", &["постхоg", "пост хок", "пост хог", "пост хоги"]),
    (
        "Whisper",
        &["wisper", "wиспер", "виспер", "виспера", "висперы"],
    ),
    ("Wispr Flow", &["wisper flow", "виспер флоу", "виспр флоу"]),
    ("Parakeet", &["паракит", "паракита", "парокит"]),
    ("GigaAM", &["гига ам", "гигам", "гигаам", "гигм"]),
    ("README", &["ridmi", "ридми", "ритми"]),
    (
        "AGENTS.md",
        &["agencmd", "agence md", "aжиnc md", "эдженс эмди"],
    ),
    ("worktree", &["vork-tри", "vork-tre", "workри", "ворк-три"]),
    ("main", &["мейн", "мейна", "мэйн", "мaйн", "мaйn", "maйн"]),
    ("origin/main", &["origin mane", "originl mane"]),
    ("Sotto", &["сотто"]),
    (
        "REST Assured",
        &[
            "rest asuret",
            "ресташуред",
            "реста шурад",
            "реста шурет",
            "ресташурт",
        ],
    ),
    ("Playwright", &["playrihte", "playrit", "плейрайт"]),
    ("Claude Code", &["клауд код", "клод код"]),
    ("Selenium", &["selnium", "селениум"]),
    ("JUnit", &["gunit", "джей юнит"]),
    ("LLM", &["llм", "ллм"]),
    ("UI", &["юай"]),
    ("SQL", &["sqel", "эскюли", "эс кью эл"]),
];

pub(super) fn supported(term: &str) -> bool {
    TERMS
        .iter()
        .any(|(canonical, _)| canonical.eq_ignore_ascii_case(term))
}

pub(super) fn exact_only(term: &str) -> bool {
    supported(term)
        && !["GitHub", "README"]
            .iter()
            .any(|s| s.eq_ignore_ascii_case(term))
}

pub(super) struct TermAliases {
    matches: HashMap<String, Option<usize>>,
    pub max_window: usize,
}

impl TermAliases {
    pub fn new(terms: &[super::CustomTerm]) -> Self {
        let mut matches = HashMap::new();
        for (index, term) in terms.iter().enumerate() {
            for (_, aliases) in TERMS
                .iter()
                .filter(|(canonical, _)| canonical.eq_ignore_ascii_case(&term.canonical))
            {
                for alias in *aliases {
                    matches
                        .entry((*alias).to_string())
                        .and_modify(|target| {
                            if *target != Some(index) {
                                *target = None;
                            }
                        })
                        .or_insert(Some(index));
                }
            }
        }
        // A user's literal term wins over an alias of another enabled term.
        for (index, term) in terms.iter().enumerate() {
            // Already-spelled catalog terms can also be ordinary English prose
            // ("a whisper", "rest assured"); leave their original case intact.
            matches.insert(
                term.canonical.to_lowercase(),
                (!term.exact_only).then_some(index),
            );
        }
        // Do not turn Wisper Flow into Whisper Flow when only Whisper is enabled.
        // Reserve known longer aliases whose prefix is an active shorter match.
        for (_, aliases) in TERMS {
            for alias in *aliases {
                if alias
                    .match_indices(' ')
                    .any(|(end, _)| matches.contains_key(&alias[..end]))
                {
                    matches.entry((*alias).to_string()).or_insert(None);
                }
            }
        }
        let max_window = matches
            .keys()
            .map(|s| s.split_whitespace().count())
            .max()
            .unwrap_or(0);
        Self {
            matches,
            max_window,
        }
    }

    pub fn find(&self, words: &[&str]) -> Option<(usize, Option<usize>)> {
        let mut key = String::new();
        let mut best = None;
        for (index, word) in words.iter().take(self.max_window).enumerate() {
            if index > 0
                && (!super::trailing_punctuation(words[index - 1]).is_empty()
                    || !super::leading_punctuation(word).is_empty())
            {
                break;
            }
            if index > 0 {
                key.push(' ');
            }
            key.push_str(
                &word
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase(),
            );
            if let Some(target) = self.matches.get(&key) {
                best = Some((index + 1, *target));
            }
        }
        best
    }
}
