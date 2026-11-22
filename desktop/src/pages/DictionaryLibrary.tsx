import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { DictionaryAnalysis, DictionaryMatch, DictionarySet, TermMatching, TextFormattingConfig } from "../bridge/types";
import { analyzeDictionary, checkDictionaryText, exportDictionarySet, getDictionaryPresets, getTermMatching } from "../bridge/dictionaries";
import { Switch } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Foldable } from "../components/Foldable";
import { Modal } from "../components/Modal";
import { t, tPlural } from "../i18n";
import { dictionaryName, dictionaryPatch, parseDictionaryWords, replaceDictionarySet } from "./dictionarySets";
import { dictionaryImportErrorMessage, mergeDictionaryImport, readDictionaryFile, serializeDictionarySet, type DictionaryImport } from "./dictionaryImport";

type Entry = DictionarySet & { builtin?: boolean };
type Session = { entry?: Entry; draft?: TextFormattingConfig; create?: boolean; imported?: DictionaryImport; baseline: string };
type Save = (patch: Partial<TextFormattingConfig>, onError?: (message: string) => void) => Promise<boolean>;
type ImportSummary = { total: number; added: number; duplicates: number };

const signature = (formatting: TextFormattingConfig) => JSON.stringify(dictionaryPatch(formatting));
const termCount = (count: number) => `${count} ${tPlural(count, ["термин", "термина", "терминов"])}`;
const TERMS_PER_PAGE = 100;

function ImportNotice({ summary, newSet }: { summary: ImportSummary; newSet: boolean }) {
  return <div className="dictionary-body" role="status">
    <p className="dictionary-note">{t("Прочитано терминов: {total}. Добавлено: {added}. Пропущено повторов: {duplicates}.", summary)}</p>
    <p className="dictionary-note">{newSet ? t("Проверьте содержимое и нажмите «Сохранить». Новый набор будет выключен.") : t("Термины добавлены в черновик. Проверьте содержимое и нажмите «Сохранить».")}</p>
  </div>;
}

function presetLabel(id: string): { name: string; description: string } {
  switch (id) {
    case "development": return { name: t("Разработка"), description: t("Термины разработки: инструменты, языки и рабочие процессы.") };
    case "ai_voice": return { name: t("ИИ и голос"), description: t("Компании, модели, ассистенты и сервисы распознавания речи.") };
    case "work_apps": return { name: t("Приложения и работа"), description: t("Приложения для общения, документов, дизайна и совместной работы.") };
    default: return { name: id, description: "" };
  }
}

/** The fold header's one line: how many terms the enabled sets actually apply. */
function librarySummary(analysis: DictionaryAnalysis | null, error: boolean): string {
  if (!analysis) return error ? t("Ошибка") : t("Загрузка…");
  if (analysis.effective_count === 0) return t("Нет активных");
  return `${analysis.effective_count} ${tPlural(analysis.effective_count, ["активный термин", "активных термина", "активных терминов"])}`;
}

export function DictionaryLibrary({ open, onToggle, formatting, onSave }: { open: boolean; onToggle: () => void; formatting: TextFormattingConfig; onSave: Save }) {
  const [presets, setPresets] = useState<Entry[]>([]);
  const [analysis, setAnalysis] = useState<DictionaryAnalysis | null>(null);
  const [error, setError] = useState(false);
  const [revision, setRevision] = useState(0);
  const [session, setSession] = useState<Session | null>(null);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const createRef = useRef<HTMLButtonElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const readId = useRef(0);
  const [reading, setReading] = useState(false);
  const [importError, setImportError] = useState("");
  const configKey = signature(formatting);

  useEffect(() => () => { readId.current += 1; }, []);

  async function importSet(file: File) {
    const request = ++readId.current;
    const baseline = configKey;
    setReading(true); setImportError("");
    try {
      const imported = await readDictionaryFile(file);
      if (request !== readId.current) return;
      setSession({ create: true, imported, baseline, entry: { id: crypto.randomUUID(), name: imported.name, description: imported.description, words: imported.words, enabled: false } });
    } catch (error) {
      if (request === readId.current) setImportError(dictionaryImportErrorMessage(error));
    } finally {
      if (request === readId.current) setReading(false);
    }
  }

  useEffect(() => {
    let cancelled = false;
    setError(false);
    setAnalysis(null);
    Promise.all([getDictionaryPresets(), analyzeDictionary(formatting)]).then(([sets, next]) => {
      if (cancelled) return;
      setPresets(sets.map(([id, words]) => ({ id, words, name: id, description: "",
        enabled: false, builtin: true })));
      setAnalysis(next);
    }).catch(() => { if (!cancelled) setError(true); });
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed by the settings' signature; the object is new on every render.
  }, [configKey, revision]);

  const entries: Entry[] = [...(formatting.dictionary_sets ?? []), ...presets.map((set) => ({ ...set,
    ...presetLabel(set.id),
    enabled: formatting.enabled_presets.includes(set.id) }))];

  async function save(next: TextFormattingConfig, onError?: (message: string) => void): Promise<boolean> {
    return onSave(dictionaryPatch(next), onError);
  }

  async function toggle(entry: Entry) {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(false);
    const next = entry.builtin ? { ...formatting, enabled_presets: entry.enabled
      ? formatting.enabled_presets.filter((id) => id !== entry.id) : [...formatting.enabled_presets, entry.id] }
      : replaceDictionarySet(formatting, { ...entry, enabled: !entry.enabled });
    try {
      const result = await analyzeDictionary(next);
      if (result.conflicts.some((conflict) => !conflict.selected)) {
        setSession({ draft: next, baseline: configKey });
      } else if (!await save(next)) setError(true);
    } catch { setError(true); }
    finally { busyRef.current = false; setBusy(false); }
  }

  return <>
    <input ref={fileRef} type="file" accept=".txt,.json,text/plain,application/json" hidden aria-label={t("Файл словаря")} onChange={(event) => { const file = event.target.files?.[0]; event.target.value = ""; if (file) void importSet(file); }}/>
    <Foldable open={open} onToggle={onToggle} title={t("Словари")}
      hint={t("Как пишется термин: подсказывает Whisper и Qwen3 при распознавании и исправляет похожие искажения после него. Обычные слова не трогает — для них нужна замена.")}
      summary={<span className="dictionary-note" role="status">{librarySummary(analysis, error)}</span>}
      aside={<>
        <button type="button" className="btn btn--ghost btn--sm" disabled={busy || reading || !analysis} onClick={() => fileRef.current?.click()}><Icon name="folder" size={12}/>{reading ? t("Чтение…") : t("Импорт")}</button>
        <button ref={createRef} type="button" className="btn btn--ghost btn--sm" disabled={busy || reading || !analysis} onClick={() => setSession({ create: true, baseline: configKey })}><Icon name="plus" size={12}/>{t("Создать")}</button>
      </>}>
      <div className="dictionary-library">
        {error && <div className="inline-error" role="alert">{t("Не удалось загрузить или сохранить словари. Изменения не применены.")} <button type="button" className="btn btn--ghost" onClick={() => setRevision((value) => value + 1)}>{t("Повторить")}</button></div>}
        {analysis && entries.length === 0 && <p className="dictionary-note">{t("Наборов пока нет. Создайте набор для своих имён и терминов.")}</p>}
        <fieldset className="dictionary-fieldset" disabled={busy || reading}>
          {entries.map((entry) => <div className="dictionary-row" key={`${entry.builtin ? "builtin" : "user"}:${entry.id}`}>
            <button type="button" className="dictionary-open" onClick={() => setSession({ entry, baseline: configKey })}>
              <span className="dictionary-name">{dictionaryName(entry)}</span>
              <span className="dictionary-note">{entry.builtin ? t("Встроенный") : t("Пользовательский")} · {termCount(entry.words.length)}</span>
            </button>
            <Switch on={entry.enabled} label={`${dictionaryName(entry)}: ${entry.enabled ? t("Включён") : t("Выключен")}`} onChange={() => void toggle(entry)}/>
          </div>)}
        </fieldset>
      </div>
    </Foldable>
    {importError && <p className="inline-error" role="alert">{importError}</p>}
    {session && <DictionaryDialog key={session.entry?.id ?? "new"} session={session} formatting={formatting}
      onSave={save} onClose={() => { setSession(null); requestAnimationFrame(() => { if (!document.activeElement || document.activeElement === document.body) createRef.current?.focus(); }); }}/ >}
  </>;
}

/** A set's terms grouped by how they fire. «По звучанию» on every row of one
 * long column said nothing concrete; a group says it once, with an example,
 * and the check field above lets you try a spelling for real. */
function TermGroups({ words, matching }: { words: string[]; matching: Map<string, TermMatching> | null }) {
  // Until the backend answers — or if it fails — the terms show ungrouped
  // rather than in a group that might be wrong.
  if (!matching) return <div className="dictionary-chips">{words.map((word, index) => <span key={`${index}:${word}`} className="pill mono">{word}</span>)}</div>;
  const info = (word: string) => matching.get(word.trim());
  const exact = words.filter((word) => info(word)?.variants.length);
  const sound = words.filter((word) => !info(word)?.variants.length && info(word)?.fuzzy);
  const hint = words.filter((word) => !info(word)?.variants.length && !info(word)?.fuzzy);
  return <>
    {exact.length > 0 && <section className="dictionary-group">
      <h3 className="dictionary-group__title">{t("Точные написания")} <span className="dictionary-note">{exact.length}</span></h3>
      <p className="dictionary-note">{t("Меняются только эти варианты, слово в слово.")}</p>
      <div className="dictionary-variants">
        {exact.map((word, index) => <div key={`${index}:${word}`} className="dictionary-variants__row">
          <span className="mono">{word}</span>
          <span className="dictionary-note">{info(word)!.variants.join(", ")}{info(word)!.fuzzy ? ` · ${t("и по звучанию")}` : ""}</span>
        </div>)}
      </div>
    </section>}
    {sound.length > 0 && <section className="dictionary-group">
      <h3 className="dictionary-group__title">{t("По похожему звучанию")} <span className="dictionary-note">{sound.length}</span></h3>
      <p className="dictionary-note">{t("Исправляется, если распознанное звучит почти так же: «пул реквест» → pull request. Обычные русские слова не трогаются.")}</p>
      <div className="dictionary-chips">{sound.map((word, index) => <span key={`${index}:${word}`} className="pill mono">{word}</span>)}</div>
    </section>}
    {hint.length > 0 && <section className="dictionary-group">
      <h3 className="dictionary-group__title">{t("Только подсказка модели")} <span className="dictionary-note">{hint.length}</span></h3>
      <p className="dictionary-note">{t("Текст не исправляется: Whisper и Qwen3 просто чаще слышат правильно.")}</p>
      <div className="dictionary-chips">{hint.map((word, index) => <span key={`${index}:${word}`} className="pill mono dictionary-chip--hint">{word}</span>)}</div>
    </section>}
  </>;
}

/** The answer of the check field: what the set turned the spelling into, and
 * by which route — or that it leaves it alone. */
function CheckResult({ probe, result, matching }: { probe: string; result: DictionaryMatch[] | null; matching: Map<string, TermMatching> | null }) {
  if (!probe.trim()) return <p className="dictionary-note">{t("Введите, как модель могла услышать слово, — увидите, во что его превратит этот набор.")}</p>;
  if (!result) return null;
  if (!result.length) return <p className="dictionary-note">{t("Не меняется")}</p>;
  return <p className="dictionary-check__result">{result.map((item) => {
    const exact = matching?.get(item.to)?.variants.some((variant) => variant.toLowerCase() === item.from.toLowerCase());
    return <span key={`${item.from}-${item.to}`}>
      <span className="mono">{item.from}</span> → <span className="mono dictionary-check__to">{item.to}</span>
      <span className="pill">{exact ? t("точное написание") : t("по звучанию")}</span>
    </span>;
  })}</p>;
}

function DictionaryDialog({ session, formatting, onSave, onClose }: { session: Session; formatting: TextFormattingConfig; onSave: (next: TextFormattingConfig, onError?: (message: string) => void) => Promise<boolean>; onClose: () => void }) {
  const [entry, setEntry] = useState<Entry>(() => session.entry ?? { id: crypto.randomUUID(), name: "", description: "", words: [], enabled: false });
  const [editing, setEditing] = useState(Boolean(session.create));
  const [words, setWords] = useState(entry.words.join("\n"));
  const [probe, setProbe] = useState("");
  const [probeResult, setProbeResult] = useState<DictionaryMatch[] | null>(null);
  const [choices, setChoices] = useState<string[]>(session.draft?.dictionary_spellings ?? formatting.dictionary_spellings ?? []);
  const [analysis, setAnalysis] = useState<DictionaryAnalysis | null>(null);
  const [analysisKey, setAnalysisKey] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
  const [deleting, setDeleting] = useState(false);
  const [discarding, setDiscarding] = useState(false);
  const [importSummary, setImportSummary] = useState<ImportSummary | null>(() => session.imported ? { total: session.imported.total, added: session.imported.words.length, duplicates: session.imported.duplicates } : null);
  const [reading, setReading] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [fileError, setFileError] = useState("");
  const [exported, setExported] = useState(false);
  const [termFilter, setTermFilter] = useState("");
  const [termPage, setTermPage] = useState(0);
  const fileRef = useRef<HTMLInputElement>(null);
  const readId = useRef(0);
  const exportBusyRef = useRef(false);
  const wordsRef = useRef(words);
  wordsRef.current = words;
  const parsedWords = useMemo(() => parseDictionaryWords(words), [words]);
  const confirmationRef = useRef<HTMLButtonElement>(null);
  const returnRef = useRef<HTMLButtonElement>(null);
  const wasConfirming = useRef(false);
  useLayoutEffect(() => {
    if (discarding || deleting) confirmationRef.current?.focus();
    else if (wasConfirming.current) returnRef.current?.focus();
    wasConfirming.current = discarding || deleting;
  }, [discarding, deleting]);
  const busyRef = useRef(false);
  const baseline = useRef(session.baseline);
  const initialDraft = useRef(JSON.stringify({ entry, words }));
  const currentConfigKey = signature(formatting);
  const latestConfigKey = useRef(currentConfigKey);
  latestConfigKey.current = currentConfigKey;
  const stale = currentConfigKey !== baseline.current;
  const base = session.draft ?? formatting;
  const candidate: TextFormattingConfig = { ...(deleting ? { ...base, dictionary_sets: (base.dictionary_sets ?? []).filter((set) => set.id !== entry.id) } : editing ? replaceDictionarySet(base, { ...entry, name: entry.name.trim(), description: entry.description.trim(), words: parsedWords }) : base), dictionary_spellings: choices };
  const candidateKey = signature(candidate);
  const checking = analysisKey !== candidateKey;
  const dirty = editing && (Boolean(session.imported) || JSON.stringify({ entry, words }) !== initialDraft.current);

  useEffect(() => () => { readId.current += 1; }, []);

  async function importWords(file: File) {
    const request = ++readId.current;
    setReading(true); setFileError(""); setExported(false);
    try {
      const imported = await readDictionaryFile(file);
      if (request !== readId.current) return;
      const merged = mergeDictionaryImport(parseDictionaryWords(wordsRef.current), imported);
      setWords(merged.words.join("\n"));
      setImportSummary({ total: imported.total, added: merged.added, duplicates: merged.duplicates });
    } catch (error) {
      if (request === readId.current) setFileError(dictionaryImportErrorMessage(error));
    } finally {
      if (request === readId.current) setReading(false);
    }
  }

  async function exportSet() {
    if (exportBusyRef.current || busyRef.current) return;
    exportBusyRef.current = true;
    setExporting(true); setFileError(""); setExported(false);
    try {
      setExported(await exportDictionarySet(serializeDictionarySet({ ...entry, name: dictionaryName(entry) })));
    } catch {
      setFileError(t("Не удалось экспортировать набор. Проверьте права на запись и свободное место."));
    } finally { exportBusyRef.current = false; setExporting(false); }
  }

  useEffect(() => {
    let cancelled = false;
    setAnalysisKey("");
    const timer = window.setTimeout(() => {
      analyzeDictionary(candidate).then((result) => { if (!cancelled) { setAnalysis(result); setAnalysisKey(candidateKey); setError(""); } })
        .catch(() => { if (!cancelled) setError(t("Не удалось проверить набор. Повторите попытку.")); });
    }, 180);
    return () => { cancelled = true; window.clearTimeout(timer); };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed by the candidate's signature; the object is new on every render.
  }, [candidateKey, revision]);

  function close() { if (discarding) { setDiscarding(false); return; } if (dirty) setDiscarding(true); else onClose(); }

  async function persist(next: TextFormattingConfig) {
    if (busyRef.current || stale) return;
    busyRef.current = true; setBusy(true); setError("");
    try {
      const checked = await analyzeDictionary(next);
      if (latestConfigKey.current !== baseline.current) return;
      if (checked.conflicts.some((conflict) => !conflict.selected)) { setAnalysis(checked); setError(t("Выберите написание для каждого конфликта.")); return; }
      // A rejection such as a term limit is shown here: the modal covers the window banner.
      let reason = "";
      if (await onSave(next, (message) => { reason = message; })) onClose();
      else setError(reason ? t("Не удалось сохранить набор: {p0} Введённые данные сохранены в редакторе.", { p0: reason }) : t("Не удалось сохранить набор. Введённые данные сохранены в редакторе; попробуйте ещё раз."));
    } catch { setError(t("Не удалось сохранить набор. Введённые данные сохранены в редакторе; попробуйте ещё раз.")); }
    finally { busyRef.current = false; setBusy(false); }
  }

  function copy() {
    const next = { ...entry, id: crypto.randomUUID(), name: t("{name} — копия", { name: dictionaryName(entry) }), enabled: false, builtin: false };
    setEntry(next); setWords(next.words.join("\n")); setEditing(true); setProbe(""); setExported(false); setFileError("");
  }

  // What each term reacts to, for the read-only view. A failure leaves the
  // list bare rather than misdescribing it.
  const [matching, setMatching] = useState<Map<string, TermMatching> | null>(null);
  const viewedWords = !editing && !session.draft ? entry.words : null;
  useEffect(() => {
    if (!viewedWords?.length) return;
    let cancelled = false;
    getTermMatching(viewedWords, Boolean(entry.builtin))
      .then((items) => { if (!cancelled) setMatching(new Map(items.map((item) => [item.term, item]))); })
      .catch(() => {});
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed by the terms shown, not the array identity.
  }, [JSON.stringify(viewedWords), entry.builtin]);

  // The set runs on its own, enabled or not, so the answer is about this set
  // rather than whatever else happens to be switched on.
  useEffect(() => {
    setProbeResult(null);
    if (!viewedWords || !probe.trim()) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      checkDictionaryText(probe, viewedWords, Boolean(entry.builtin))
        .then((result) => { if (!cancelled) setProbeResult(result); })
        .catch(() => {});
    }, 180);
    return () => { cancelled = true; window.clearTimeout(timer); };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed by the terms shown, not the array identity.
  }, [probe, JSON.stringify(viewedWords), entry.builtin]);
  const selectedWords = new Set(editing ? parsedWords : entry.words);
  const unsupported = analysis?.unsupported_words.filter((word) => selectedWords.has(word)) ?? [];
  const filteredWords = useMemo(() => {
    const query = termFilter.trim().toLowerCase();
    return query ? entry.words.filter((word) => word.toLowerCase().includes(query)) : entry.words;
  }, [entry.words, termFilter]);
  const pageCount = Math.ceil(filteredWords.length / TERMS_PER_PAGE);
  const currentPage = Math.min(termPage, Math.max(0, pageCount - 1));
  const visibleWords = filteredWords.slice(currentPage * TERMS_PER_PAGE, (currentPage + 1) * TERMS_PER_PAGE);
  const title = session.draft ? t("Выберите написание") : editing ? t("Редактор набора") : dictionaryName(entry);

  // Three shells: the discard question is a narrow ask box with no header, the
  // read-only set scrolls its term list under a fixed heading, and the editor,
  // the draft picker and the delete confirmation keep the plain dialog.
  const viewing = !editing && !session.draft && !deleting;
  let shell = "dictionary-modal";
  if (discarding) shell += " modal--ask";
  else if (viewing) shell += " dictionary-modal--view";

  return <Modal title={discarding ? t("Закрыть без сохранения изменений?") : title} busy={busy || exporting} onClose={close} showHeader={!discarding} closeRef={editing ? returnRef : undefined} className={shell}>
    <div className="modal__body dictionary-body">
      {discarding ? <><p>{t("Закрыть без сохранения изменений?")}</p><div className="dictionary-toolbar dictionary-confirm-actions"><button ref={confirmationRef} type="button" className="btn btn--ghost btn--sm" onClick={() => setDiscarding(false)}>{t("Вернуться")}</button><button type="button" className="btn btn--ghost btn--sm" onClick={onClose}>{t("Не сохранять")}</button></div></> : <>
        {stale && <p className="inline-error" role="alert">{t("Словари изменились в другом окне. Скопируйте несохранённый текст и откройте набор заново.")}</p>}
        <fieldset className="dictionary-fieldset dictionary-body" disabled={busy || reading || exporting}>
          {editing ? <>
            <label className="dictionary-label">{t("Название набора")}<input autoFocus className="field" value={entry.name} onChange={(event) => setEntry({ ...entry, name: event.target.value })}/></label>
            <label className="dictionary-label">{t("Описание (необязательно)")}<input className="field" value={entry.description} onChange={(event) => setEntry({ ...entry, description: event.target.value })}/></label>
            <label className="dictionary-label"><span className="dictionary-term-heading"><span>{t("Термины")}</span><span className="dictionary-note" aria-hidden="true">{parsedWords.length}</span></span><textarea aria-label={t("Термины")} className="field mono dictionary-words" value={words} onChange={(event) => setWords(event.target.value)} placeholder={t("например: Tauri\nClaude Code")}/></label>
            <div className="dictionary-toolbar"><p className="dictionary-note">{t("По одному слову или фразе в строке. Запятые остаются частью термина.")}</p><button type="button" className="btn btn--ghost btn--sm" disabled={stale} onClick={() => fileRef.current?.click()}><Icon name="folder" size={12}/>{reading ? t("Чтение…") : t("Добавить из файла")}</button></div>
            <p className="dictionary-note">{t("TXT: один термин в строке, UTF-8. JSON: экспортированный набор версии 1. До 2 МиБ.")}</p>
            {importSummary && <ImportNotice summary={importSummary} newSet={Boolean(session.create)}/>}
            <p className="dictionary-note">{t("Термин — до 128 символов и 8 слов. Во всех активных пользовательских наборах — до 2000 разных терминов.")}</p>
          </> : !session.draft && <>
            <div className="dictionary-check">
              <label className="dictionary-label">{t("Проверить слово")}<input className="field" value={probe} onChange={(event) => setProbe(event.target.value)} placeholder={t("например: пул реквест")}/></label>
              <div aria-live="polite"><CheckResult probe={probe} result={probeResult} matching={matching}/></div>
            </div>
            <div className="dictionary-label"><div className="dictionary-term-heading"><span>{t("Термины")}</span><span className="dictionary-note">{entry.words.length}</span></div></div>
            {entry.words.length > 0 && <label className="dictionary-label">{t("Поиск по терминам")}<input className="field" value={termFilter} onChange={(event) => { setTermFilter(event.target.value); setTermPage(0); }}/></label>}
            {entry.words.length
              ? <div className="dictionary-term-list">{filteredWords.length ? <TermGroups words={visibleWords} matching={matching}/> : <p className="dictionary-note">{t("Термины не найдены")}</p>}</div>
              : <p className="dictionary-note">{t("В наборе пока нет терминов")}</p>}
            {filteredWords.length > TERMS_PER_PAGE && <div className="dictionary-toolbar" aria-label={t("Страницы терминов")}>
              <button type="button" className="btn btn--ghost btn--sm" disabled={currentPage === 0} onClick={() => setTermPage(currentPage - 1)}>{t("Назад")}</button>
              <span className="dictionary-note" role="status">{t("Показано {from}–{to} из {total}", { from: currentPage * TERMS_PER_PAGE + 1, to: Math.min((currentPage + 1) * TERMS_PER_PAGE, filteredWords.length), total: filteredWords.length })}</span>
              <button type="button" className="btn btn--ghost btn--sm" disabled={currentPage + 1 >= pageCount} onClick={() => setTermPage(currentPage + 1)}>{t("Далее")}</button>
            </div>}
          </>}
          {editing && unsupported.length > 0 && <p className="dictionary-note">{t("Эти термины слишком короткие для коррекции текста, но могут использоваться как подсказки распознавания:")} <span className="mono">{unsupported.join(", ")}</span></p>}
          {(editing || session.draft || deleting) && analysis?.conflicts.map((conflict) => <div className="dictionary-label" role="group" aria-label={t("Написание термина «{term}»", { term: conflict.key })} key={conflict.key}>
            <span>{t("Написание термина «{term}»", { term: conflict.key })}</span>
            <div className="dictionary-toolbar">{conflict.variants.map((word) => <button type="button" className={conflict.selected === word ? "btn btn--primary" : "btn btn--ghost"} key={word} aria-pressed={conflict.selected === word}
              onClick={() => setChoices([...choices.filter((choice) => choice.toLowerCase() !== conflict.key), word])}>{word}</button>)}</div>
          </div>)}
          {(editing || session.draft || deleting) && Boolean(analysis?.conflicts.length) && <p className="dictionary-note">{t("Выбранное написание используется во всех активных наборах. Регистр результата также зависит от исходного текста.")}</p>}
          {deleting && <div role="alert" className="dictionary-body"><p>{t("Удалить набор «{name}»? Его содержимое будет потеряно.", { name: dictionaryName(entry) })}</p><div className="dictionary-toolbar"><button ref={confirmationRef} type="button" className="btn btn--ghost" onClick={() => setDeleting(false)}>{t("Отмена")}</button><button type="button" className="btn btn--primary" disabled={stale || checking || !analysis || analysis.conflicts.some((conflict) => !conflict.selected)} onClick={() => void persist(candidate)}>{t("Удалить")}</button></div></div>}
        </fieldset>
        {editing && <input ref={fileRef} type="file" accept=".txt,.json,text/plain,application/json" hidden aria-label={t("Добавить термины из файла")} onChange={(event) => { const file = event.target.files?.[0]; event.target.value = ""; if (file) void importWords(file); }}/>}
        {fileError && <p className="inline-error" role="alert">{fileError}</p>}
        {exported && <p className="dictionary-note" role="status">{t("Набор экспортирован.")}</p>}
        {error && <div className="inline-error" role="alert">{error} {checking && <button type="button" className="btn btn--ghost" onClick={() => setRevision((value) => value + 1)}>{t("Повторить")}</button>}</div>}
      </>}
    </div>
    {!discarding && !deleting && <div className="modal__foot dictionary-actions">
      {editing || session.draft
        ? <button type="button" className="btn btn--primary" disabled={busy || reading || stale || checking || !analysis || (editing && !entry.name.trim()) || analysis.conflicts.some((conflict) => !conflict.selected)} onClick={() => void persist(candidate)}>{busy ? t("Сохранение…") : t("Сохранить")}</button>
        : <>
        {!entry.builtin && <button type="button" className="btn btn--primary" disabled={busy || exporting || stale} onClick={() => { setEntry({ ...entry, name: dictionaryName(entry) }); setEditing(true); setExported(false); }}>{t("Редактировать")}</button>}
        <button type="button" className="btn btn--ghost" disabled={busy || exporting || stale} onClick={copy}>{t("Создать копию")}</button>
        <button type="button" className="btn btn--ghost" disabled={busy || exporting} onClick={() => void exportSet()}><Icon name="download" size={12}/>{t("Экспорт")}</button>
        {!entry.builtin && <button ref={returnRef} type="button" className="btn btn--ghost" disabled={busy || exporting || stale} onClick={() => setDeleting(true)}>{t("Удалить")}</button>}
      </>}
    </div>}
  </Modal>;
}
