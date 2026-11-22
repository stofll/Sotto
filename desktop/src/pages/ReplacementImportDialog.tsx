import { useEffect, useMemo, useRef, useState } from "react";
import type { ReplacementRule } from "../bridge/types";
import { validateReplacementRules } from "../bridge/textFormatting";
import { Modal } from "../components/Modal";
import { Segmented } from "../components/Shell";
import { t } from "../i18n";
import { MAX_REPLACEMENT_IMPORT_BYTES, parseReplacementImport, reviewReplacementImport, type ReplacementImportPolicy, type ReplacementImportRow } from "./replacementImport";
import { ReplacementPager, REPLACEMENT_PAGE_SIZE } from "./ReplacementPager";

export function ReplacementImportDialog({ file, rules, onApply, onClose, onChooseFile }: {
  file: File;
  rules: ReplacementRule[];
  onApply: (rules: ReplacementRule[]) => void;
  onClose: () => void;
  onChooseFile: () => void;
}) {
  const [state, setState] = useState<{ rows: ReplacementImportRow[]; pending: boolean; error?: string }>({ rows: [], pending: true });
  const [attempt, setAttempt] = useState(0);
  const [policy, setPolicy] = useState<ReplacementImportPolicy>("skip");
  const [onlyProblems, setOnlyProblems] = useState(false);
  const [page, setPage] = useState(0);
  const parsed = useRef<ReplacementImportRow[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    setState({ rows: parsed.current ?? [], pending: true });
    void (async () => {
      try {
        // Retrying a native check preserves the reviewed rules and their ids.
        let rows = parsed.current;
        if (!rows) {
          if (file.size > MAX_REPLACEMENT_IMPORT_BYTES) throw new Error(t("Файл правил не должен превышать 2 МиБ."));
          const bytes = await file.arrayBuffer();
          if (cancelled) return;
          let source: string;
          try {
            source = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
          } catch {
            throw new Error(t("Файл правил должен быть в кодировке UTF-8."));
          }
          rows = parseReplacementImport(source);
        }
        if (cancelled) return;
        parsed.current = rows;
        setState({ rows, pending: true });
        const valid = rows.filter((entry): entry is ReplacementImportRow & { rule: ReplacementRule } => Boolean(entry.rule));
        const errors = valid.length ? await validateReplacementRules(valid.map((entry) => entry.rule)) : [];
        if (cancelled) return;
        const byRow = new Map(errors.map((error) => [valid[error.index]?.row, error.message]));
        setState({ rows: rows.map((entry) => byRow.has(entry.row) ? { ...entry, error: byRow.get(entry.row) } : entry), pending: false });
      } catch (error) {
        if (!cancelled) setState({ rows: parsed.current ?? [], pending: false, error: error instanceof Error ? error.message : String(error) });
      }
    })();
    return () => { cancelled = true; };
  }, [file, attempt]);

  const review = useMemo(() => reviewReplacementImport(rules, state.rows, policy), [rules, state.rows, policy]);
  const visible = onlyProblems ? review.rows.filter((row) => row.status !== "new") : review.rows;
  const displayPage = Math.min(page, Math.max(0, Math.ceil(visible.length / REPLACEMENT_PAGE_SIZE) - 1));
  const pageRows = visible.slice(displayPage * REPLACEMENT_PAGE_SIZE, (displayPage + 1) * REPLACEMENT_PAGE_SIZE);
  const statuses = { new: t("Новое"), duplicate: t("Повтор"), conflict: t("Конфликт"), invalid: t("Ошибка") };
  const canApply = !state.pending && !state.error && review.invalid === 0 && review.added + review.replaced > 0;

  return <Modal title={t("Импорт правил замен")} onClose={onClose} className="dictionary-modal" focusFirstInput={false}>
    <div className="modal__body">
      <p className="dictionary-note">{file.name}</p>
      <p className="dictionary-note">{t("JSON в UTF-8, до 2 МиБ: объект с replacement_rules, массив правил или словарь «поиск»: «замена». Пустая замена удаляет совпадение.")}</p>
      {state.pending && <div role="status" className="dictionary-note">{t("Проверяю правила…")}</div>}
      {state.error && <div role="alert" className="inline-error" style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{t("Не удалось проверить импорт: {reason}", { reason: state.error })}</div>}
      {!state.pending && !state.error && <>
        <p className="dictionary-note" aria-live="polite">{t("Корректных: {valid}; повторов: {duplicates}; конфликтов: {conflicts}; ошибок: {invalid}.", { valid: review.rows.length - review.invalid, duplicates: review.duplicates, conflicts: review.conflicts, invalid: review.invalid })}</p>
        {review.rows.length === 0 && <p className="dictionary-note">{t("В файле нет правил замен.")}</p>}
        {review.invalid > 0 && <div role="alert" className="inline-error">{t("Исправьте ошибки в файле и выберите его снова. Правила ещё не добавлены.")}</div>}
        {review.conflicts > 0 && <>
          <Segmented value={policy} options={[{ value: "skip", label: t("Пропускать совпадения") }, { value: "replace", label: t("Заменять совпадения") }]} onChange={(value) => setPolicy(value as ReplacementImportPolicy)}/>
          <p className="dictionary-note">{policy === "skip"
            ? t("При одинаковом поиске и режиме остаётся первое правило: из текущего списка или из файла.")
            : t("При одинаковом поиске и режиме применяется последнее правило из файла. Позиции существующих правил сохраняются.")}</p>
        </>}
        {review.duplicates + review.conflicts + review.invalid > 0 && <label className="dictionary-note flex-row" style={{ gap: 8 }}>
          <input type="checkbox" className="checkbox" checked={onlyProblems} onChange={(event) => { setOnlyProblems(event.target.checked); setPage(0); }}/>{t("Только повторы, конфликты и ошибки")}
        </label>}
        <div role="list" aria-label={t("Правила из файла")} className="dictionary-check__result">
          {pageRows.map((entry) => <div role="listitem" key={entry.row} style={{ padding: "8px 0", borderTop: "1px solid var(--line-soft)", overflowWrap: "anywhere" }}>
            <div className="flex-row" style={{ gap: 8, marginBottom: 4 }}><span className="dictionary-note">{t("Запись {number}", { number: entry.row })}</span><span className={`pill${entry.status === "invalid" ? " warn" : ""}`}>{statuses[entry.status]}</span></div>
            {entry.find && <div className="mono" style={{ whiteSpace: "pre-wrap" }}>{entry.find}{entry.rule && <> → {entry.rule.replace || t("удалить совпадение")}</>}</div>}
            {entry.error && <div className="inline-error" style={{ whiteSpace: "pre-wrap" }}>{entry.error}</div>}
            {entry.previous && <p className="dictionary-note">{entry.previousRow === undefined ? t("Совпадение в текущем списке") : t("Совпадение с записью {number} из файла", { number: entry.previousRow })}: <span className="mono">{entry.previous.find} → {entry.previous.replace || t("удалить совпадение")}</span></p>}
          </div>)}
        </div>
        <ReplacementPager label={t("Страницы импорта")} page={displayPage} count={visible.length} onPage={setPage}/>
        <p className="dictionary-note">{t("Добавится: {added}; заменится: {replaced}. После импорта нажмите «Сохранить» в списке правил.", { added: review.added, replaced: review.replaced })}</p>
      </>}
    </div>
    <div className="modal__foot" style={{ flexWrap: "wrap" }}>
      <button type="button" className="btn btn--ghost btn--sm" onClick={onChooseFile}>{t("Выбрать другой файл")}</button>
      {state.error && parsed.current && <button type="button" className="btn btn--ghost btn--sm" onClick={() => setAttempt((value) => value + 1)}>{t("Повторить проверку")}</button>}
      <div className="flex-row" style={{ gap: 8 }}>
        <button type="button" className="btn btn--ghost btn--sm" onClick={onClose}>{t("Отмена")}</button>
        <button type="button" className="btn btn--primary btn--sm" disabled={!canApply} onClick={() => onApply(review.rules)}>{t("Добавить в список")}</button>
      </div>
    </div>
  </Modal>;
}
