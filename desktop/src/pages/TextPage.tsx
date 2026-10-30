import { useEffect, useRef, useState } from "react";
import { invoke } from "../bridge";
import type { ConfigResult, DictionaryMatch, PreviewFormatResult, PreviewReplacementsResult, ReplacementMatchMode, ReplacementRule, TextFormattingConfig } from "../bridge/types";
import { Card, PageHeader, Segmented, Switch } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Foldable } from "../components/Foldable";
import { Hint } from "../components/Hint";
import { confirmDestructive } from "../components/ConfirmDialog";
import { CustomSelect, type SelectOption } from "../components/CustomSelect";
import { DiffText } from "../components/DiffBlock";
import { RowMenu } from "../components/RowMenu";
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
    <div style={{ display: "grid", justifyItems: "center", gap: 4, padding: "18px 12px", textAlign: "center" }}>
      <div style={{ font: "500 13px/1.3 var(--font-sans)", color: "var(--ink-dim)" }}>{t("Нет правил замены")}</div>
      <div style={{ maxWidth: 360, font: "400 12px/1.5 var(--font-sans)", color: "var(--ink-mute)" }}>{t("Добавьте слово или фразу, которые нужно автоматически исправлять после распознавания.")}</div>
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
  normalize_ellipsis: false,
  correct_spelling: true,
  split_sentences: false,
  split_paragraphs: false,
  capitalize_sentences: true,
  final_punctuation: true,
  custom_parasite_words: [],
  disabled_parasite_words: [],
  custom_words: [],
  enabled_presets: [],
};

/** One switch in «Очистка». Steps nobody wants apart share a switch and write
 * all their flags at once; the backend still runs each step on its own flag.
 *
 * The details are an «i» hint beside the title, as elsewhere in the app, rather
 * than two lines of fine print under every row. */
type CleanGroup = { id: string; keys: Array<keyof TextFormattingConfig>; title: string; hint: string };

const MASTER_HINT = () => t("Включает и выключает очистку. Замены и словари работают независимо от неё.");

const CLEAN_GROUPS = (): CleanGroup[] => ([
  { id: "glitches", keys: ["remove_hallucinations", "collapse_phrase_loops"], title: t("Убирать сбои распознавания"), hint: t("«субтитры сделал…», [Music], зациклившиеся фразы; если кроме них ничего нет — вставка отменяется") },
  { id: "disfluencies", keys: ["remove_fillers", "remove_duplicates"], title: t("Убирать запинки и повторы"), hint: t("э-э, ммм, «я я хочу» → «я хочу»; английские uh, umm — при английской диктовке") },
  { id: "parasites", keys: ["remove_parasites"], title: t("Удалять слова-паразиты"), hint: t("встроенный список только русский; свои слова работают на любом языке") },
  { id: "spelling", keys: ["correct_spelling"], title: t("Исправлять опечатки"), hint: t("при русском языке диктовки: однозначные исправления по встроенному словарю, без LLM") },
  { id: "ellipsis", keys: ["normalize_ellipsis"], title: t("Убирать многоточия пауз"), hint: t("внутри фразы — пробел, на границе предложения — точка; код, ссылки и числовые диапазоны сохраняются") },
  { id: "paragraphs", keys: ["split_paragraphs"], title: t("Разбивать на абзацы"), hint: t("группировать готовые предложения без изменения слов; сохранять существующие переносы") },
  { id: "split", keys: ["split_sentences"], title: t("Разбивать длинные фразы"), hint: t("мягкое разделение длинных фраз по связкам") },
]);

const groupIsOn = (formatting: TextFormattingConfig, group: CleanGroup) => group.keys.some((key) => Boolean(formatting[key]));

type SentenceStyle = "letter" | "no_dot" | "chat";

// The segments show the result itself rather than its name; the name is the
// hint and the accessible label. Latin «Aa» reads the same in both locales.
const SENTENCE_STYLES = (): Array<{ value: SentenceStyle; label: string; hint: string }> => ([
  { value: "letter", label: "Aa.", hint: t("Заглавные и точка") },
  { value: "no_dot", label: "Aa", hint: t("Без точки в конце") },
  { value: "chat", label: "aa", hint: t("Как в чате: строчные, без точки") },
]);

const SENTENCE_STYLE_FLAGS: Record<SentenceStyle, Pick<TextFormattingConfig, "capitalize_sentences" | "final_punctuation">> = {
  letter: { capitalize_sentences: true, final_punctuation: true },
  no_dot: { capitalize_sentences: true, final_punctuation: false },
  chat: { capitalize_sentences: false, final_punctuation: false },
};

/** Lowercase with a final dot is not offered — nobody writes like that on
 * purpose. A config that holds it shows as «Как в чате», and the next choice
 * writes both flags. */
function sentenceStyle(formatting: TextFormattingConfig): SentenceStyle {
  if (!formatting.capitalize_sentences) return "chat";
  return formatting.final_punctuation ? "letter" : "no_dot";
}

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
export function TextPage({ config, onConfigChanged, previewDraft, onPreviewDraftChange }: { config: ConfigResult | null; onConfigChanged: (partial: Partial<ConfigResult>, onError?: (message: string) => void) => Promise<ConfigResult | null>; previewDraft: string | null; onPreviewDraftChange: (text: string) => void }) {
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
  // At most one rule shows its matching options at a time.
  const [ruleOptionsId, setRuleOptionsId] = useState<string | null>(null);
  const findRef = useRef<HTMLInputElement>(null);

  // ── A shared preview of the whole local pass ───────────────────────────
  const previewText = textPreview(previewDraft);
  const [previewResult, setPreviewResult] = useState("");
  const [previewMatches, setPreviewMatches] = useState<PreviewReplacementsResult["matched_rules"]>([]);
  const [dictionaryMatches, setDictionaryMatches] = useState<DictionaryMatch[]>([]);
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
          setDictionaryMatches(format.dictionary_matches ?? []);
          setPreviewError(null);
        })
        .catch((e) => {
          if (cancelled) return;
          setPreviewResult("");
          setPreviewMatches([]);
          setDictionaryMatches([]);
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
  const cleanGroups = CLEAN_GROUPS();
  const cleanRowStyle = { padding: "9px 12px", borderBottom: "1px solid var(--line-soft)", display: "flex", alignItems: "center", gap: 10, opacity: formatting.enabled ? 1 : 0.55 } as const;
  const renderGroup = (group: CleanGroup) => (
    <div key={group.id} style={cleanRowStyle}>
      <div className="flex-grow" style={{ minWidth: 0 }}>
        <div style={{ font: "500 13px/1.2 var(--font-sans)", color: "var(--ink)", display: "inline-flex", alignItems: "center", gap: 5 }}>{group.title}<Hint text={group.hint}/></div>
      </div>
      {/* The word list opens from the row it belongs to. The summary keeps
          the point of showing it at all: how many words the step removes,
          and how many you stopped. */}
      {group.id === "parasites" && parasiteSets.length > 0 &&
        <button type="button" className="btn btn--ghost btn--sm" aria-label={`${t("Список")}: ${parasiteSummary}`} onClick={() => setParasitesOpen(true)}>
          {parasiteSummary}<Icon name="chev-right" size={11}/>
        </button>}
      {/* A group that disagrees with itself — one flag on, one off, from a
          config saved before the groups — shows as on; switching it writes
          every flag the same. */}
      <Switch label={group.title} on={groupIsOn(formatting, group)} onChange={(next) => void saveFormatting(Object.fromEntries(group.keys.map((key) => [key, next])))}/>
    </div>
  );
  // The backend does not apply paused replacements in the full pass, while the
  // preview of that single stage always applies them — otherwise it would be
  // useless exactly when the rules are being set up. The discrepancy is
  // labelled.
  const matchesAreHypothetical = paused;
  const showPreviewDiff = Boolean(previewText.trim() && previewResult);
  // A search box over three rules is one more field to read past; it appears
  // once the list is long enough to need it.
  const showRuleSearch = rules.length > 6 || Boolean(filter);

  return (
    <div className="page">
      <input type="file" accept=".json,application/json" ref={fileInputRef} style={{ display: "none" }} onChange={(e) => { const file = e.target.files?.[0]; if (file) void importRules(file); }}/>
      {/* There are no state pills in the header: both duplicated what is
          visible next to the blocks themselves — «не сохранено» lights up
          right on «Замены». A permanent green "saved" pill across the whole
          screen reported only that nothing had happened. */}
      <PageHeader title={t("Текст")}/>

      <div className="text-grid">
        <div className="flex-col" style={{ gap: 12, minWidth: 0 }}>
          {/* No «7/8» counter beside the title: it restated the switches below
              and still left you to find which one was off. */}
          <Foldable
            open={Boolean(folds.clean)}
            onToggle={() => toggleFold("clean")}
            title={t("Очистка")}
            hint={t("Удаляет из распознанного текста слова-паразиты, повторы и лишние пробелы, исправляет пунктуацию. Применяются только включённые правила.")}
            aside={<Hint text={MASTER_HINT()}><Switch on={formatting.enabled} onChange={(next) => void saveFormatting({ enabled: next })}/></Hint>}
          >
            <div className="fold__rows">
              {cleanGroups.map(renderGroup)}
              {/* One choice for two flags: of their four combinations only
                  three are ever wanted, and a messenger reply without a final
                  dot is the one two plain switches made awkward to find. */}
              <div style={{ ...cleanRowStyle, borderBottom: "none" }}>
                <div className="flex-grow" style={{ font: "500 13px/1.2 var(--font-sans)", color: "var(--ink)" }}>{t("Предложения")}</div>
                <Segmented
                  value={sentenceStyle(formatting)}
                  options={SENTENCE_STYLES()}
                  onChange={(next) => void saveFormatting(SENTENCE_STYLE_FLAGS[next as SentenceStyle])}
                />
              </div>
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
            hint={t("Точное правило: меняет именно то, что вы указали, — в том числе обычные слова. Правило всегда важнее словаря.")}
            summary={!saved ? <span className="pill warn">{t("не сохранено")}</span> : undefined}
            aside={<Hint text={paused ? t("Замены на паузе") : t("Замены применяются")}><Switch on={!paused} onChange={(next) => void setPaused(!next)}/></Hint>}
          >
            {showRuleSearch && <div style={{ padding: "8px 12px", borderBottom: "1px solid var(--line-soft)" }}>
              <div className="input-search">
                <Icon name="search" size={13} className="input-search__icon"/>
                <input className="field" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={t("Найти правило")} style={{ height: 30 }}/>
              </div>
            </div>}
            {rules.length === 0 ? <ReplacementsEmptyState/> : visibleRules.map((rule, index) => {
              const optionsOpen = ruleOptionsId === rule.id;
              return (
                // One line per rule. Matching mode, case and order are set once
                // and rarely revisited, so they wait behind «⋯»; a mode other
                // than the default stays visible as a pill, or a regex rule
                // would read as a plain word.
                <div key={rule.id} style={{ padding: "7px 12px", borderBottom: "1px solid var(--line-soft)", opacity: rule.enabled ? 1 : 0.66 }}>
                  <div className="flex-row" style={{ gap: 8 }}>
                    <Switch label={rule.find || t("Правило замены")} on={rule.enabled} onChange={(enabled) => updateRule(rule.id, { enabled })}/>
                    <input ref={index === 0 ? findRef : undefined} className="field mono" value={rule.find} onChange={(e) => updateRule(rule.id, { find: e.target.value })} placeholder={t("что искать")} aria-label={t("что искать")} style={{ height: 28, flex: "1 1 0", minWidth: 0 }}/>
                    <Icon name="arrow-right" size={12} style={{ color: "var(--ink-mute)", flexShrink: 0 }}/>
                    {/* An empty replacement deletes the match. The placeholder
                        says so in exactly the case it applies — the field is
                        empty — instead of a warning pill on the row. */}
                    <input className="field mono" value={rule.replace} onChange={(e) => updateRule(rule.id, { replace: e.target.value })} placeholder={t("пусто = удалить")} aria-label={t("на что заменить")} style={{ height: 28, flex: "1 1 0", minWidth: 0 }}/>
                    {rule.match !== "word" && <span className="pill mono">{MATCH_LABELS()[rule.match]}</span>}
                    <RowMenu label={t("Действия с правилом")} items={[
                      { id: "options", label: optionsOpen ? t("Скрыть параметры") : t("Параметры"), icon: "sliders", onSelect: () => setRuleOptionsId(optionsOpen ? null : rule.id) },
                      { id: "up", label: t("Поднять"), icon: "chev-up", disabled: index === 0, onSelect: () => moveRule(rule.id, -1) },
                      { id: "down", label: t("Опустить"), icon: "chev-down", disabled: index === visibleRules.length - 1, onSelect: () => moveRule(rule.id, 1) },
                      { id: "delete", label: t("Удалить"), icon: "trash", danger: true, onSelect: () => void deleteRule(rule.id) },
                    ]}/>
                  </div>
                  {optionsOpen && <div className="flex-row" style={{ flexWrap: "wrap", gap: 8, marginTop: 8 }}>
                    <div style={{ width: 130 }}><CustomSelect<string> value={rule.match} options={Object.entries(MATCH_LABELS()).map<SelectOption<string>>(([value, label]) => ({ value, label }))} onChange={(next) => updateRule(rule.id, { match: next as ReplacementMatchMode })}/></div>
                    <label className="pill" style={{ justifyContent: "center", cursor: "pointer" }}><input className="checkbox" type="checkbox" checked={rule.case_sensitive} onChange={(e) => updateRule(rule.id, { case_sensitive: e.target.checked })}/> Aa</label>
                    <label className="pill" style={{ justifyContent: "center", cursor: "pointer" }}><input className="checkbox" type="checkbox" checked={rule.preserve_case} onChange={(e) => updateRule(rule.id, { preserve_case: e.target.checked })}/>  {t("Регистр")}</label>
                    <span className="pill mono">{rule.usage_count || 0}  {t("сраб.")}</span>
                  </div>}
                </div>
              );
            })}
            {formError && <div role="alert" style={{ padding: "10px 12px", color: "var(--err)", font: "500 12px/1.4 var(--font-sans)", borderBottom: "1px solid var(--line-soft)" }}>{formError}</div>}
            {/* Export and import are one-off operations, so they share a «⋯»
                rather than standing beside the frequent ones. «Сохранить»
                appears only when there is something to save. */}
            <div className="flex-row" style={{ gap: 6, padding: "6px 12px" }}>
              <button type="button" className="btn btn--ghost btn--sm" onClick={() => addRule()} disabled={savingRules}><Icon name="plus" size={12}/>{t("Добавить правило")}</button>
              <div className="flex-grow"/>
              <RowMenu label={t("Экспорт и импорт правил")} items={[
                { id: "export", label: t("Экспорт"), icon: "download", disabled: rules.length === 0, onSelect: exportRules },
                { id: "import", label: t("Импорт"), icon: "folder", onSelect: () => fileInputRef.current?.click() },
              ]}/>
              {!saved && <button type="button" className="btn btn--primary btn--sm" onClick={() => void saveRules()} disabled={savingRules}><Icon name="check" size={12}/>{savingRules ? t("Сохраняю") : t("Сохранить")}</button>}
            </div>
          </Foldable>

          <DictionaryLibrary open={Boolean(folds.dict)} onToggle={() => toggleFold("dict")} formatting={formatting} onSave={async (patch, onError) => Boolean(await onConfigChanged({ text_formatting: patch as TextFormattingConfig }, onError))}/>
        </div>

        {/* One card for the whole preview: the text you type under «До», and
            the same text with the changes marked under «После». The arrow and
            the «Diff» caption of the former three cards are gone; the two
            step labels stay, because they say what the right column is for. */}
        <div className="flex-col" style={{ gap: 12, minWidth: 0 }}>
          <Card>
            <h2 className="preview-card__title">{t("Предпросмотр")}</h2>
            <div className="preview-card__step">{t("До")}</div>
            <textarea className="field mono" value={previewText} onChange={(e) => onPreviewDraftChange(e.target.value)} placeholder={t("Введите текст для проверки обработки")} aria-label={t("Введите текст для проверки обработки")} style={{ width: "100%", minHeight: 110, padding: 12, resize: "vertical", lineHeight: 1.55 }}/>
            <div className="preview-card__step preview-card__step--after">{t("После")}</div>
            <div className="preview-result">
              {previewError
                ? <span style={{ color: "var(--err)" }}>{previewError}</span>
                : showPreviewDiff
                  ? <DiffText before={previewText} after={previewResult}/>
                  : <span style={{ color: "var(--ink-mute)" }}>{t("Введите текст для предпросмотра")}</span>}
            </div>
            {/* Only what fired, and from which block: a replacement rule and a
                dictionary term can both change a word, and seeing which one
                did is how the difference between them becomes clear. */}
            {!previewError && previewMatches?.length ? <div className="preview-matches">
              <span className="preview-matches__label">{t("Замены")}</span>
              {previewMatches.map((item) => <span className={matchesAreHypothetical ? "pill" : "pill ok"} key={`${item.id}-${item.find}`}>{item.find}: {item.count}</span>)}
            </div> : null}
            {!previewError && dictionaryMatches.length ? <div className="preview-matches">
              <span className="preview-matches__label">{t("Словари")}</span>
              {dictionaryMatches.map((item) => <span className="pill" key={`${item.from}-${item.to}`}>{item.from} → {item.to}{item.count > 1 ? ` ×${item.count}` : ""}</span>)}
            </div> : null}
            {!previewError && matchesAreHypothetical && previewMatches?.length ? <div style={{ marginTop: 6, font: "400 11px/1.4 var(--font-sans)", color: "var(--warn)" }}>{t("Замены на паузе: правила совпали бы, но в результат выше не попали.")}</div> : null}
          </Card>
          {/* The examples stay in sight rather than inside «Замены»: one click
              shows a rule working in the preview above, which is how the
              feature explains itself. */}
          <Card pad="rows">
            <div style={{ font: "600 13px/1.2 var(--font-sans)", color: "var(--ink)", display: "inline-flex", alignItems: "center", gap: 5, marginBottom: 8 }}>
              {t("Добавить правило-пример")}
              <Hint text={t("Нажмите, чтобы создать правило — оно сразу попадёт в список слева и в предпросмотр.")}/>
            </div>
            <div className="flex-row" style={{ flexWrap: "wrap", gap: 6 }}>{replacementExamples().map(([find, replace]) => <button key={find} type="button" className="btn btn--ghost btn--sm" onClick={() => addRule(find, replace)}><span className="mono">{find}</span><Icon name="arrow-right" size={11}/><span className="mono">{replace}</span></button>)}</div>
          </Card>
        </div>
      </div>
    </div>
  );
}
