import { useEffect, useRef, useState } from "react";
import { invoke } from "../bridge";
import type { ConfigResult, PreviewFormatResult, PreviewReplacementsResult, ReplacementMatchMode, ReplacementRule, TextFormattingConfig } from "../bridge/types";
import { Card, PageHeader, Switch } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Foldable } from "../components/Foldable";
import { Hint } from "../components/Hint";
import { confirmDestructive } from "../components/ConfirmDialog";
import { CustomSelect, type SelectOption } from "../components/CustomSelect";
import { DiffBlock } from "../components/DiffBlock";
import { t, tPlural } from "../i18n";
import { textPreview, replacementExamples } from "./textExamples";
import { DictionaryLibrary } from "./DictionaryLibrary";
import { Modal } from "../components/Modal";
import { getParasiteSets, type ParasiteSet } from "../bridge/dictionaries";

const MATCH_LABELS = (): Record<string, string> => ({ word: t("Слово"), phrase: t("Фраза"), contains: t("Внутри"), regex: "Regex" });

function makeReplacementRule(find = "", replace = ""): ReplacementRule {
  const id = typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : `rule-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return { id, find, replace, enabled: true, match: "word", case_sensitive: false, preserve_case: false, usage_count: 0 };
}

function replacementRulesFromConfig(config: ConfigResult | null): ReplacementRule[] {
  const rules = Array.isArray(config?.replacement_rules) ? config!.replacement_rules : [];
  if (rules.length) return rules.map((rule) => ({ ...makeReplacementRule(), ...rule, find: rule.find ?? "", replace: rule.replace ?? "" }));
  return Object.entries(config?.replacements ?? {}).map(([find, replace]) => ({ ...makeReplacementRule(find, replace), id: `legacy-${find}` }));
}

function replacementRulesToLegacyRecord(rules: ReplacementRule[]): Record<string, string> {
  const result: Record<string, string> = {};
  for (const rule of rules) {
    const find = rule.find.trim();
    if (find && rule.enabled) result[find] = rule.replace;
  }
  return result;
}

function validateReplacementRules(rules: ReplacementRule[]): string | null {
  const seen = new Set<string>();
  for (const rule of rules) {
    const find = rule.find.trim();
    if (!find) return t("У каждого правила должно быть заполнено поле поиска.");
    const key = `${rule.match}:${find.toLowerCase()}`;
    if (seen.has(key)) return t("Правила с одинаковым поиском и режимом совпадения конфликтуют между собой.");
    seen.add(key);
    if (rule.find === rule.replace && rule.match !== "regex") return t("Одно из правил ничего не меняет: текст поиска совпадает с заменой.");
  }
  return null;
}

function downloadTextFile(filename: string, text: string, mime = "application/json;charset=utf-8") {
  const blob = new Blob([text], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function ReplacementsEmptyState() {
  return (
    <div style={{ display: "grid", placeItems: "center", padding: "46px 24px", color: "var(--ink-mute)", textAlign: "center" }}>
      <div style={{ marginBottom: 12, opacity: 0.75 }}><Icon name="replace" size={32}/></div>
      <div style={{ font: "500 14px/1.3 var(--font-sans)", color: "var(--ink-dim)" }}>{t("Нет правил замены")}</div>
      <div style={{ marginTop: 6, maxWidth: 390, font: "400 12px/1.5 var(--font-sans)" }}>{t("Добавьте слово или фразу, которые нужно автоматически исправлять после распознавания.")}</div>
    </div>
  );
}

const FORMAT_DEFAULTS: TextFormattingConfig = {
  enabled: true,
  remove_hallucinations: true,
  remove_fillers: true,
  remove_parasites: true,
  remove_duplicates: true,
  collapse_phrase_loops: true,
  clean_commas: true,
  normalize_spaces: true,
  correct_spelling: true,
  split_sentences: false,
  capitalize_sentences: true,
  final_punctuation: true,
  custom_parasite_words: [],
  disabled_parasite_words: [],
  custom_words: [],
  enabled_presets: [],
};

type FormatRule = { key: keyof TextFormattingConfig; title: string; sub: string };

// The master switch for the whole local pass stands apart from the list: it
// goes into the header of the «Очистка» card rather than into its body.
const MASTER_RULE = (): FormatRule => (
  { key: "enabled", title: t("Включить форматирование"), sub: t("Главный переключатель всего локального пайплайна") }
);

const CLEAN_RULES = (): FormatRule[] => ([
  { key: "remove_hallucinations", title: t("Убирать артефакты распознавания"), sub: t("«субтитры сделал…», «спасибо за просмотр», [Music]; если кроме них ничего нет — вставка отменяется") },
  { key: "remove_fillers", title: t("Удалять заполнители"), sub: t("э-э, ммм, а-а и похожие звуки; английские uh, umm, hmm — при английской диктовке") },
  { key: "remove_parasites", title: t("Удалять слова-паразиты"), sub: t("встроенный список только русский; свои слова работают на любом языке") },
  { key: "remove_duplicates", title: t("Удалять повторы"), sub: t("я я хочу -> я хочу") },
  { key: "collapse_phrase_loops", title: t("Схлопывать зациклившиеся фразы"), sub: t("я думаю что. я думаю что. я думаю что. -> я думаю что.") },
  { key: "clean_commas", title: t("Чистить запятые"), sub: t("двойные запятые и запятая в начале текста") },
  { key: "normalize_spaces", title: t("Нормализовать пробелы"), sub: t("лишние пробелы и пропущенные пробелы после знаков в русском тексте") },
  { key: "correct_spelling", title: t("Исправлять опечатки"), sub: t("при русском языке диктовки: однозначные исправления по встроенному словарю, без LLM") },
  { key: "split_sentences", title: t("Разбивать длинные предложения"), sub: t("мягкое разделение длинных фраз по связкам") },
  { key: "capitalize_sentences", title: t("Капитализация предложений"), sub: t("заглавная буква в начале текста и после точки") },
  { key: "final_punctuation", title: t("Финальная пунктуация"), sub: t("добавлять точку, если фраза без знака в конце") },
]);

function normalizeTextFormatting(config: ConfigResult | null): TextFormattingConfig {
  return { ...FORMAT_DEFAULTS, ...(config?.text_formatting ?? {}) };
}

/** The name of a built-in set: its language, or the bare code for a language
 * nobody has written a caption for yet. */
function parasiteSetLabel(language: string): string {
  const names: Record<string, string> = { ru: t("Русские"), en: t("Английские") };
  return names[language] ?? language.toUpperCase();
}

function parseCustomWords(value: string): string[] {
  return value.split(/[\n,]/).map((item) => item.trim()).filter(Boolean);
}

// A versioned key: the default changed to "everything collapsed", and a saved
// choice under the old key would have overridden it for everyone who had already
// opened this page. Resetting the folds once is cheaper than a default nobody
// ever sees.
const TEXT_FOLDS_KEY = "sotto.text.folds.v2";

/** «Обработка → Текст»: the entire local pass — cleanup, replacements,
 * dictionaries.
 *
 * These used to be two pages, «Форматирование» and «Замены». The split was a
 * fiction: in the backend one `Formatter::process` runs both halves, and
 * `preview_format` already applied the replacements — that is, the preview on
 * «Форматирование» showed a result its own switches did not explain. Here there
 * is one pass and one preview. */
export function TextPage({ config, onConfigChanged, previewDraft, onPreviewDraftChange }: { config: ConfigResult | null; onConfigChanged: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null>; previewDraft: string | null; onPreviewDraftChange: (text: string) => void }) {
  // ── Cleanup and dictionaries: saved immediately, no draft ──────────────
  const formatting = normalizeTextFormatting(config);
  const [newParasite, setNewParasite] = useState("");
  // What has been asked for but not yet confirmed — see `saveParasites`.
  const [parasiteDraft, setParasiteDraft] = useState<Partial<TextFormattingConfig>>({});
  const parasiteWrites = useRef(0);
  // The sets come from the backend rather than being copied into the frontend:
  // a word added there must appear here without a second edit, and a list that
  // silently disagrees with the step is exactly the failure this section exists
  // to fix.
  const [parasiteSets, setParasiteSets] = useState<ParasiteSet[]>([]);
  const [parasitesOpen, setParasitesOpen] = useState(false);
  useEffect(() => {
    let alive = true;
    // A failure leaves the section empty rather than showing a wrong list: the
    // switch above still works, and the words are not misreported.
    void getParasiteSets().then((sets) => { if (alive) setParasiteSets(sets); }).catch(() => {});
    return () => { alive = false; };
  }, []);

  // ── Replacements: a draft until the «Сохранить» button ─────────────────
  const configRules = replacementRulesFromConfig(config);
  const paused = config?.replacements_paused ?? false;
  const [rules, setRules] = useState<ReplacementRule[]>(() => configRules);
  const [filter, setFilter] = useState("");
  const [formError, setFormError] = useState<string | null>(null);
  const [savingRules, setSavingRules] = useState(false);
  const [saved, setSaved] = useState(true);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const findRef = useRef<HTMLInputElement>(null);

  // ── A shared preview of the whole local pass ───────────────────────────
  const previewText = textPreview(previewDraft);
  const [previewResult, setPreviewResult] = useState("");
  const [previewMatches, setPreviewMatches] = useState<PreviewReplacementsResult["matched_rules"]>([]);
  const [previewError, setPreviewError] = useState<string | null>(null);

  const [folds, setFolds] = useState<Record<string, boolean>>(() => {
    try {
      const stored = window.localStorage.getItem(TEXT_FOLDS_KEY);
      if (stored) return JSON.parse(stored) as Record<string, boolean>;
    } catch {/* ignore */}
    // The page opens as a list of what it contains rather than the expanded
    // contents of two blocks: with «Очистка» and «Замены» expanded you had to
    // scroll to reach the preview on the right.
    return { clean: false, repl: false, dict: false };
  });

  function toggleFold(id: string) {
    setFolds((current) => {
      const next = { ...current, [id]: !current[id] };
      try { window.localStorage.setItem(TEXT_FOLDS_KEY, JSON.stringify(next)); } catch {/* ignore */}
      return next;
    });
  }

  useEffect(() => {
    setRules(configRules);
    setSaved(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reset the draft when the saved rules change in content, not on every new array.
  }, [JSON.stringify(config?.replacement_rules ?? []), JSON.stringify(config?.replacements ?? {})]);

  // Two calls for one preview: `preview_format` gives the full local result
  // (cleanup AND replacements at once), while `preview_replacements` gives only
  // metadata about which rules fired. The first does not return that.
  useEffect(() => {
    let cancelled = false;
    const timer = window.setTimeout(() => {
      const patch = { replacement_rules: rules };
      Promise.all([
        invoke<PreviewFormatResult>("preview_format", { text: previewText, patch }),
        invoke<PreviewReplacementsResult>("preview_replacements", { text: previewText, patch }),
      ])
        .then(([format, replacements]) => {
          if (cancelled) return;
          setPreviewResult(format.formatted || "");
          setPreviewMatches(replacements.matched_rules ?? []);
          setPreviewError(null);
        })
        .catch((e) => {
          if (cancelled) return;
          setPreviewResult("");
          setPreviewMatches([]);
          setPreviewError(e instanceof Error ? e.message : String(e));
        });
    }, 180);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- content-keyed; the formatting and pause settings are read by the backend preview, so they re-run it too.
  }, [previewText, JSON.stringify(rules), JSON.stringify(formatting), paused]);

  // The cleanup settings save themselves, with no button and no indicator: the
  // checkbox is the confirmation — it stays in its new position once the config
  // comes back. A separate "saving" pill used to live in the page header and
  // flashed at every sneeze.
  async function saveFormatting(patch: Partial<TextFormattingConfig>): Promise<boolean> {
    return Boolean(await onConfigChanged({ text_formatting: patch as TextFormattingConfig }));
  }

  /// Save one parasite-list change, and show it before the backend confirms it.
  ///
  /// `config` only moves when a write comes back, so two clicks in a row both
  /// read the same stale value: switching «короче» off and then «типа» sent a
  /// patch built from a list that still had neither, and the second write
  /// dropped the first. The draft holds what we have already asked for and is
  /// what every next change is computed from.
  ///
  /// It is cleared only when no write is still in flight — clearing it per
  /// answer would briefly show the first result while the second was still on
  /// its way. A failed write clears with the rest, so the list falls back to
  /// what the backend actually holds rather than to a change that never landed.
  async function saveParasites(patch: Partial<TextFormattingConfig>): Promise<boolean> {
    setParasiteDraft((current) => ({ ...current, ...patch }));
    parasiteWrites.current += 1;
    try {
      return await saveFormatting(patch);
    } finally {
      parasiteWrites.current -= 1;
      if (parasiteWrites.current === 0) setParasiteDraft({});
    }
  }

  const customParasites = parasiteDraft.custom_parasite_words ?? formatting.custom_parasite_words ?? [];

  /// Commit whatever is in the input. A word is added on Enter and on blur, so
  /// a word typed and then clicked away from is not silently thrown out — that
  /// is what a textarea saved on blur used to promise and a list has to keep.
  ///
  /// The field is cleared only once the write has come back. Clearing it first
  /// meant a failed save took the phrase with it: gone from the list it never
  /// reached and gone from the field it was typed into, with nothing left to
  /// retry from.
  async function addCustomParasites() {
    const known = new Set(customParasites.map((word) => word.toLowerCase()));
    const added: string[] = [];
    for (const word of parseCustomWords(newParasite)) {
      // `known` grows as we go, so «вроде, вроде» adds one word, not two.
      const key = word.toLowerCase();
      if (known.has(key)) continue;
      known.add(key);
      added.push(word);
    }
    // Nothing new to write — a blank field, or a word already on the list. The
    // input has served its purpose either way.
    if (added.length === 0) { setNewParasite(""); return; }
    if (await saveParasites({ custom_parasite_words: [...customParasites, ...added] })) setNewParasite("");
  }

  function removeCustomParasite(word: string) {
    void saveParasites({ custom_parasite_words: customParasites.filter((item) => item !== word) });
  }

  const disabledParasites = parasiteDraft.disabled_parasite_words ?? formatting.disabled_parasite_words ?? [];
  const parasiteIsOff = (word: string) => disabledParasites.some((off) => off.trim().toLowerCase() === word.toLowerCase());
  // Absent means nobody has chosen and each set applies by its own default; an
  // empty array is a choice — no built-in set at all. The two must not be
  // conflated, or switching the last set off would silently turn it back on.
  const chosenSets = "parasite_sets" in parasiteDraft ? parasiteDraft.parasite_sets : formatting.parasite_sets;
  const setIsOn = (set: ParasiteSet) => chosenSets ? chosenSets.includes(set.id) : set.default_on;
  // The set for the language being dictated goes first; «auto» keeps the
  // backend's own order, because there is nothing to sort by yet.
  const dictationLanguage = config?.language ?? "ru";
  const orderedSets = [...parasiteSets].sort((a, b) =>
    Number(b.language === dictationLanguage) - Number(a.language === dictationLanguage));
  const activeWords = orderedSets.filter(setIsOn).flatMap((set) => set.words);
  const offCount = activeWords.filter(parasiteIsOff).length;
  const parasiteSummary = [
    `${activeWords.length} ${tPlural(activeWords.length, ["слово", "слова", "слов"])}`,
    ...(offCount > 0 ? [t("{count} выключено", { count: offCount })] : []),
  ].join(" · ");

  function toggleParasite(word: string) {
    const next = parasiteIsOff(word)
      ? disabledParasites.filter((off) => off.trim().toLowerCase() !== word.toLowerCase())
      : [...disabledParasites, word];
    void saveParasites({ disabled_parasite_words: next });
  }

  function toggleParasiteSet(set: ParasiteSet) {
    // The first change writes out the full resolved selection rather than a
    // one-element list, so the sets left alone keep whatever they resolved to.
    const current = parasiteSets.filter(setIsOn).map((item) => item.id);
    const next = setIsOn(set) ? current.filter((id) => id !== set.id) : [...current, set.id];
    void saveParasites({ parasite_sets: next });
  }

  function updateRule(id: string, patch: Partial<ReplacementRule>) {
    setRules((current) => current.map((rule) => rule.id === id ? { ...rule, ...patch } : rule));
    setSaved(false);
  }

  function moveRule(id: string, direction: -1 | 1) {
    setRules((current) => {
      const index = current.findIndex((rule) => rule.id === id);
      const nextIndex = index + direction;
      if (index < 0 || nextIndex < 0 || nextIndex >= current.length) return current;
      const next = [...current];
      [next[index], next[nextIndex]] = [next[nextIndex], next[index]];
      return next;
    });
    setSaved(false);
  }

  async function deleteRule(id: string) {
    const rule = rules.find((item) => item.id === id);
    const label = rule?.find?.trim();
    const question = label
      ? t("Удалить правило «{p0}»? Это действие нельзя отменить.", { p0: label })
      : t("Удалить это правило? Это действие нельзя отменить.");
    if (!await confirmDestructive(question)) return;
    setRules((current) => current.filter((item) => item.id !== id));
    setSaved(false);
  }

  async function saveRules(nextRules = rules) {
    const validation = validateReplacementRules(nextRules);
    if (validation) {
      setFormError(validation);
      return;
    }
    setFormError(null);
    setSavingRules(true);
    try {
      // A rejected save never lands here: onConfigChanged reports backend
      // failures in the window-wide banner and answers null. So only a config
      // that actually came back may mark the rules saved — the page keeps the
      // unsaved state and an enabled button for a retry.
      const result = await onConfigChanged({ replacement_rules: nextRules, replacements: replacementRulesToLegacyRecord(nextRules) });
      if (result) {
        setRules(replacementRulesFromConfig(result));
        setSaved(true);
      }
    } finally {
      setSavingRules(false);
    }
  }

  async function setPaused(nextPaused: boolean) {
    setSavingRules(true);
    try {
      await onConfigChanged({ replacements_paused: nextPaused });
    } finally {
      setSavingRules(false);
    }
  }

  function addRule(find = "", replace = "") {
    setRules((current) => [makeReplacementRule(find, replace), ...current]);
    setSaved(false);
    setFolds((current) => ({ ...current, repl: true }));
    window.setTimeout(() => findRef.current?.focus(), 0);
  }

  function exportRules() {
    downloadTextFile("replacement-rules.json", JSON.stringify({ replacement_rules: rules }, null, 2));
  }

  async function importRules(file: File) {
    try {
      const parsed = JSON.parse(await file.text());
      const imported = Array.isArray(parsed?.replacement_rules)
        ? parsed.replacement_rules.map((rule: Partial<ReplacementRule>) => ({ ...makeReplacementRule(), ...rule }))
        : Object.entries(parsed as Record<string, string>).map(([find, replace]) => makeReplacementRule(find, String(replace)));
      const next = [...rules, ...imported].filter((rule) => rule.find.trim());
      const validation = validateReplacementRules(next);
      if (validation) throw new Error(validation);
      setRules(next);
      setSaved(false);
      setFormError(null);
    } catch (e) {
      setFormError(e instanceof Error ? e.message : t("Не удалось импортировать JSON."));
    } finally {
      if (fileInputRef.current) fileInputRef.current.value = "";
    }
  }

  const normalizedFilter = filter.trim().toLowerCase();
  const visibleRules = normalizedFilter ? rules.filter((rule) => `${rule.find}\n${rule.replace}`.toLowerCase().includes(normalizedFilter)) : rules;
  const activeCount = rules.filter((rule) => rule.enabled).length;
  const cleanRules = CLEAN_RULES();
  const activeCleanCount = cleanRules.filter((rule) => Boolean(formatting[rule.key])).length;
  const masterRule = MASTER_RULE();
  // The backend does not apply paused replacements in the full pass, while the
  // preview of that single stage always applies them — otherwise it would be
  // useless exactly when the rules are being set up. The discrepancy is
  // labelled.
  const matchesAreHypothetical = paused;
  // The diff is shown only when there is something to compare with; otherwise
  // the result card holds an "enter some text" prompt.
  const showPreviewDiff = Boolean(previewText.trim() && previewResult);

  return (
    <div className="page">
      <input type="file" accept=".json,application/json" ref={fileInputRef} style={{ display: "none" }} onChange={(e) => { const file = e.target.files?.[0]; if (file) void importRules(file); }}/>
      {/* There are no state pills in the header: both duplicated what is
          visible next to the blocks themselves — the rule counter sits on
          «Замены», and «не сохранено» lights up right there by the save button.
          A permanent green "saved" pill across the whole screen reported only
          that nothing had happened. */}
      <PageHeader title={t("Текст")}/>

      <div className="text-grid">
        <div className="flex-col" style={{ gap: 12, minWidth: 0 }}>
          <Foldable
            open={Boolean(folds.clean)}
            onToggle={() => toggleFold("clean")}
            title={t("Очистка")}
            hint={t("Удаляет из распознанного текста слова-паразиты, повторы и лишние пробелы, исправляет пунктуацию. Применяются только включённые правила.")}
            summary={<span className="head-count">{activeCleanCount}/{cleanRules.length}</span>}
            /* There is no "enabled" pill next to the switch: it said exactly
               what the switch's position said, and in a narrow column it pushed
               the header onto a second line. */
            aside={<Hint text={masterRule.sub}><Switch on={formatting.enabled} onChange={(next) => void saveFormatting({ enabled: next })}/></Hint>}
          >
            <div className="fold__rows">
              {cleanRules.map((opt, i) => {
                const value = Boolean(formatting[opt.key]);
                return (
                  <div key={String(opt.key)} style={{ padding: "10px 12px", borderBottom: i === cleanRules.length - 1 ? "none" : "1px solid var(--line-soft)", display: "flex", alignItems: "center", gap: 10, opacity: formatting.enabled ? 1 : 0.55 }}>
                    <div className="flex-grow" style={{ minWidth: 0 }}>
                      <div style={{ font: "500 13px/1.2 var(--font-sans)", color: "var(--ink)" }}>{opt.title}</div>
                      <div style={{ font: "400 11.5px/1.4 var(--font-sans)", color: "var(--ink-mute)", marginTop: 2 }}>{opt.sub}</div>
                      {/* The word list opens from the row it belongs to. It
                          used to sit at the bottom of the card, ten rows away
                          from its own switch, where it read as loose clutter.
                          The summary keeps the point of showing it at all: how
                          many words the step removes, and how many you stopped. */}
                      {opt.key === "remove_parasites" && parasiteSets.length > 0 &&
                        <button type="button" className="btn btn--ghost" style={{ marginTop: 6, height: 26 }} onClick={() => setParasitesOpen(true)}>
                          <Icon name="sliders" size={12}/>{t("Список")}: {parasiteSummary}
                        </button>}
                    </div>
                    <Switch label={opt.title} on={value} onChange={(next) => void saveFormatting({ [opt.key]: next })}/>
                  </div>
                );
              })}
            </div>
          </Foldable>

          {parasitesOpen && <Modal title={t("Слова-паразиты")} className="parasite-modal" onClose={() => setParasitesOpen(false)}>
            <div className="modal__body parasite-body">
              {/* How a chip works is the same in every set, so it is said once,
                  under the title. Repeated per section it was the same sentence
                  twice on one screen, and it grew with every language added. */}
              {orderedSets.some(setIsOn) &&
                <p className="parasite-note parasite-intro">{t("Нажмите на слово, чтобы перестать его удалять. Зачёркнутые остаются в тексте.")}</p>}
              {orderedSets.map((set) => {
                const on = setIsOn(set);
                return (
                  <section key={set.id}>
                    <div className="parasite-set__head">
                      <h3 className="parasite-heading">{parasiteSetLabel(set.language)}</h3>
                      <Switch on={on} label={parasiteSetLabel(set.language)} onChange={() => toggleParasiteSet(set)}/>
                    </div>
                    {/* An off set shows its switch and the reason, and nothing
                        else. Its words cannot be removed from anything, and a
                        list of them is the clutter that made an English reader
                        stare at thirteen Cyrillic chips. */}
                    {!on && <p className="parasite-note">{t("Набор выключен: эти слова из текста не удаляются.")}</p>}
                    {on && <div className="parasite-chips">
                      {set.words.map((word) => {
                        const off = parasiteIsOff(word);
                        return (
                          <button
                            key={word}
                            type="button"
                            className="pill parasite-chip"
                            data-off={off ? "true" : "false"}
                            aria-pressed={!off}
                            onClick={() => toggleParasite(word)}
                          >{word}</button>
                        );
                      })}
                    </div>}
                  </section>
                );
              })}
              <section>
                <h3 className="parasite-heading">{t("Свои слова-паразиты")}</h3>
                <p className="parasite-note">{t("Введите слово или фразу и нажмите Enter. Несколько сразу можно разделить запятыми.")}</p>
                {/* A list, not a text field. The field was a textarea whose
                    contents were parsed on save: a word typed into it never
                    became anything you could see or take back out, while the
                    built-in words right above it were chips all along. */}
                <div className="flex-row" style={{ gap: 8 }}>
                  <input
                    className="field flex-grow"
                    value={newParasite}
                    onChange={(e) => setNewParasite(e.target.value)}
                    onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); void addCustomParasites(); } }}
                    onBlur={() => void addCustomParasites()}
                    placeholder={t("например: собственно")}
                    aria-label={t("Своё слово-паразит")}
                    style={{ height: 30 }}
                  />
                  <button type="button" className="btn btn--ghost" style={{ height: 30 }} disabled={!newParasite.trim()} onClick={() => void addCustomParasites()}><Icon name="plus" size={12}/>{t("Добавить")}</button>
                </div>
                {customParasites.length > 0 && <div className="parasite-chips" style={{ marginTop: 8 }}>
                  {customParasites.map((word) => (
                    <span key={word} className="pill parasite-chip parasite-chip--own">
                      {word}
                      <button type="button" className="parasite-chip__remove" aria-label={`${t("Удалить")}: ${word}`} onClick={() => removeCustomParasite(word)}><Icon name="x" size={10}/></button>
                    </span>
                  ))}
                </div>}
              </section>
            </div>
            {/* No footer. Everything in here saves as it is changed — a chip on
                click, a word on Enter or on blur — so a «Сохранить» would
                promise something that already happened, and a second «Закрыть»
                beside the × in the header is one control saying what the other
                one says. */}
          </Modal>}

          <Foldable
            open={Boolean(folds.repl)}
            onToggle={() => toggleFold("repl")}
            title={t("Замены")}
            hint={t("Заменяет найденные слова и фразы по вашим правилам: например, «щас» на «сейчас». Учитывает настройки регистра и совпадения каждого правила.")}
            summary={<>
              <span className="head-count">{activeCount}/{rules.length}</span>
              {!saved && <span className="pill warn">{t("не сохранено")}</span>}
            </>}
            aside={<Hint text={paused ? t("Замены на паузе") : t("Замены применяются")}><Switch on={!paused} onChange={(next) => void setPaused(!next)}/></Hint>}
          >
            <div style={{ padding: "10px 12px", display: "grid", gap: 10 }}>
              <div className="flex-row" style={{ gap: 8, flexWrap: "wrap" }}>
                <div className="input-search" style={{ flex: "1 1 180px" }}>
                  <Icon name="search" size={13} className="input-search__icon"/>
                  <input className="field" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={t("Найти правило")} style={{ height: 32 }}/>
                </div>
                <button className="btn btn--ghost" onClick={() => addRule()} disabled={savingRules}><Icon name="plus" size={13}/>{t("Добавить")}</button>
                <button className="btn btn--primary" onClick={() => void saveRules()} disabled={savingRules || saved}><Icon name="check" size={12}/>{savingRules ? t("Сохраняю") : t("Сохранить")}</button>
              </div>

              <div className="fold__rows">
                {visibleRules.length === 0 ? <ReplacementsEmptyState/> : <div>{visibleRules.map((rule, index) => <div key={rule.id} style={{ padding: 10, display: "grid", gap: 8, borderBottom: index < visibleRules.length - 1 ? "1px solid var(--line-soft)" : "none", opacity: rule.enabled ? 1 : 0.66 }}>
                  <div className="flex-row" style={{ gap: 8, flexWrap: "wrap" }}><Switch on={rule.enabled} onChange={(enabled) => updateRule(rule.id, { enabled })}/><span className="pill mono">{MATCH_LABELS()[rule.match]}</span><span className="pill mono">{rule.usage_count || 0}  {t("сраб.")}</span>{!rule.replace && <span className="pill warn">{t("удаляет текст")}</span>}<div style={{ marginLeft: "auto", display: "flex", gap: 4 }}><button className="btn btn--ghost" style={{ height: 24, padding: "0 7px" }} onClick={() => moveRule(rule.id, -1)} disabled={index === 0} aria-label={t("Поднять")}><Icon name="chev-down" size={12} style={{ transform: "rotate(180deg)" }}/></button><button className="btn btn--ghost" style={{ height: 24, padding: "0 7px" }} onClick={() => moveRule(rule.id, 1)} disabled={index === visibleRules.length - 1} aria-label={t("Опустить")}><Icon name="chev-down" size={12}/></button><button className="btn btn--ghost" style={{ height: 24, padding: "0 7px", color: "var(--err)" }} onClick={() => void deleteRule(rule.id)} aria-label={t("Удалить")}><Icon name="trash" size={12}/></button></div></div>
                  <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) 24px minmax(0, 1fr)", gap: 8, alignItems: "center" }}><input ref={index === 0 ? findRef : undefined} className="field mono" value={rule.find} onChange={(e) => updateRule(rule.id, { find: e.target.value })} placeholder={t("что искать")} style={{ height: 30 }}/><Icon name="arrow-right" size={13} style={{ color: "var(--ink-mute)" }}/><input className="field mono" value={rule.replace} onChange={(e) => updateRule(rule.id, { replace: e.target.value })} placeholder={t("на что заменить")} style={{ height: 30 }}/></div>
                  <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(120px, 1fr))", gap: 8 }}><CustomSelect<string> value={rule.match} options={Object.entries(MATCH_LABELS()).map<SelectOption<string>>(([value, label]) => ({ value, label }))} onChange={(next) => updateRule(rule.id, { match: next as ReplacementMatchMode })}/><label className="pill" style={{ justifyContent: "center", cursor: "pointer" }}><input className="checkbox" type="checkbox" checked={rule.case_sensitive} onChange={(e) => updateRule(rule.id, { case_sensitive: e.target.checked })}/> Aa</label><label className="pill" style={{ justifyContent: "center", cursor: "pointer" }}><input className="checkbox" type="checkbox" checked={rule.preserve_case} onChange={(e) => updateRule(rule.id, { preserve_case: e.target.checked })}/>  {t("Регистр")}</label></div>
                </div>)}</div>}
                {formError && <div role="alert" style={{ padding: "10px 12px", color: "var(--err)", font: "500 12px/1.4 var(--font-sans)", borderTop: "1px solid var(--line)" }}>{formError}</div>}
              </div>

              {/* Export and import are one-off operations, and on one line with
                  the search and «Сохранить» they stood between frequent actions.
                  The captions are mandatory: without them «Импорт» is just a
                  glyph. */}
              <div className="flex-row" style={{ gap: 6, justifyContent: "flex-end" }}>
                <button className="btn btn--ghost" style={{ height: 26 }} onClick={exportRules} disabled={rules.length === 0}><Icon name="download" size={12}/>{t("Экспорт")}</button>
                <button className="btn btn--ghost" style={{ height: 26 }} onClick={() => fileInputRef.current?.click()}><Icon name="folder" size={12}/>{t("Импорт")}</button>
              </div>
            </div>
          </Foldable>

          <DictionaryLibrary open={Boolean(folds.dict)} onToggle={() => toggleFold("dict")} formatting={formatting} onSave={async (patch) => Boolean(await onConfigChanged({ text_formatting: patch as TextFormattingConfig }))}/>
        </div>

        {/* The heading and the explanation live inside the first card rather
            than above it: in the left column block headings sit inside the
            border, and an external caption above the card read as alien. The
            «до» and «после» steps are labelled with text — pills turned a
            utility caption into an accent that competed with the card's own
            content. */}
        <div className="flex-col" style={{ gap: 12, minWidth: 0 }}>
          <Card>
            <div className="preview-card__head">
              <h2 className="preview-card__title">
                {t("Живой предпросмотр")}
                <Hint text={t("Весь локальный проход: очистка и замены — ровно то, что уходит в модель или во вставку.")}/>
              </h2>
              <span className="preview-card__step">{t("До")}</span>
            </div>
            <textarea className="field mono" value={previewText} onChange={(e) => onPreviewDraftChange(e.target.value)} placeholder={t("Введите текст для проверки обработки")} style={{ width: "100%", minHeight: 145, padding: 12, resize: "vertical", lineHeight: 1.55 }}/>
          </Card>
          <div className="preview-pair__arrow preview-pair__arrow--down" aria-hidden="true"><Icon name="arrow-right" size={14}/></div>
          <Card>
            <div className="preview-card__head">
              <span className="preview-card__step preview-card__step--after">{t("После")}</span>
            </div>
            {previewError ? <p style={{ margin: 0, font: "500 12px/1.55 var(--font-sans)", color: "var(--err)" }}>{previewError}</p> : <>
              {/* The result shows only the diff: it is the processed text,
                  simply with the changes highlighted. A separate paragraph above
                  it printed the same string a second time. */}
              {showPreviewDiff
                ? <DiffBlock before={previewText} after={previewResult} title={t("Diff: исходный → после обработки")} />
                : <p style={{ margin: 0, font: "400 13px/1.65 var(--font-sans)", color: "var(--ink-mute)", whiteSpace: "pre-wrap" }}>{t("Введите текст для предпросмотра")}</p>}
              <div className="flex-row" style={{ flexWrap: "wrap", gap: 6, marginTop: 10 }}>{previewMatches?.length ? previewMatches.map((item) => <span className={matchesAreHypothetical ? "pill" : "pill ok"} key={`${item.id}-${item.find}`}>{item.find}: {item.count}</span>) : <span className="pill">{t("Сработало 0 правил")} {rules.length === 0 ? t("— правил пока нет") : ""}</span>}</div>
              {matchesAreHypothetical && previewMatches?.length ? <div style={{ marginTop: 6, font: "400 11px/1.4 var(--font-sans)", color: "var(--warn)" }}>{t("Замены на паузе: правила совпали бы, но в результат выше не попали.")}</div> : null}
            </>}
          </Card>
          <Card pad="rows">
            <div style={{ font: "600 13px/1.2 var(--font-sans)", color: "var(--ink)", display: "inline-flex", alignItems: "center", gap: 5, marginBottom: 8 }}>
              {t("Добавить правило-пример")}
              <Hint text={t("Нажмите, чтобы создать правило — оно сразу попадёт в список слева и в предпросмотр.")}/>
            </div>
            <div className="flex-row" style={{ flexWrap: "wrap", gap: 6 }}>{replacementExamples().map(([find, replace]) => <button key={find} className="btn btn--ghost" onClick={() => addRule(find, replace)} style={{ height: 26 }}><span className="mono">{find}</span><Icon name="arrow-right" size={11}/><span className="mono">{replace}</span></button>)}</div>
          </Card>
        </div>
      </div>
    </div>
  );
}
