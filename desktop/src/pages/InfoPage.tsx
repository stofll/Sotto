import { useEffect, useState, type ReactNode } from "react";
import { invoke, subscribe } from "../bridge";
import type { ConfigResult, UpdateDownloadProgress, UpdateInfo } from "../bridge/types";
import { Card, CardHead, PageHeader, SectionLabel } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Hint } from "../components/Hint";
import { confirmDestructive } from "../components/ConfirmDialog";
import { CustomSelect, type SelectOption } from "../components/CustomSelect";
import { t } from "../i18n";
import { FeedbackCard } from "./FeedbackCard";
import { DEFAULT_HOTKEY } from "../hotkey";

function hotkeyParts(hotkey?: string): string[] {
  const labels: Record<string, string> = { ctrl: "Ctrl", control: "Ctrl", shift: "Shift", alt: "Alt", win: "Win", cmd: "Win", super: "Win", space: "Space", enter: "Enter", esc: "Esc", tab: "Tab" };
  return (hotkey || DEFAULT_HOTKEY).split("+").map((part) => labels[part.trim().toLowerCase()] ?? part.trim().toUpperCase()).filter(Boolean);
}

function KbdSequence({ keys }: { keys: string[] }) {
  return <span style={{ display: "inline-flex", alignItems: "center", gap: 4, flexWrap: "wrap" }}>{keys.map((key, i) => <span key={`${key}-${i}`} style={{ display: "inline-flex", alignItems: "center", gap: 4 }}><span className="kbd">{key}</span>{i < keys.length - 1 && <span style={{ color: "var(--ink-mute)" }}>+</span>}</span>)}</span>;
}

function HelpCard({ title, icon, children, accent = false }: { title: string; icon?: string; children: ReactNode; accent?: boolean }) {
  return (
    <Card accent={accent} style={{ minWidth: 0 }}>
      <CardHead title={title} icon={icon}/>
      {children}
    </Card>
  );
}

function InfoRow({ label, value }: { label: string; value: ReactNode }) {
  return <div style={{ display: "flex", justifyContent: "space-between", gap: 12, padding: "8px 0", borderBottom: "1px solid var(--line-soft)", font: "500 12px/1.25 var(--font-sans)" }}><span style={{ color: "var(--ink-mute)" }}>{label}</span><span style={{ color: "var(--ink)", textAlign: "right" }}>{value}</span></div>;
}

function PipelineStep({ index, title, detail, icon }: { index: number; title: string; detail: string; icon: string }) {
  return (
    <div style={{ padding: 14, borderRadius: "var(--radius)", background: "var(--bg-2)", border: "1px solid var(--line)" }}>
      <div className="flex-row" style={{ gap: 8, marginBottom: 8 }}>
        <span style={{ font: "500 10px/1 var(--font-mono)", color: "var(--ink-mute)", background: "var(--bg-4)", padding: "2px 6px", borderRadius: 4 }}>{index}</span>
        <span className="card-icon" style={{ width: 26, height: 26, color: "var(--accent-text)" }}><Icon name={icon} size={13}/></span>
        <span style={{ font: "600 13px/1.2 var(--font-sans)", color: "var(--ink)" }}>{title}</span>
      </div>
      <div style={{ font: "400 12px/1.5 var(--font-sans)", color: "var(--ink-mute)" }}>{detail}</div>
    </div>
  );
}

type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "available"; info: UpdateInfo }
  | { kind: "downloading"; info: UpdateInfo; progress: UpdateDownloadProgress | null }
  | { kind: "error"; message: string };

function formatMb(bytes: number) {
  return t("{p0} МБ", { p0: (bytes / 1024 / 1024).toFixed(1) });
}

// An update is never installed by itself: the check when the page opens is
// silent (there is nothing to gain from showing a network error) and downloading
// happens only on an explicit click. The user always sees exactly what is
// coming: the version, the date and the release notes.
function UpdatesCard({ version }: { version?: string | null }) {
  const [state, setState] = useState<UpdateState>({ kind: "idle" });

  async function check(loud: boolean) {
    setState({ kind: "checking" });
    try {
      const info = await invoke<UpdateInfo>("check_update");
      setState(info.available ? { kind: "available", info } : { kind: "current" });
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      // A silent check on open must not shout about a missing network.
      setState(loud ? { kind: "error", message } : { kind: "idle" });
    }
  }

  useEffect(() => { void check(false); }, []);

  useEffect(() => {
    const unlisten = subscribe<UpdateDownloadProgress>("update-download-progress", (progress) => {
      setState((current) => current.kind === "downloading" ? { ...current, progress } : current);
    });
    return () => unlisten();
  }, []);

  async function install(info: UpdateInfo) {
    setState({ kind: "downloading", info, progress: null });
    try {
      // On success the application restarts and we never return here.
      await invoke("install_update");
    } catch (e) {
      setState({ kind: "error", message: e instanceof Error ? e.message : String(e) });
    }
  }

  const busy = state.kind === "checking" || state.kind === "downloading";
  const percent = state.kind === "downloading" && state.progress?.total
    ? Math.round((state.progress.downloaded / state.progress.total) * 100)
    : null;

  return (
    <HelpCard title={t("Обновления")} icon="download">
      <div style={{ display: "grid", gap: 0 }}>
        <InfoRow label={t("Установленная версия")} value={<span className="mono">{version ?? "0.0.0"}</span>}/>
        {state.kind === "available" && <InfoRow label={t("Доступна версия")} value={<span className="mono" style={{ color: "var(--accent-text)" }}>{state.info.version}</span>}/>}
        {state.kind === "available" && state.info.date && <InfoRow label={t("Опубликована")} value={state.info.date.slice(0, 10)}/>}
      </div>

      {state.kind === "available" && state.info.notes && (
        <div style={{ marginTop: 12 }}>
          <SectionLabel>{t("Что нового")}</SectionLabel>
          <div style={{ maxHeight: 180, overflow: "auto", padding: "10px 12px", borderRadius: "var(--radius-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 12px/1.55 var(--font-sans)", color: "var(--ink-dim)", whiteSpace: "pre-wrap" }}>
            {state.info.notes}
          </div>
        </div>
      )}

      {state.kind === "downloading" && (
        <div style={{ marginTop: 12 }}>
          <div style={{ position: "relative", height: 4, borderRadius: 999, background: "var(--bg-4)", overflow: "hidden" }}>
            {percent != null
              ? <div style={{ position: "absolute", inset: 0, width: `${percent}%`, background: "var(--accent)", borderRadius: 999, transition: "width 200ms ease" }}/>
              : <div style={{ position: "absolute", inset: 0, width: "40%", background: "linear-gradient(90deg, transparent, var(--accent), transparent)", animation: "progress-sweep 1.15s ease-in-out infinite", borderRadius: 999 }}/>}
          </div>
          <p style={{ margin: "8px 0 0", font: "400 11.5px/1.4 var(--font-sans)", color: "var(--ink-mute)" }}>
            {state.progress
              ? t("Скачано {p0}{p1}. Приложение перезапустится само.", { p0: formatMb(state.progress.downloaded), p1: state.progress.total ? ` из ${formatMb(state.progress.total)}` : "" })
              : t("Скачиваем обновление. Приложение перезапустится само.")}
          </p>
        </div>
      )}

      {state.kind === "current" && <p style={{ margin: "12px 0 0", font: "400 11.5px/1.4 var(--font-sans)", color: "var(--ink-mute)" }}>{t("Установлена последняя версия.")}</p>}
      {state.kind === "error" && <p style={{ margin: "12px 0 0", font: "400 11.5px/1.4 var(--font-sans)", color: "var(--err)" }}>{state.message}</p>}

      <div className="flex-row" style={{ gap: 8, marginTop: 12, flexWrap: "wrap" }}>
        <button className="btn btn--ghost" type="button" disabled={busy} onClick={() => void check(true)}>
          <Icon name="refresh" size={13}/> {state.kind === "checking" ? t("Проверяем…") : t("Проверить обновления")}
        </button>
        {state.kind === "available" && (
          <button className="btn btn--primary" type="button" onClick={() => void install(state.info)}>
            <Icon name="download" size={13}/>  {t("Обновить до")} {state.info.version}
          </button>
        )}
      </div>
    </HelpCard>
  );
}

const LOG_LEVELS = ["error", "warn", "info", "debug", "trace"] as const;

// Long enough to reach another window, short enough that nobody waits for it.
const PASTE_TEST_DELAY_SECONDS = 3;

// The logs rotate at 5 MB and keep three archives, so the size lives between
// kilobytes and a couple of dozen megabytes. "0.0 MB" on a fresh install reports
// nothing, so small values are shown in kilobytes.
function formatLogSize(bytes: number) {
  return bytes < 1024 * 1024
    ? t("{p0} КБ", { p0: Math.round(bytes / 1024).toString() })
    : t("{p0} МБ", { p0: (bytes / 1024 / 1024).toFixed(1) });
}

// Diagnosing somebody else's problem rests on what they send you. Here are the
// three things they can send: the log level, an environment summary, and the
// saved recordings.
function DiagnosticsCard({ config, onConfigChanged }: { config: ConfigResult | null; onConfigChanged?: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null> }) {
  const [report, setReport] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [logsBytes, setLogsBytes] = useState<number | null>(null);
  const [pasteCountdown, setPasteCountdown] = useState<number | null>(null);
  const [pasteResult, setPasteResult] = useState<string | null>(null);
  const logLevel = config?.log_level ?? "info";
  const saveRecordings = config?.debug_save_recordings ?? false;
  const overlayDiag = config?.debug_overlay_diag ?? false;

  // The size is read when the page opens and after a cleanup. It changes by a
  // megabyte a week; there is no point watching it in real time.
  useEffect(() => {
    void invoke<number>("logs_size").then(setLogsBytes).catch(() => setLogsBytes(null));
  }, []);

  // Clearing takes two clicks: the logs are the only trace of a problem that
  // has already happened, and a mis-click erases exactly what brought the person
  // to this page.
  async function clearLogs() {
    if (!await confirmDestructive(t("Очистить логи? Это действие нельзя отменить."), t("Очистить"))) return;
    try {
      setLogsBytes(await invoke<number>("clear_logs"));
    } catch {
      // An auxiliary action: there is nothing here worth failing over.
    }
  }

  // The paste test runs the real delivery pipeline, so it needs a real
  // target — and clicking the button puts focus on Sotto, which would make
  // the text land in this very window. The countdown is the window in which
  // to focus the application the test is meant to reach.
  useEffect(() => {
    if (pasteCountdown === null) return;
    if (pasteCountdown > 0) {
      const timer = window.setTimeout(() => setPasteCountdown(pasteCountdown - 1), 1000);
      return () => window.clearTimeout(timer);
    }
    let cancelled = false;
    void invoke<string>("test_paste")
      .then((ok) => { if (!cancelled) setPasteResult(ok); })
      .catch((e) => { if (!cancelled) setPasteResult(e instanceof Error ? e.message : String(e)); })
      .finally(() => { if (!cancelled) setPasteCountdown(null); });
    return () => { cancelled = true; };
  }, [pasteCountdown]);

  async function copyReport() {
    try {
      const text = await invoke<string>("get_diagnostics");
      setReport(text);
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      setReport(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <HelpCard title={t("Диагностика")} icon="info">
      <div style={{ display: "grid", gap: 0 }}>
        <InfoRow
          label={t("Подробность логов")}
          value={
            <div style={{ width: 150 }}>
              <CustomSelect<string>
                value={logLevel}
                disabled={!onConfigChanged}
                options={LOG_LEVELS.map<SelectOption<string>>((level) => ({ value: level, label: level }))}
                onChange={(next) => void onConfigChanged?.({ log_level: next as ConfigResult["log_level"] })}
              />
            </div>
          }
        />
        <InfoRow
          label={t("Размер логов")}
          value={
            <span style={{ color: "var(--ink-mute)", font: "500 11.5px/1 var(--font-mono)" }}>
              {logsBytes === null ? "—" : formatLogSize(logsBytes)}
            </span>
          }
        />
        <InfoRow
          label={t("Сохранять записи в WAV")}
          value={
            <Hint text={t("Класть каждую запись рядом с логами. Нужно, чтобы воспроизвести жалобу «распознало не то».")}>
            <label className="checkbox-row" style={{ justifyContent: "flex-end" }}>
              <input className="checkbox" type="checkbox" checked={saveRecordings} disabled={!onConfigChanged} onChange={(e) => void onConfigChanged?.({ debug_save_recordings: e.target.checked })}/>
              <span style={{ color: "var(--ink-mute)", font: "500 11.5px/1 var(--font-sans)" }}>{saveRecordings ? t("включено") : t("выключено")}</span>
            </label>
            </Hint>
          }
        />
        <InfoRow
          label={t("Диагностика оверлея")}
          value={
            <Hint text={t("Писать в лог стили и список окон на каждом показе и скрытии оверлея. Нужно только для разбора мигающей системной рамки; в логи попадают заголовки открытых окон.")}>
            <label className="checkbox-row" style={{ justifyContent: "flex-end" }}>
              <input className="checkbox" type="checkbox" checked={overlayDiag} disabled={!onConfigChanged} onChange={(e) => void onConfigChanged?.({ debug_overlay_diag: e.target.checked })}/>
              <span style={{ color: "var(--ink-mute)", font: "500 11.5px/1 var(--font-sans)" }}>{overlayDiag ? t("включено") : t("выключено")}</span>
            </label>
            </Hint>
          }
        />
      </div>
      <div className="flex-row" style={{ gap: 8, marginTop: 12, flexWrap: "wrap" }}>
        <button className="btn btn--ghost" type="button" onClick={() => void invoke("open_diagnostics_folder").catch(() => {})}>
          <Icon name="folder" size={12}/>  {t("Открыть папку логов")} </button>
        <button className="btn btn--ghost" type="button" onClick={() => void copyReport()}>
          <Icon name="copy" size={12}/> {copied ? t("Скопировано") : t("Скопировать сводку")}
        </button>
        <button className="btn btn--ghost" type="button" onClick={() => void clearLogs()}>
          <Icon name="trash" size={12}/> {t("Очистить логи")}
        </button>
        <Hint text={t("Кладёт пробный текст в буфер и вставляет его в активное окно тем же путём, что и диктовка. Отделяет поломку вставки от поломки распознавания.")}>
          <button
            className="btn btn--ghost"
            type="button"
            data-testid="paste-test"
            disabled={pasteCountdown !== null}
            onClick={() => { setPasteResult(null); setPasteCountdown(PASTE_TEST_DELAY_SECONDS); }}
          >
            <Icon name="test" size={12}/> {pasteCountdown === null
              ? t("Проверить вставку")
              : pasteCountdown > 0
                ? t("Переключитесь в нужное окно… {p0}", { p0: pasteCountdown.toString() })
                : t("Вставляю…")}
          </button>
        </Hint>
      </div>
      {pasteResult && (
        <p data-testid="paste-test-result" style={{ margin: "10px 0 0", font: "400 11.5px/1.5 var(--font-sans)", color: "var(--ink-mute)" }}>{pasteResult}</p>
      )}
      {report && (
        <pre style={{ margin: "12px 0 0", padding: 10, background: "var(--bg-2)", border: "1px solid var(--line)", borderRadius: "var(--radius)", font: "500 11px/1.5 var(--font-mono)", color: "var(--ink-mute)", whiteSpace: "pre-wrap", overflowX: "auto" }}>{report}</pre>
      )}
      {saveRecordings && (
        <p style={{ margin: "10px 0 0", font: "400 11.5px/1.5 var(--font-sans)", color: "var(--ink-mute)" }}>
           {t("Записи с микрофона пишутся на диск. Хранятся последние 50, старые удаляются автоматически.")} </p>
      )}
    </HelpCard>
  );
}

export function InfoPage({ version, config, onConfigChanged }: { version?: string | null; config: ConfigResult | null; onConfigChanged?: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null> }) {
  const pipelineMode = config?.ai_processing?.pipeline_mode ?? "local";
  const hotkey = hotkeyParts(config?.hotkey);
  const recordingMode = config?.recording_mode === "push_to_talk" ? t("Удержание клавиш") : t("Переключатель");
  const llmThreshold = config?.ai_processing?.llm_min_duration_seconds ?? 0;

  const pipelineSteps = [
    { icon: "kbd", title: t("Горячая клавиша"), detail: t("Приложение слушает глобальный hotkey, не забирая фокус у текущего окна. При старте запоминается окно, куда потом нужно вставить текст.") },
    { icon: "mic", title: t("Запись и overlay"), detail: t("Микрофон пишет фразу, поверх экрана появляется компактный overlay с уровнем звука и кнопкой отмены.") },
    { icon: "cpu", title: t("Локальное распознавание"), detail: t("Аудио распознается локальной моделью. На этом этапе появляется сырой текст без ручной правки.") },
    { icon: "wand", title: t("Форматирование"), detail: t("Локальные правила удаляют заполнители, слова-паразиты, повторы, лишние пробелы и добавляют базовую пунктуацию.") },
    { icon: "spark", title: t("LLM-этап"), detail: pipelineMode === "local" ? t("В текущем режиме LLM пропускается: итогом становится локально отформатированный текст.") : llmThreshold > 0 ? t("Если запись не короче {p0} с, текст отправляется выбранному LLM-провайдеру. При таймауте вставляется локальный fallback.", { p0: llmThreshold }) : t("Текст отправляется выбранному LLM-провайдеру. При таймауте вставляется локальный fallback.") },
    { icon: "copy", title: t("Вставка и история"), detail: t("Готовый текст вставляется в исходное окно, а запись сохраняется в истории вместе с raw/formatted/final версиями и статистикой обработки.") },
  ];

  return (
    <div className="page">
      <PageHeader
        title={t("Справка")}
      />

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.15fr) minmax(280px, .85fr)", gap: 14, marginBottom: 14 }} className="help-top">
        <HelpCard title={t("Как пользоваться")} icon="play" accent>
          <div style={{ display: "grid", gap: 10 }}>
            {[t("Откройте окно, куда нужно вставить результат."), t("Нажмите горячую клавишу и продиктуйте фразу."), t("Остановите запись тем же hotkey или отпустите клавиши в режиме удержания."), t("Дождитесь обработки: текст вставится автоматически, если включен auto-paste.")].map((text, i) => <div key={text} style={{ display: "grid", gridTemplateColumns: "22px 1fr", gap: 14, alignItems: "start", padding: "10px 0", borderBottom: i < 3 ? "1px solid var(--line-soft)" : "none" }}><span style={{ width: 22, height: 22, borderRadius: "50%", background: "var(--accent-soft)", color: "var(--accent-text)", display: "grid", placeItems: "center", font: "600 11px/1 var(--font-mono)" }}>{i + 1}</span><span style={{ font: "500 13.5px/1.55 var(--font-sans)", color: "var(--ink)" }}>{text}</span></div>)}
          </div>
        </HelpCard>

        <HelpCard title={t("Текущие команды")} icon="kbd">
          <div style={{ display: "grid", gap: 0 }}>
            <InfoRow label={t("Начать / остановить")} value={<KbdSequence keys={hotkey}/>}/>
            <InfoRow label={t("Отменить overlay")} value={<KbdSequence keys={["Esc"]}/>}/>
            <InfoRow label={t("Режим записи")} value={recordingMode}/>
            <InfoRow label={t("Автовставка")} value={config?.auto_paste ? t("Включена") : t("Выключена")}/>
          </div>
          {/* The "in … mode recording runs …" line is gone: the mode itself is
              on the row above, and how it works is written in the hint on
              «Режим записи» in settings, in the same place it is switched. */}
        </HelpCard>
      </div>

      <HelpCard title={t("Pipeline записи")} icon="spark">
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(250px, 1fr))", gap: 12 }}>
          {pipelineSteps.map((step, i) => <PipelineStep key={step.title} index={i + 1} title={step.title} detail={step.detail} icon={step.icon}/>) }
        </div>
      </HelpCard>

      <FeedbackCard/>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) minmax(280px, .85fr)", gap: 14, marginTop: 14 }} className="help-top">
        <DiagnosticsCard config={config} onConfigChanged={onConfigChanged}/>
        <UpdatesCard version={version}/>
      </div>

      <style>{`
        @media (max-width: 1100px) { .help-top { grid-template-columns: 1fr !important; } }
      `}</style>
    </div>
  );
}
