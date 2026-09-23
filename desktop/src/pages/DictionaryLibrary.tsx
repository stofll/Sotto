import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { DictionaryAnalysis, DictionarySet, TextFormattingConfig } from "../bridge/types";
import { analyzeDictionary, getDictionaryPresets } from "../bridge/dictionaries";
import { Switch } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Foldable } from "../components/Foldable";
import { Modal } from "../components/Modal";
import { t, tPlural } from "../i18n";
import { dictionaryName, dictionaryPatch, parseDictionaryWords, replaceDictionarySet } from "./dictionarySets";

type Entry = DictionarySet & { builtin?: boolean };
type Session = { entry?: Entry; draft?: TextFormattingConfig; create?: boolean; baseline: string };
type Save = (patch: Partial<TextFormattingConfig>) => Promise<boolean>;

const signature = (formatting: TextFormattingConfig) => JSON.stringify(dictionaryPatch(formatting));
const termCount = (count: number) => `${count} ${tPlural(count, ["термин", "термина", "терминов"])}`;

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
  const configKey = signature(formatting);

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
    name: set.id === "development" ? t("Разработка") : set.id,
    description: set.id === "development" ? t("Термины разработки: инструменты, языки и рабочие процессы.") : "",
    enabled: formatting.enabled_presets.includes(set.id) }))];

  async function save(next: TextFormattingConfig): Promise<boolean> {
    return onSave(dictionaryPatch(next));
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
    <Foldable open={open} onToggle={onToggle} title={t("Словари")}
      hint={t("Помогает исправлять похожие написания имён, брендов и терминов после распознавания. Активные наборы работают вместе; Whisper также использует их как подсказку. Это не точная замена текста.")}
      summary={<span className="dictionary-note" role="status">{librarySummary(analysis, error)}</span>}
      aside={<button ref={createRef} type="button" className="btn btn--ghost btn--sm" disabled={busy || !analysis} onClick={() => setSession({ create: true, baseline: configKey })}><Icon name="plus" size={12}/>{t("Создать")}</button>}>
      <div className="dictionary-library">
        {!formatting.enabled && <p className="dictionary-note">{t("Форматирование выключено: словари не исправляют текст. Подсказка Whisper продолжает работать.")}</p>}
        {error && <div className="dictionary-error" role="alert">{t("Не удалось загрузить или сохранить словари. Изменения не применены.")} <button type="button" className="btn btn--ghost" onClick={() => setRevision((value) => value + 1)}>{t("Повторить")}</button></div>}
        {analysis && entries.length === 0 && <p className="dictionary-note">{t("Наборов пока нет. Создайте набор для своих имён и терминов.")}</p>}
        <fieldset className="dictionary-fieldset" disabled={busy}>
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
    {session && <DictionaryDialog key={session.entry?.id ?? "new"} session={session} formatting={formatting}
      onSave={save} onClose={() => { setSession(null); requestAnimationFrame(() => { if (!document.activeElement || document.activeElement === document.body) createRef.current?.focus(); }); }}/ >}
  </>;
}

function DictionaryDialog({ session, formatting, onSave, onClose }: { session: Session; formatting: TextFormattingConfig; onSave: (next: TextFormattingConfig) => Promise<boolean>; onClose: () => void }) {
  const [entry, setEntry] = useState<Entry>(() => session.entry ?? { id: crypto.randomUUID(), name: "", description: "", words: [], enabled: false });
  const [editing, setEditing] = useState(Boolean(session.create));
  const [words, setWords] = useState(entry.words.join("\n"));
  const [filter, setFilter] = useState("");
  const [choices, setChoices] = useState<string[]>(session.draft?.dictionary_spellings ?? formatting.dictionary_spellings ?? []);
  const [analysis, setAnalysis] = useState<DictionaryAnalysis | null>(null);
  const [analysisKey, setAnalysisKey] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
  const [deleting, setDeleting] = useState(false);
  const [discarding, setDiscarding] = useState(false);
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
  const stale = signature(formatting) !== baseline.current;
  const base = session.draft ?? formatting;
  const candidate: TextFormattingConfig = { ...(deleting ? { ...base, dictionary_sets: (base.dictionary_sets ?? []).filter((set) => set.id !== entry.id) } : editing ? replaceDictionarySet(base, { ...entry, name: entry.name.trim(), description: entry.description.trim(), words: parseDictionaryWords(words) }) : base), dictionary_spellings: choices };
  const candidateKey = signature(candidate);
  const checking = analysisKey !== candidateKey;
  const dirty = editing && JSON.stringify({ entry, words }) !== initialDraft.current;

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
      if (checked.conflicts.some((conflict) => !conflict.selected)) { setAnalysis(checked); setError(t("Выберите написание для каждого конфликта.")); return; }
      if (await onSave(next)) onClose(); else setError(t("Не удалось сохранить набор. Введённые данные сохранены в редакторе; попробуйте ещё раз."));
    } catch { setError(t("Не удалось сохранить набор. Введённые данные сохранены в редакторе; попробуйте ещё раз.")); }
    finally { busyRef.current = false; setBusy(false); }
  }

  function copy() {
    const next = { ...entry, id: crypto.randomUUID(), name: t("{name} — копия", { name: dictionaryName(entry) }), enabled: false, builtin: false };
    setEntry(next); setWords(next.words.join("\n")); setEditing(true); setFilter("");
  }

  const shownWords = entry.words.filter((word) => word.toLowerCase().includes(filter.trim().toLowerCase()));
  const unsupported = analysis?.unsupported_words.filter((word) => (editing ? parseDictionaryWords(words) : entry.words).includes(word)) ?? [];
  const title = session.draft ? t("Выберите написание") : editing ? t("Редактор набора") : dictionaryName(entry);

  // Three shells: the discard question is a narrow ask box with no header, the
  // read-only set scrolls its term list under a fixed heading, and the editor,
  // the draft picker and the delete confirmation keep the plain dialog.
  const viewing = !editing && !session.draft && !deleting;
  let shell = "dictionary-modal";
  if (discarding) shell += " modal--ask";
  else if (viewing) shell += " dictionary-modal--view";

  return <Modal title={discarding ? t("Закрыть без сохранения изменений?") : title} busy={busy} onClose={close} showHeader={!discarding} closeRef={editing ? returnRef : undefined} className={shell}>
    <div className="modal__body dictionary-body">
      {discarding ? <><p>{t("Закрыть без сохранения изменений?")}</p><div className="dictionary-toolbar dictionary-confirm-actions"><button ref={confirmationRef} type="button" className="btn btn--ghost btn--sm" onClick={() => setDiscarding(false)}>{t("Вернуться")}</button><button type="button" className="btn btn--ghost btn--sm" onClick={onClose}>{t("Не сохранять")}</button></div></> : <>
        {stale && <p className="dictionary-error" role="alert">{t("Словари изменились в другом окне. Скопируйте несохранённый текст и откройте набор заново.")}</p>}
        <fieldset className="dictionary-fieldset dictionary-body" disabled={busy}>
          {editing ? <>
            <label className="dictionary-label">{t("Название набора")}<input autoFocus className="field" value={entry.name} onChange={(event) => setEntry({ ...entry, name: event.target.value })}/></label>
            <label className="dictionary-label">{t("Описание (необязательно)")}<input className="field" value={entry.description} onChange={(event) => setEntry({ ...entry, description: event.target.value })}/></label>
            <label className="dictionary-label"><span className="dictionary-term-heading"><span>{t("Термины")}</span><span className="dictionary-note" aria-hidden="true">{parseDictionaryWords(words).length}</span></span><textarea aria-label={t("Термины")} className="field mono dictionary-words" value={words} onChange={(event) => setWords(event.target.value)} placeholder={t("например: Tauri\nClaude Code")}/></label>
            <p className="dictionary-note">{t("По одному слову или фразе в строке. Также можно разделять запятыми.")}</p>
          </> : !session.draft && <>
            <label className="dictionary-label">{t("Поиск по набору")}<input className="field" value={filter} onChange={(event) => setFilter(event.target.value)}/></label>
            <div className="dictionary-label"><div className="dictionary-term-heading"><span>{t("Термины")}</span><span className="dictionary-note">{entry.words.length}</span></div></div>
            <ul className="dictionary-term-list">{shownWords.map((word, index) => <li key={`${index}:${word}`}>{word}</li>)}</ul>{!shownWords.length && <p className="dictionary-note">{entry.words.length ? t("Ничего не найдено") : t("В наборе пока нет терминов")}</p>}
          </>}
          {unsupported.length > 0 && <p className="dictionary-note">{t("Эти термины слишком короткие для коррекции текста, но остаются в подсказке Whisper:")} <span className="mono">{unsupported.join(", ")}</span></p>}
          {(editing || session.draft || deleting) && analysis?.conflicts.map((conflict) => <div className="dictionary-label" role="group" aria-label={t("Написание термина «{term}»", { term: conflict.key })} key={conflict.key}>
            <span>{t("Написание термина «{term}»", { term: conflict.key })}</span>
            <div className="dictionary-toolbar">{conflict.variants.map((word) => <button type="button" className={conflict.selected === word ? "btn btn--primary" : "btn btn--ghost"} key={word} aria-pressed={conflict.selected === word}
              onClick={() => setChoices([...choices.filter((choice) => choice.toLowerCase() !== conflict.key), word])}>{word}</button>)}</div>
          </div>)}
          {(editing || session.draft || deleting) && Boolean(analysis?.conflicts.length) && <p className="dictionary-note">{t("Выбранное написание используется во всех активных наборах. Регистр результата также зависит от исходного текста.")}</p>}
          {deleting && <div role="alert" className="dictionary-body"><p>{t("Удалить набор «{name}»? Его содержимое будет потеряно.", { name: dictionaryName(entry) })}</p><div className="dictionary-toolbar"><button ref={confirmationRef} type="button" className="btn btn--ghost" onClick={() => setDeleting(false)}>{t("Отмена")}</button><button type="button" className="btn btn--primary" disabled={stale || checking || !analysis || analysis.conflicts.some((conflict) => !conflict.selected)} onClick={() => void persist(candidate)}>{t("Удалить")}</button></div></div>}
        </fieldset>
        {error && <div className="dictionary-error" role="alert">{error} {checking && <button type="button" className="btn btn--ghost" onClick={() => setRevision((value) => value + 1)}>{t("Повторить")}</button>}</div>}
      </>}
    </div>
    {!discarding && !deleting && <div className="modal__foot dictionary-actions">
      {editing || session.draft
        ? <button type="button" className="btn btn--primary" disabled={busy || stale || checking || !analysis || (editing && !entry.name.trim()) || analysis.conflicts.some((conflict) => !conflict.selected)} onClick={() => void persist(candidate)}>{busy ? t("Сохранение…") : t("Сохранить")}</button>
        : <>
        {!entry.builtin && <button type="button" className="btn btn--primary" disabled={busy || stale} onClick={() => { setEntry({ ...entry, name: dictionaryName(entry) }); setEditing(true); }}>{t("Редактировать")}</button>}
        <button type="button" className="btn btn--ghost" disabled={busy || stale} onClick={copy}>{t("Создать копию")}</button>
        {!entry.builtin && <button ref={returnRef} type="button" className="btn btn--ghost" disabled={busy || stale} onClick={() => setDeleting(true)}>{t("Удалить")}</button>}
      </>}
    </div>}
  </Modal>;
}
