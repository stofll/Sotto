"""Мутационный прогон: каждая мутация обязана уронить названный тест.

Мутация вносится в файл, гоняется фильтр тестов, файл восстанавливается из
снимка, сделанного перед правкой, — безусловно, даже если прогон упал с
исключением.

Восстановление именно из снимка, а не через `git checkout`: тот откатывает
до HEAD и вместе с мутацией сносит всю незакоммиченную работу в файле.
Проверено на себе — прогон по незакоммиченным правкам стёр их подчистую.
"""
import io, subprocess, sys, os

ROOT = r"D:/Project/speech-to-text"
RUST = os.path.join(ROOT, "desktop/src-tauri")
FRONT = os.path.join(ROOT, "desktop")

# (label, file, what to replace, with what, how to run, filter)
MUTATIONS = [
    ("отмена больше не перебивает результат", "desktop/src-tauri/src/lib.rs",
     "    if cancelled {\n        return Completion::Cancelled;\n    }", "",
     "rust", "completion_tests"),
    ("пробел снова считается текстом", "desktop/src-tauri/src/lib.rs",
     "Ok(inference) if inference.text.trim().is_empty() => Completion::Empty,",
     "Ok(inference) if inference.text.is_empty() => Completion::Empty,",
     "rust", "completion_tests"),
    ("вставляем даже пустое", "desktop/src-tauri/src/lib.rs",
     "pub(crate) fn is_deliverable(final_text: &str) -> bool {\n    !final_text.trim().is_empty()",
     "pub(crate) fn is_deliverable(final_text: &str) -> bool {\n    let _ = final_text;\n    true",
     "rust", "completion_tests"),
    ("статус LLM снова не сериализуется", "desktop/src-tauri/src/lib.rs",
     "    status.and_then(|s| serde_json::to_string(s).ok())",
     "    status.map(|_| \"{\\\"text\\\":\\\"x\\\"}\".to_string())",
     "rust", "retry_ai_tests"),
    ("тайминги записи затираются", "desktop/src-tauri/src/lib.rs",
     "    let mut stats = existing\n        .and_then(Value::as_object)\n        .cloned()\n        .unwrap_or_default();",
     "    let _ = existing;\n    let mut stats = serde_json::Map::new();",
     "rust", "retry_ai_tests"),
    ("причина провала снова молчит", "desktop/src-tauri/src/lib.rs",
     "    Some(status.skipped_reason.clone())\n        .filter(|r| !r.trim().is_empty())\n        .or_else(|| Some(\"unknown\".to_string()))",
     "    None",
     "rust", "retry_ai_tests"),
    ("текст при ретрае снова не пишется", "desktop/src-tauri/src/lib.rs",
     "        Some(text) => conn.execute(\n            \"UPDATE history SET text = ?1, length = ?2, ai_processing_json = ?3, \\\n             processing_stats_json = ?4 WHERE id = ?5\",",
     "        Some(text) => conn.execute(\n            \"UPDATE history SET ai_processing_json = ?3, \\\n             processing_stats_json = ?4 WHERE id = ?5 AND ?1 = ?1 AND ?2 = ?2\",",
     "rust", "retry_ai_tests"),
    ("гибридный режим снова не доходит до LLM", "desktop/src-tauri/src/lib.rs",
     '        == "hybrid"\n}', '        == "hybrid "\n}',
     "rust", "llm_gate_tests"),
    ("неполный конфиг молча включает сеть", "desktop/src-tauri/src/lib.rs",
     '        .unwrap_or("local")\n        == "hybrid"',
     '        .unwrap_or("hybrid")\n        == "hybrid"',
     "rust", "llm_gate_tests"),
    ("миграция трогает здоровые строки", "desktop/src-tauri/src/migrations/v4.sql",
     "  AND json_type(ai_processing_json, '$.text') = 'text'", "",
     "rust", "migration_v4_tests"),
    ("транслитерация выключена", "desktop/src-tauri/src/formatter.rs",
     "            } else {\n                latin.push_str(translit_char(lower));\n            }",
     "            }",
     "rust", "custom_words_tests"),
    ("бюджет правок расширен на одну", "desktop/src-tauri/src/formatter.rs",
     "    edit_distance(&folded, &key) <= edit_budget(key.len())",
     "    edit_distance(&folded, &key) <= edit_budget(key.len()) + 1",
     "rust", "custom_words_tests"),
    ("бюджет перестал зависеть от длины", "desktop/src-tauri/src/formatter.rs",
     "        0..=6 => 1,\n        7..=11 => 2,\n        _ => 3,",
     "        _ => 1,",
     "rust", "custom_words_tests"),
    ("короткие термины снова не проходят", "desktop/src-tauri/src/formatter.rs",
     "        0..=6 => 1,", "        0..=6 => 0,",
     "rust", "custom_words_tests"),
    # We supply the preset, not the user, so mutations here are not corrupted
    # logic but a bad entry in the data: exactly what the guards are there for.
    ("в набор попал термин, садящийся на обычную речь", "desktop/src-tauri/src/formatter.rs",
     '    "thread",\n];', '    "thread",\n    "buffer",\n];',
     "rust", "custom_words_tests"),
    ("в набор попал термин, который никогда не совпадёт", "desktop/src-tauri/src/formatter.rs",
     '    "thread",\n];', '    "thread",\n    "Vite",\n];',
     "rust", "custom_words_tests"),
    ("выключатель набора стал декоративным", "desktop/src-tauri/src/formatter.rs",
     "        for id in &self.enabled_presets {",
     "        for id in DICTIONARY_PRESETS.iter().map(|s| s.id.to_string()).collect::<Vec<_>>().iter() {",
     "rust", "custom_words_tests"),
    ("набор затирает свои слова пользователя", "desktop/src-tauri/src/formatter.rs",
     "        for word in &self.custom_words {\n            push(word, &mut out, &mut seen);\n        }\n",
     "",
     "rust", "custom_words_tests"),
    ("написание из набора побеждает пользовательское", "desktop/src-tauri/src/formatter.rs",
     "            if seen.contains(&key) {\n                return;\n            }\n",
     "",
     "rust", "custom_words_tests"),
    ("оверлей больше не по центру", "desktop/src-tauri/src/overlay.rs",
     "    let x = monitor_x + ((monitor_w as i32 - window_w as i32) / 2).max(0);",
     "    let x = monitor_x + (monitor_w as i32 - window_w as i32).max(0);",
     "rust", "overlay::tests"),
    ("кламп по абсолюту утаскивает оверлей на основной монитор", "desktop/src-tauri/src/overlay.rs",
     "    let x = monitor_x + ((monitor_w as i32 - window_w as i32) / 2).max(0);",
     "    let x = (monitor_x + (monitor_w as i32 - window_w as i32) / 2).max(0);",
     "rust", "overlay::tests"),
    ("оверлей прилипает к нижней кромке", "desktop/src-tauri/src/overlay.rs",
     "    let y = monitor_y + (monitor_h as i32 - window_h as i32 - bottom_offset).max(0);",
     "    let y = monitor_y + (monitor_h as i32 - window_h as i32).max(0);",
     "rust", "overlay::tests"),
    ("окно шире монитора уезжает за левую кромку", "desktop/src-tauri/src/overlay.rs",
     "((monitor_w as i32 - window_w as i32) / 2).max(0)",
     "((monitor_w as i32 - window_w as i32) / 2)",
     "rust", "overlay::tests"),
    ("неизвестный набор роняет форматирование", "desktop/src-tauri/src/formatter.rs",
     "                continue;\n            };\n            for word in set.words {",
     "                panic!(\"нет набора\");\n            };\n            for word in set.words {",
     "rust", "custom_words_tests"),
    ("регистр найденного больше не переносится", "desktop/src-tauri/src/formatter.rs",
     "fn apply_case_of(matched: &str, canonical: &str) -> String {",
     "fn apply_case_of(matched: &str, canonical: &str) -> String {\n    if true { let _ = matched; return canonical.to_string(); }",
     "rust", "custom_words_tests"),
    ("окно снова тащит служебное слово", "desktop/src-tauri/src/formatter.rs",
     "                    if trimmed >= score {\n                        continue;\n                    }",
     "                    let _ = trimmed;",
     "rust", "custom_words_tests"),
    ("Anthropic снова получает Bearer", "desktop/src-tauri/src/ai/models.rs",
     "                (\"x-api-key\".to_string(), api_key.to_string()),",
     "                (\"Authorization\".to_string(), format!(\"Bearer {api_key}\")),",
     "rust", "ai::models"),
    ("Gemini-префикс ресурса не срезается", "desktop/src-tauri/src/ai/models.rs",
     "            return Some(name.trim_start_matches(\"models/\").to_string());",
     "            return Some(name.to_string());",
     "rust", "ai::models"),
    ("оверлей снова обещает вставку до вставки", "desktop/src/overlay/overlayDetail.ts",
     "    if (state === \"done\") {\n        return polishingMs < POLISHING_LABEL_AFTER_MS",
     "    if (false) {\n        return polishingMs < POLISHING_LABEL_AFTER_MS",
     "front", "src/overlay/overlayDetail.test.ts"),
    ("подпись обработки мигает с первой мс", "desktop/src/overlay/overlayDetail.ts",
     "export const POLISHING_LABEL_AFTER_MS = 600;", "export const POLISHING_LABEL_AFTER_MS = 0;",
     "front", "src/overlay/overlayDetail.test.ts"),
    ("открывающая пунктуация теряется", "desktop/src-tauri/src/formatter.rs",
     "                    out.push_str(leading_punctuation(first.raw));", "",
     "rust", "custom_words_tests"),
    ("разделители пересобираются пробелом", "desktop/src-tauri/src/formatter.rs",
     "                    out.push_str(first.gap);",
     "                    out.push_str(if out.is_empty() { \"\" } else { \" \" });",
     "rust", "custom_words_tests"),
    ("порог снова отсекает короткие диктовки", "desktop/src/pages/aiShared.ts",
     "  llm_min_duration_seconds: 0,", "  llm_min_duration_seconds: 30,",
     "front", "src/pages/aiShared.test.ts"),
    ("нулевой порог профиля подменяется старым конфигом", "desktop/src/pages/aiShared.ts",
     "    llm_min_duration_seconds: profile.llm_min_duration_seconds ?? ai.llm_min_duration_seconds,",
     "    llm_min_duration_seconds: profile.llm_min_duration_seconds || ai.llm_min_duration_seconds,",
     "front", "src/pages/aiShared.test.ts"),
    ("лог снова растёт без потолка", "desktop/src-tauri/src/structured_log.rs",
     "        if self.written >= self.limit {", "        if self.written >= u64::MAX {",
     "rust", "structured_log"),
    ("счётчик забывает уже написанное на диск", "desktop/src-tauri/src/structured_log.rs",
     "        let written = file.metadata().map(|meta| meta.len()).unwrap_or(0);",
     "        let written = 0;",
     "rust", "structured_log"),
    ("архивы не сдвигаются, первый затирается каждый раз", "desktop/src-tauri/src/structured_log.rs",
     "    for index in (1..keep).rev() {\n        let _ = fs::rename(archive_path(path, index), archive_path(path, index + 1));\n    }\n",
     "",
     "rust", "structured_log"),
    ("уборка наследия сносит активный лог", "desktop/src-tauri/src/structured_log.rs",
     "        if same_file(&candidate, active) {\n            continue;\n        }\n",
     "",
     "rust", "structured_log"),
    ("список наследия задевает чужие файлы", "desktop/src-tauri/src/structured_log.rs",
     'const LEGACY_LOG_NAMES: [&str; 2] = ["sidecar.log", "app.log"];',
     'const LEGACY_LOG_NAMES: [&str; 3] = ["sidecar.log", "app.log", "config.json"];',
     "rust", "structured_log"),
    ("очистка оставляет архивы на диске", "desktop/src-tauri/src/structured_log.rs",
     "            remove_archives(path, keep);", "            let _ = keep;",
     "rust", "structured_log"),
    ("размер логов не учитывает архивы", "desktop/src-tauri/src/structured_log.rs",
     "        .chain((1..=keep).map(|index| archive_path(path, index)))",
     "        .chain((1..=keep).skip(usize::MAX).map(|index| archive_path(path, index)))",
     "rust", "structured_log"),

    # ── High-risk invariants ────────────────────────────────────────────
    # A silent failure costs most here: deleting somebody else's data, accepting
    # a substituted model file, letting text rewritten by the LLM through. There
    # was a test for each of these, but nobody had checked it could fail.

    ("история удаляет свежие записи вместо старых", "desktop/src-tauri/src/history.rs",
     '"DELETE FROM history WHERE timestamp <= ?1"',
     '"DELETE FROM history WHERE timestamp >= ?1"',
     "rust", "history::tests"),
    ("срок хранения истории перестал действовать", "desktop/src-tauri/src/history.rs",
     "    if policy.max_age_seconds > 0 {", "    if false {",
     "rust", "history::tests"),
    ("предел по числу записей перестал действовать", "desktop/src-tauri/src/history.rs",
     "    if policy.max_entries > 0 {\n        conn.execute(",
     "    if false {\n        conn.execute(",
     "rust", "history::tests"),

    ("подменённый файл модели принимается как годный", "desktop/src-tauri/src/model.rs",
     "    if !actual.eq_ignore_ascii_case(sha256) {", "    if false {",
     "rust", "model::tests"),
    ("оборванная закачка проходит проверку размера", "desktop/src-tauri/src/model.rs",
     "    if metadata.len() != expected_bytes {", "    if false {",
     "rust", "model::tests"),

    ("LLM снова может съесть половину текста", "desktop/src-tauri/src/ai/fidelity.rs",
     "    kept_word_ratio(input, output).is_some_and(|ratio| ratio < MIN_KEPT_WORD_RATIO)",
     "    kept_word_ratio(input, output).is_some_and(|ratio| ratio < 0.0)",
     "rust", "ai::fidelity"),
    ("страховка не срабатывает ни на какой длине", "desktop/src-tauri/src/ai/fidelity.rs",
     "const MIN_WORDS_TO_JUDGE: usize = 40;", "const MIN_WORDS_TO_JUDGE: usize = usize::MAX;",
     "rust", "ai::fidelity"),
    # Inverting trim_matches: letters are trimmed instead of punctuation, and a
    # token consisting of a single dash stops being discarded. The first version
    # of this mutation added .filter(|_| true) — that is, it changed no behaviour
    # at all, and "missed" said something about the mutation rather than the
    # test.
    ("одинокая пунктуация снова считается словом", "desktop/src-tauri/src/ai/fidelity.rs",
     "                .trim_matches(|c: char| !c.is_alphanumeric())",
     "                .trim_matches(|c: char| c.is_alphanumeric())",
     "rust", "ai::fidelity"),
]


def run(kind, filt):
    if kind == "rust":
        cmd = ["cargo", "test", "--lib", filt]
        cwd = RUST
    else:
        cmd = ["npx", "vitest", "run", filt]
        cwd = FRONT
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, encoding="utf-8", errors="replace", shell=(kind != "rust"))
    return p.returncode, ((p.stdout or "") + (p.stderr or ""))


results = []
for label, relpath, find, repl, kind, filt in MUTATIONS:
    path = os.path.join(ROOT, relpath)
    src = io.open(path, encoding="utf-8").read()
    if find not in src:
        results.append((label, "НЕ НАЙДЕНО В КОДЕ"))
        continue
    io.open(path, "w", encoding="utf-8").write(src.replace(find, repl, 1))
    try:
        code, out = run(kind, filt)
        if code != 0:
            results.append((label, "поймано"))
        else:
            results.append((label, "ПРОПУЩЕНО"))
    finally:
        # From the snapshot rather than git checkout: the file may hold
        # uncommitted work, and rolling back to HEAD would carry it away along
        # with the mutation.
        io.open(path, "w", encoding="utf-8").write(src)
    print(f"  {results[-1][1]:>18}  {label}", flush=True)

print()
caught = sum(1 for _, r in results if r == "поймано")
print(f"поймано {caught} из {len(results)}")
for label, r in results:
    if r != "поймано":
        print(f"  !! {r}: {label}")
sys.exit(0 if caught == len(results) else 1)
