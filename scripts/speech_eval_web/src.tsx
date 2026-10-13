import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { Card, CardHead } from "../../desktop/src/components/Shell";
import { LOCALE_LABELS, setLocale, useLocale, type Locale } from "../../desktop/src/i18n";
import { t } from "./i18n";
import "../../desktop/src/styles.css";
import "./style.css";

type Answer = { choice: string; custom: string };
type Question = { id: string; prompt: string; source: string; options: { id: string; text: string; note?: string }[]; default: Answer };
type Review = { revision: number; mode: "complete" | "partial"; asr_text: string | null; answers: Record<string, Answer> };
type Clip = { id: string; ultra_text: string; audio_url: string; audio_path: string; duration_seconds: number;
  questions: Question[]; parts: ({ text: string } | { question_id: string })[]; review: Review | null };
type Batch = { id: string; title: string; records: Clip[] };
type State = { catalog_sha256: string; token: string; batches: Batch[] };
type Draft = { answers: Record<string, Answer>; override: string | null; revision: number };

function readPreference(key: string, fallback: string) {
  try { return localStorage.getItem(key) ?? fallback; } catch { return fallback; }
}

function answerText(question: Question, answer: Answer) {
  if (answer.choice === "custom") return answer.custom;
  if (answer.choice === "unclear") return t("[неразборчиво]");
  return question.options.find(option => option.id === answer.choice)?.text ?? question.source;
}

function questionTitle(prompt: string) {
  if (prompt === "verb") return t("Глагол");
  if (prompt === "first_term") return t("Первый термин");
  if (prompt === "second_term") return t("Второй термин");
  return prompt;
}

function parts(clip: Clip, draft: Draft) {
  return clip.parts.map(part => "text" in part ? { text: part.text, key: null } : {
    text: answerText(clip.questions.find(q => q.id === part.question_id)!, draft.answers[part.question_id]),
    key: part.question_id,
  });
}

function initialDraft(clip: Clip): Draft {
  const draft: Draft = { answers: Object.fromEntries(clip.questions.map(q => [q.id, clip.review?.answers[q.id] ?? q.default])),
    override: null, revision: clip.review?.revision ?? 0 };
  if (clip.review?.mode === "complete" && clip.review.asr_text !== parts(clip, draft).map(p => p.text).join("")) {
    draft.override = clip.review.asr_text;
  }
  return draft;
}

function App() {
  const locale = useLocale();
  const [theme, setTheme] = useState(readPreference("review-theme", "dark"));
  const [state, setState] = useState<State | null>(null);
  const [index, setIndex] = useState(0);
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const [loadError, setLoadError] = useState(false);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<"complete" | "partial" | "error" | null>(null);
  const [audioError, setAudioError] = useState(false);
  const audio = useRef<HTMLAudioElement>(null);
  const records = state?.batches.flatMap(batch => batch.records) ?? [];
  const clip = records[index];
  const draft = clip ? drafts[clip.id] ?? initialDraft(clip) : null;
  const transcript = clip && draft ? parts(clip, draft) : [];
  const assembled = transcript.map(part => part.text).join("");
  const complete = !!clip && !!draft && !/\[(неразборчиво|unclear)\]/i.test(draft.override ?? assembled) && clip.questions.every(q => {
    const answer = draft.answers[q.id];
    return answer.choice !== "unclear" && !!answerText(q, answer).trim() && !!answer.choice;
  });

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try { localStorage.setItem("review-theme", theme); } catch { /* Optional preference. */ }
  }, [theme]);

  useEffect(() => {
    void setLocale(readPreference("review-locale", "ru") === "en" ? "en" : "ru");
    const controller = new AbortController();
    fetch("/api/state", { signal: controller.signal }).then(async response => {
      if (!response.ok) throw new Error("Load failed");
      const loaded: State = await response.json();
      setState(loaded);
      try {
        const saved = JSON.parse(localStorage.getItem(`review-drafts:${loaded.catalog_sha256}`) ?? "{}");
        const valid: Record<string, Draft> = {};
        for (const record of loaded.batches.flatMap(b => b.records)) {
          const entry = saved[record.id] as Draft | undefined;
          if (entry?.revision === (record.review?.revision ?? 0) && record.questions.every(q =>
            entry.answers?.[q.id] && typeof entry.answers[q.id].choice === "string" && typeof entry.answers[q.id].custom === "string") &&
            (entry.override === null || typeof entry.override === "string")) valid[record.id] = entry;
        }
        setDrafts(valid);
      } catch { /* Invalid drafts never affect saved reviews. */ }
    }).catch(error => { if (error.name !== "AbortError") setLoadError(true); });
    return () => controller.abort();
  }, []);

  useEffect(() => {
    if (state) {
      try { localStorage.setItem(`review-drafts:${state.catalog_sha256}`, JSON.stringify(drafts)); } catch { /* Saving to disk still works. */ }
    }
  }, [drafts, state]);

  useEffect(() => { setMessage(null); setAudioError(false); }, [index]);

  function update(next: Draft) {
    setDrafts(current => ({ ...current, [clip.id]: next }));
    setMessage(null);
  }

  async function save(mode: "complete" | "partial") {
    if (!state || !clip || !draft) return;
    setSaving(true);
    setMessage(null);
    try {
      const response = await fetch("/api/review", { method: "POST", headers: {
        "Content-Type": "application/json", "X-Review-Token": state.token,
      }, body: JSON.stringify({ id: clip.id, catalog_sha256: state.catalog_sha256, revision: draft.revision,
        mode, answers: draft.answers, asr_text: draft.override ?? assembled }) });
      if (!response.ok) throw new Error("Save failed");
      const result: { review: Review } = await response.json();
      setState(current => current && ({ ...current, batches: current.batches.map(batch => ({ ...batch,
        records: batch.records.map(record => record.id === clip.id ? { ...record, review: result.review } : record) })) }));
      setDrafts(current => { const next = { ...current }; delete next[clip.id]; return next; });
      setMessage(mode);
    } catch { setMessage("error"); } finally { setSaving(false); }
  }

  function changeLocale(value: Locale) {
    void setLocale(value);
    try { localStorage.setItem("review-locale", value); } catch { /* Optional preference. */ }
  }

  return <div className="app-frame review-app"><main className="review-page">
    <header className="page-header">
      <div><div className="section-label">SOTTO / ULTRA PARAKEET</div>
        <h1 className="page-title">{t("Проверка по аудио")}</h1>
        <p className="page-sub">{t("Полный текст, короткие записи, только нужные исправления.")}</p></div>
      <div className="page-actions">
        <select className="field review-small" aria-label={t("Язык интерфейса")} value={locale} onChange={e => changeLocale(e.target.value as Locale)}>
          <option value="ru">{LOCALE_LABELS.ru}</option><option value="en">{LOCALE_LABELS.en}</option>
        </select>
        <button className="btn btn--ghost" onClick={() => setTheme(theme === "dark" ? "light" : "dark")}>{theme === "dark" ? t("Светлая тема") : t("Тёмная тема")}</button>
      </div>
    </header>
    {!state && <Card><p role="status">{loadError ? t("Не удалось загрузить записи. Проверьте, что локальный сервер работает, и обновите страницу.") : t("Загрузка…")}</p>
      {loadError && <button className="btn" onClick={() => location.reload()}>{t("Обновить страницу")}</button>}</Card>}
    {state && records.length === 0 && <Card><p>{t("В этой партии пока нет записей.")}</p></Card>}
    {state && clip && draft && <>
      <div className="review-progress">
        <span>{t("Партия {batch} · {done} из {total} проверено", { batch: state.batches.find(b => b.records.some(r => r.id === clip.id))!.id,
          done: records.filter(r => r.review?.mode === "complete").length, total: records.length })}</span>
        <span>{t("Аудио остаётся на этом компьютере")}</span>
      </div>
      <nav className="review-nav" aria-label={t("Записи")}>
        {records.map((record, i) => <button key={record.id} className={`btn ${i === index ? "btn--primary" : "btn--ghost"}`}
          aria-current={i === index ? "step" : undefined} disabled={saving} onClick={() => setIndex(i)}>
          {t("Запись {n}", { n: i + 1 })}{record.review?.mode === "complete" ? " ✓" : ""}
        </button>)}
      </nav>
      <Card accent className="review-audio">
        <CardHead title={t("Запись {n} · {seconds} с", { n: index + 1, seconds: clip.duration_seconds.toFixed(2) })}
          sub={clip.id} actions={<span className={`pill ${clip.review?.mode === "complete" ? "ok" : "ghost"}`}>
            {clip.review?.mode === "complete" ? t("Проверено") : t("Ожидает проверки")}</span>} />
        <audio key={clip.id} ref={audio} controls preload="metadata" src={clip.audio_url} aria-label={t("Аудиозапись")}
          onError={() => setAudioError(true)} />
        <div className="review-audio-actions">
          <button className="btn btn--ghost" disabled={audioError} onClick={() => {
            if (audio.current) { audio.current.currentTime = 0; void audio.current.play().catch(() => setAudioError(true)); }
          }}>{t("Слушать с начала")}</button>
          <label>{t("Скорость")} <select key={clip.id} className="field review-small" aria-label={t("Скорость")} defaultValue="1" onChange={e => {
            if (audio.current) audio.current.playbackRate = Number(e.target.value);
          }}><option value="0.75">0.75×</option><option value="1">1×</option><option value="1.25">1.25×</option></select></label>
          <a href={clip.audio_url} target="_blank" rel="noreferrer">{t("Открыть WAV")}</a>
        </div>
        {audioError && <p className="review-error" role="alert">{t("Не удалось воспроизвести аудио. Откройте WAV или обновите страницу.")}</p>}
        <details><summary>{t("Путь к файлу")}</summary><code className="review-path">{clip.audio_path}</code></details>
      </Card>
      <Card>
        <CardHead title={t("Полный текст записи")} sub={t("Выбранные варианты уже подставлены. Проверьте и остальные слова по аудио.")} />
        <p className="review-transcript">{draft.override ?? transcript.map((part, i) => part.key ? <mark key={i}>{part.text}</mark> : part.text)}</p>
        <details><summary>{t("Исходный текст Ultra")}</summary><p className="review-original">{clip.ultra_text}</p></details>
      </Card>
      <Card>
        <CardHead title={t("Места для проверки")} sub={t("Выберите вариант по аудио или напишите свой. Изменения сразу появятся в полном тексте.")} />
        <div className="review-questions">{clip.questions.map(question => <fieldset key={question.id} disabled={saving || draft.override !== null}>
          <legend>{question.id} · {questionTitle(question.prompt)}</legend>
          {question.options.map(option => <label className="review-option" key={option.id}>
            <input type="radio" name={`${clip.id}:${question.id}`} checked={draft.answers[question.id].choice === option.id}
              onChange={() => update({ ...draft, answers: { ...draft.answers, [question.id]: { ...draft.answers[question.id], choice: option.id } } })} />
            <span><b>{option.id}</b> · {option.text}{option.note && <small>{option.note}</small>}</span>
          </label>)}
          <label className="review-option"><input type="radio" name={`${clip.id}:${question.id}`} checked={draft.answers[question.id].choice === "custom"}
            onChange={() => update({ ...draft, answers: { ...draft.answers, [question.id]: { ...draft.answers[question.id], choice: "custom" } } })} />{t("Свой вариант")}</label>
          <input className="field" aria-label={t("Свой вариант для {id}", { id: question.id })} placeholder={t("Напишите только этот фрагмент")}
            value={draft.answers[question.id].custom} disabled={saving || draft.override !== null} onChange={e => update({ ...draft, answers: { ...draft.answers,
              [question.id]: { choice: "custom", custom: e.target.value } } })} />
          <label className="review-option"><input type="radio" name={`${clip.id}:${question.id}`} checked={draft.answers[question.id].choice === "unclear"}
            onChange={() => update({ ...draft, answers: { ...draft.answers, [question.id]: { ...draft.answers[question.id], choice: "unclear" } } })} />{t("Неразборчиво")}</label>
        </fieldset>)}</div>
      </Card>
      <Card>
        <label className="review-option"><input type="checkbox" checked={draft.override !== null} disabled={saving}
          onChange={e => update({ ...draft, override: e.target.checked ? assembled : null })} />{t("Есть другие исправления в полном тексте")}</label>
        {draft.override !== null && <>
          <p className="page-sub">{t("Исправьте нужные слова ниже. Сохранится этот полный текст; повторения оставьте как в аудио. Пунктуация не требуется.")}</p>
          <textarea className="field review-editor" aria-label={t("Исправленный полный текст")} disabled={saving}
            value={draft.override} onChange={e => update({ ...draft, override: e.target.value })} />
          <p className="page-sub">{t("Чтобы снова выбирать варианты, снимите галочку свободной правки. Полный текст вернётся к выбранным вариантам.")}</p>
        </>}
        <p className="review-save-note">{t("Сохранение означает, что все слова в полном тексте проверены по аудио.")}</p>
        <div className="review-save-actions">
          <button className="btn btn--primary" disabled={saving || !complete || !(draft.override ?? assembled).trim()} onClick={() => void save("complete")}>
            {saving ? t("Сохранение…") : t("Сохранить проверенную запись")}</button>
          <button className="btn btn--ghost" disabled={saving} onClick={() => void save("partial")}>{t("Сохранить только фрагменты")}</button>
          {index + 1 < records.length && <button className="btn btn--ghost" disabled={saving} onClick={() => setIndex(index + 1)}>{t("Следующая запись")}</button>}
        </div>
        <div role="status" aria-live="polite" className={message === "error" ? "review-error" : "review-status"}>
          {message === "complete" && t("Сохранено. Полный текст добавлен в эталон для метрик.")}
          {message === "partial" && t("Фрагменты сохранены. Полный текст пока не считается эталоном.")}
          {message === "error" && <>{t("Не удалось сохранить. Черновик сохранён в браузере; обновите страницу и повторите.")}
            <button className="btn btn--ghost" onClick={() => location.reload()}>{t("Обновить страницу")}</button></>}
          {!complete && t("Для полного эталона нужно разобрать все отмеченные места. Можно сохранить только фрагменты.")}
        </div>
      </Card>
      <footer className="review-footer">{t("Слова — как в аудио. Предложения по чистовому тексту проверяем отдельно.")}</footer>
    </>}
  </main></div>;
}

createRoot(document.getElementById("root")!).render(<App />);
