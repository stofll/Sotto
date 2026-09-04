import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { isCurrentSession, isCurrentSessionOrUnscoped } from "../bridge/sessionEvents";
import { Icon } from "../components/Icon";
import { applyLocaleFromConfig, t, useLocale } from "../i18n";
import type { ConfigResult } from "../bridge/types";
import { overlayDetail } from "./overlayDetail";

let _currentSessionId: number | null = null;

type PreviewPayload = { session_id: number; text: string };

// `done` means the speech is decoded; `pasted` means the text is in the
// window. They used to be one state, so a slow LLM pass produced an
// overlay that announced a character count before anything was inserted.
type OverlayState = "recording" | "processing" | "loading" | "done" | "pasted" | "error";
type TimedPayload = { timestamp?: number };
type AiProcessingPayload = { fallback?: boolean; skipped_reason?: string };
type TranscriptionPayload = { text?: string; length?: number; ai_processing?: AiProcessingPayload; ai_problem?: string };
type PastePayload = { session_id?: number; length?: number; ai_processing?: AiProcessingPayload };
type ErrorPayload = { session_id?: number; message?: string };
type AudioLevelPayload = { level?: number };

function belongsToCurrentSession(payload: unknown) {
  return isCurrentSession(payload, _currentSessionId);
}

function belongsToCurrentSessionOrIsUnscoped(payload: unknown) {
  return isCurrentSessionOrUnscoped(payload, _currentSessionId);
}

const stateMeta = () => ({
  recording: { label: t("Запись"), color: "var(--rec)", tint: "rgba(239,111,71,0.18)" },
  processing: { label: t("Распознаю"), color: "var(--accent)", tint: "rgba(246,169,59,0.16)" },
  loading: { label: t("Загрузка модели"), color: "var(--accent)", tint: "rgba(246,169,59,0.16)" },
  done: { label: t("Распознано"), color: "var(--accent)", tint: "rgba(246,169,59,0.16)" },
  pasted: { label: t("Готово"), color: "var(--ok)", tint: "rgba(74,222,128,0.14)" },
  error: { label: t("Ошибка"), color: "var(--err)", tint: "rgba(239,94,107,0.16)" },
});

function eventTime(payload?: TimedPayload) {
  return payload?.timestamp ? payload.timestamp * 1000 : Date.now();
}

function formatDuration(ms: number) {
  const total = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

function clampLevel(value: unknown) {
  return Math.max(0, Math.min(1, typeof value === "number" && Number.isFinite(value) ? value : 0));
}

function shortAiProblem(payload?: TranscriptionPayload) {
  if (payload?.ai_problem) return payload.ai_problem;
  const ai = payload?.ai_processing;
  // Ненастроенный провайдер — не fallback: запроса не было вовсе, и до этой
  // строки такая диктовка приходила без единого слова о том, почему в режиме
  // с LLM вставился необработанный текст.
  if (ai?.skipped_reason === "missing_provider" || ai?.skipped_reason === "missing_api_key") {
    return t("LLM не настроена, вставлен локальный текст");
  }
  if (!ai?.fallback) return "";
  if (ai.skipped_reason === "provider_timeout") return t("LLM не ответила, вставлен локальный текст");
  if (ai.skipped_reason === "provider_quota_or_rate_limit") return t("Лимит LLM, вставлен локальный текст");
  return t("Ошибка LLM, вставлен локальный текст");
}

export function OverlayApp() {
  useLocale();
  // Оверлей — отдельное webview-окно со своим JS-контекстом, поэтому язык
  // главного окна сюда сам не переносится. Читаем его один раз при монтаже и
  // слушаем дальнейшие изменения; это не добавляет IPC на старт записи.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void tauriInvoke<ConfigResult>("get_config")
      .then((config) => { if (!disposed) applyLocaleFromConfig(config.ui_language); })
      .catch(() => { if (!disposed) applyLocaleFromConfig(undefined); });
    void listen<ConfigResult>("config-updated", (event) => {
      applyLocaleFromConfig(event.payload.ui_language);
    }).then((fn) => {
      if (disposed) fn(); else unlisten = fn;
    });
    return () => { disposed = true; unlisten?.(); };
  }, []);
  const [state, setState] = useState<OverlayState | null>(null);
  const [levels, setLevels] = useState<number[]>(Array(24).fill(0.04));
  const [borderEnergy, setBorderEnergy] = useState(0.08);
  const [recordingStartedAt, setRecordingStartedAt] = useState(Date.now());
  const [recordingStoppedAt, setRecordingStoppedAt] = useState<number | null>(null);
  const [now, setNow] = useState(Date.now());
  // Length of the text that actually went into the window, reported by
  // `paste-done`. Deliberately NOT derived from the `whisper-done` payload:
  // that text is the pre-LLM draft, so counting it announced a number for
  // characters that were never inserted.
  const [pastedLength, setPastedLength] = useState<number | null>(null);
  // When decoding finished, so "Распознано" can start showing how long the
  // post-processing has been running.
  const [decodedAt, setDecodedAt] = useState<number | null>(null);
  // Растущая гипотеза потоковой модели. Живёт только во время записи: после
  // остановки её место занимает финальный текст, а показывать одновременно
  // черновик и результат — значит показывать два разных ответа на один
  // вопрос.
  const [previewText, setPreviewText] = useState("");
  // Сессия, для которой включён живой предпросмотр. Хранится номером, а не
  // флагом: событие о включении и `recording-started` приходят из разных
  // мест, и порядок между ними не гарантирован — по номеру видно, что они
  // об одной и той же диктовке.
  const [armedSession, setArmedSession] = useState<number | null>(null);
  const [errorText, setErrorText] = useState("");
  const [aiProblem, setAiProblem] = useState("");
  const [isClosing, setIsClosing] = useState(false);
  const isClosingRef = useRef(false);

  const handleClose = useCallback(() => {
    if (isClosingRef.current || state === null) return;
    isClosingRef.current = true;
    setIsClosing(true);
    const hide = () => tauriInvoke("hide").catch(() => {});
    // Every path below has to end in `hide()` and release `isClosingRef`.
    // The pill has no other control, so a path that skips either one leaves
    // the user staring at an overlay they cannot dismiss until the
    // stuck-overlay timeout.
    const release = () => {
      isClosingRef.current = false;
      setIsClosing(false);
    };

    if (state === "pasted" || state === "error") {
      void hide().finally(release);
      return;
    }

    if (_currentSessionId === null) {
      void hide().finally(release);
      return;
    }
    void tauriInvoke<boolean>("cancel_recording", { sessionId: _currentSessionId })
      // A refusal (`false`) means final delivery already claimed the session,
      // so nothing was cancelled. That is a reason not to *claim* a cancel,
      // not a reason to keep the pill on screen — hide either way.
      .then((cancelled) => {
        if (!cancelled) console.warn("cancelRecording: backend refused, session already committing");
      })
      .catch(() => {})
      .finally(() => hide().finally(release));
  }, [state]);

  useLayoutEffect(() => {
    const htmlBg = document.documentElement.style.background;
    const htmlOverflow = document.documentElement.style.overflow;
    const bodyBg = document.body.style.background;
    const bodyOverflow = document.body.style.overflow;
    const root = document.getElementById("root");
    const rootBg = root?.style.background;
    const rootOverflow = root?.style.overflow;
    document.documentElement.classList.add("overlay-window");
    document.documentElement.style.background = "transparent";
    document.documentElement.style.overflow = "hidden";
    document.body.style.background = "transparent";
    document.body.style.overflow = "hidden";
    if (root) root.style.background = "transparent";
    if (root) root.style.overflow = "hidden";
    return () => {
      document.documentElement.style.background = htmlBg;
      document.documentElement.style.overflow = htmlOverflow;
      document.documentElement.classList.remove("overlay-window");
      document.body.style.background = bodyBg;
      document.body.style.overflow = bodyOverflow;
      if (root) root.style.background = rootBg ?? "";
      if (root) root.style.overflow = rootOverflow ?? "";
    };
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    const win = getCurrentWebviewWindow();
    const isOverlayState = (value: unknown): value is OverlayState => {
      return typeof value === "string" && ["recording", "processing", "loading", "done", "pasted", "error"].includes(value);
    };
    const applyOverlayState = (next: OverlayState) => {
      isClosingRef.current = false;
      setIsClosing(false);
      setState(next);
      if (next === "recording") {
        setRecordingStartedAt(Date.now());
        setRecordingStoppedAt(null);
        setPastedLength(null);
        setDecodedAt(null);
        setErrorText("");
        setAiProblem("");
        setLevels(Array(24).fill(0.04));
        setBorderEnergy(0.08);
      }
      if (next === "processing") {
        setRecordingStoppedAt((current) => current ?? Date.now());
      }
    };
    const resetOverlayState = () => {
      isClosingRef.current = false;
      setState(null);
      setIsClosing(false);
      setPastedLength(null);
      setDecodedAt(null);
      setPreviewText("");
      setArmedSession(null);
      setErrorText("");
      setAiProblem("");
      setRecordingStartedAt(Date.now());
      setRecordingStoppedAt(null);
      setLevels(Array(24).fill(0.04));
      setBorderEnergy(0.08);
    };

    let disposed = false;
    const unlistenState = win.listen<string>("overlay-state", (e) => {
      if (!isOverlayState(e.payload)) return;
      applyOverlayState(e.payload);
    });
    // Rust emits this right before window.hide() so the next window.show()
    // doesn't flash the previous-recording UI before a fresh state arrives.
    const unlistenReset = win.listen("overlay-reset", () => {
      resetOverlayState();
    });
    const unlisten = Promise.all([unlistenState, unlistenReset]).then(async (fns) => {
      if (!disposed) {
        await tauriInvoke("overlay_ready").catch(() => {});
        // Initial-state handshake: after listeners are wired up, ask Rust for
        // the current state. Rust may have queued it before React hydrated.
        try {
          const current = await tauriInvoke<string | null>("current_state");
          if (!disposed && isOverlayState(current)) {
            applyOverlayState(current);
          } else if (!disposed && !current) {
            void tauriInvoke("hide").catch(() => {});
          }
        } catch {
          // ignore overlay warm-up races
        }
      }
      return () => fns.forEach((fn) => fn());
    });
    return () => {
      disposed = true;
      unlisten.then((fn) => fn());
    };
  }, []);

  // Отдельная форма оверлея на время диктовки потоковой моделью. Признак —
  // сама модель, а не наличие текста: иначе окно меняет форму посреди
  // фразы, ровно в тот момент, когда на него смотрят.
  // Пришедший текст — страховка на случай, если событие о включении
  // разминулось с прогревом окна: показать гипотезу в таблетке негде.
  const streaming = state === "recording" && (armedSession !== null || previewText.length > 0);
  useEffect(() => {
    void tauriInvoke("set_overlay_streaming", { streaming }).catch(() => {});
  }, [streaming]);

  useEffect(() => {
    const unlisteners = Promise.all([
      listen<PreviewPayload>("transcription-delta", (e) => {
        // Событие предыдущей диктовки не должно дописывать текущую: Rust
        // уже фильтрует по сессии, но между остановкой и следующим стартом
        // окно всё равно есть.
        if (_currentSessionId === null || e.payload?.session_id !== _currentSessionId) return;
        setPreviewText(e.payload.text ?? "");
      }),
      listen<{ session_id?: number; armed?: boolean }>("live-preview-armed", (e) => {
        setArmedSession(e.payload?.armed ? (e.payload.session_id ?? null) : null);
      }),
      listen<number>("recording-started", (e) => {
        _currentSessionId = e.payload;
        setPreviewText("");
        // Не затираем отметку, если она уже пришла про эту же диктовку:
        // порядок этих двух событий не гарантирован.
        setArmedSession((current) => (current === e.payload ? current : null));
        setState("recording");
        setRecordingStartedAt(eventTime(e.payload as unknown as TimedPayload));
        setRecordingStoppedAt(null);
        setPastedLength(null);
        setDecodedAt(null);
        setErrorText("");
        setAiProblem("");
        setLevels(Array(24).fill(0.04));
        setBorderEnergy(0.08);
      }),
      listen<number>("recording-stopped", (e) => {
        if (!belongsToCurrentSession(e.payload)) return;
        setPreviewText("");
        setArmedSession(null);
        setState("processing");
        setRecordingStoppedAt(Date.now());
      }),
      listen<number>("whisper-started", (e) => {
        if (!belongsToCurrentSession(e.payload)) return;
        setState("processing");
        setRecordingStoppedAt((current) => current ?? Date.now());
      }),
      listen<AudioLevelPayload>("audio-level", (e) => {
        const level = clampLevel(e.payload?.level);
        setLevels((current) => [...current.slice(1), level]);
        setBorderEnergy((current) => current * 0.78 + Math.sqrt(level) * 0.22);
      }),
      // Decoded, not delivered. The payload is the raw whisper output —
      // local formatting and the LLM pass still have to run — so nothing
      // here may claim a length or an outcome. `paste-done` does that.
      listen<TranscriptionPayload>("whisper-done", (e) => {
        if (!belongsToCurrentSession(e.payload)) return;
        setState("done");
        setPastedLength(null);
        setAiProblem("");
        setDecodedAt(Date.now());
      }),
      // The text is in the window: the only moment a character count is
      // true, and the first moment the LLM outcome is known.
      listen<PastePayload>("paste-done", (e) => {
        if (!belongsToCurrentSession(e.payload)) return;
        _currentSessionId = null;
        setState("pasted");
        setPastedLength(typeof e.payload?.length === "number" ? e.payload.length : null);
        setAiProblem(shortAiProblem(e.payload));
        setDecodedAt(null);
      }),
      // Распознали, но вставить не смогли. Отдельное событие, потому что
      // whisper тут ни при чём, а оверлей иначе остаётся ждать вставки,
      // которой не будет.
      listen<ErrorPayload>("paste-failed", (e) => {
        if (!belongsToCurrentSession(e.payload)) return;
        _currentSessionId = null;
        setState("error");
        setDecodedAt(null);
        setErrorText(e.payload?.message ?? t("Не удалось вставить текст в активное окно."));
      }),
      listen<ErrorPayload>("whisper-failed", (e) => {
        if (!belongsToCurrentSessionOrIsUnscoped(e.payload)) return;
        _currentSessionId = null;
        setState("error");
        setErrorText(
          e.payload?.message
            ?? t("Не удалось распознать речь. Откройте «Настройки → Модели» и убедитесь, что модель скачана."),
        );
      }),
      listen<ErrorPayload>("whisper-load-failed", (e) => {
        setState("error");
        setErrorText(
          e.payload?.message
            ?? t("Не удалось загрузить модель. Откройте «Настройки → Модели» и попробуйте снова."),
        );
      }),
      listen<unknown>("whisper-empty", (e) => {
        if (!belongsToCurrentSession(e.payload)) return;
        _currentSessionId = null;
        setState(null);
        // Overlay hides via Rust's hide() call (subscribe_engine_events).
      }),
      listen<unknown>("whisper-cancelled", (e) => {
        if (!belongsToCurrentSession(e.payload)) return;
        _currentSessionId = null;
        setState(null);
        // Overlay will hide via Rust's hide() call on cancellation.
      }),
    ]);
    return () => { unlisteners.then((items) => items.forEach((fn) => fn())); };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") handleClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [handleClose]);

  if (state === null) return null;

  const duration = formatDuration((recordingStoppedAt ?? now) - recordingStartedAt);
  const borderPulse = state === "recording" ? Math.max(0.08, Math.min(1, borderEnergy)) : 0.25;
  const innerBorderAlpha = 0.08 + borderPulse * 0.34;
  const innerGlowAlpha = 0.06 + borderPulse * 0.22;
  const surfaceGlowAlpha = 0.03 + borderPulse * 0.14;

  const shell = {
    width: "100%",
    maxWidth: streaming ? 592 : 552,
    height: streaming ? 142 : 56,
    borderRadius: streaming ? 22 : 999,
    padding: 4,
    background: "linear-gradient(180deg, rgba(142,64,43,0.82), rgba(80,38,29,0.90))",
    boxShadow: "0 1px 0 rgba(255,255,255,0.05) inset",
  } as const;
  const surface = {
    width: "100%",
    height: "100%",
    borderRadius: streaming ? 18 : 999,
    background: `radial-gradient(circle at 48% 50%, rgba(239,111,71,${surfaceGlowAlpha}) 0%, rgba(239,111,71,${surfaceGlowAlpha * 0.55}) 34%, rgba(15,17,22,0) 68%), rgba(15, 17, 22, 0.97)`,
    border: `1px solid rgba(255,154,108,${innerBorderAlpha})`,
    boxShadow: `0 0 ${5 + borderPulse * 12}px rgba(239,111,71,${innerGlowAlpha}) inset`,
    transition: "background 120ms ease, border-color 120ms ease, box-shadow 120ms ease",
  } as const;

  return (
    <div className="app-frame" style={{ position: "fixed", inset: 0, padding: 0, background: "transparent", display: "flex", alignItems: "center", justifyContent: "center", overflow: "hidden", fontFamily: "var(--font-sans)", color: "var(--text)", letterSpacing: 0 }}>
      <div style={shell}>
        {streaming ? (
          // Текст занимает всю ширину и живёт под верхним рядом: в строке
          // между таймером и крестиком ему оставалось меньше половины окна,
          // и всё, что не помещалось, просто не показывалось.
          <div style={{ ...surface, display: "flex", flexDirection: "column", gap: 8, padding: "9px 11px" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <TimerBadge duration={duration}/>
              <div style={{ flex: 1, minWidth: 0 }}>
                <LevelWaveform levels={levels}/>
              </div>
              <CloseButton state={state} onClose={handleClose} disabled={isClosing}/>
            </div>
            <PreviewPane text={previewText}/>
          </div>
        ) : (
          <div style={{ ...surface, display: "grid", gridTemplateColumns: "62px 1fr 30px", alignItems: "center", columnGap: 8, padding: "0 8px" }}>
            <TimerBadge duration={state === "loading" ? "--:--" : duration}/>
            <div style={{ minWidth: 0, alignSelf: "center" }}>
              <StateDetail state={state} levels={levels} pastedLength={pastedLength} polishingMs={decodedAt === null ? 0 : now - decodedAt} errorText={errorText} aiProblem={aiProblem}/>
            </div>
            <CloseButton state={state} onClose={handleClose} disabled={isClosing}/>
          </div>
        )}
      </div>
    </div>
  );
}

function TimerBadge({ duration }: { duration: string }) {
  return (
    <div style={{ justifySelf: "start", height: 34, minWidth: 58, padding: "0 8px", borderRadius: 999, background: "rgba(239,111,71,0.12)", color: "var(--rec)", display: "flex", alignItems: "center", justifyContent: "center", gap: 6 }}>
      <span style={{ width: 6, height: 6, borderRadius: "50%", background: "var(--rec)", flex: "0 0 auto" }}/>
      <span style={{ font: "700 12px/1 var(--font-mono)", fontVariantNumeric: "tabular-nums" }}>{duration}</span>
    </div>
  );
}

function CloseButton({ state, onClose, disabled }: { state: OverlayState; onClose: () => void; disabled?: boolean }) {
  const meta = stateMeta()[state];
  const label = state === "recording" || state === "processing" || state === "done" ? t("Отменить запись") : state === "error" ? t("Закрыть") : t("Отменить");
  return (
    <button aria-label={label} onClick={onClose} disabled={disabled} style={{ appearance: "none", width: 28, height: 28, borderRadius: "50%", border: 0, background: "rgba(255,255,255,0.08)", color: state === "recording" ? "rgba(255,255,255,0.84)" : meta.color, display: "grid", placeItems: "center", cursor: disabled ? "default" : "pointer", opacity: disabled ? 0.55 : 1 }}>
      <Icon name="x" size={15}/>
    </button>
  );
}

function StateDetail({ state, levels, pastedLength, polishingMs, errorText, aiProblem }: { state: OverlayState; levels: number[]; pastedLength: number | null; polishingMs: number; errorText: string; aiProblem: string }) {
  const detail = overlayDetail({ state, pastedLength, polishingMs, errorText, aiProblem });
  if (detail.kind === "waveform") {
    return <LevelWaveform levels={levels}/>;
  }
  if (detail.kind === "progress") {
    return <ProgressStrip label={detail.label} />;
  }
  if ("warning" in detail) {
    return (
      <div style={{ display: "grid", gap: 2, minWidth: 0, textAlign: "left" }}>
        <div style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", font: "700 12px/1.1 var(--font-sans)", color: "var(--ok)" }}>{detail.text}</div>
        <div style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", font: "600 10px/1.1 var(--font-sans)", color: "var(--warn)" }}>{detail.warning}</div>
      </div>
    );
  }
  return <div style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", font: "600 14px/1.2 var(--font-sans)", color: "var(--text-2)", textAlign: "left" }}>{detail.text}</div>;
}

/// Живая лента гипотезы: видно всегда конец.
///
/// Обрезка по количеству символов этого не давала: показывались первые
/// строки последних N символов, то есть середина сказанного, а свежие слова
/// — те самые, ради которых на оверлей и смотрят, — оставались за нижним
/// краем. Поэтому не обрезаем, а прокручиваем к концу.
///
/// Гипотеза может переписываться задним числом, поэтому это черновик, а не
/// результат: он никуда не вставляется, вставится расшифровка всей записи.
function PreviewPane({ text }: { text: string }) {
  const ref = useRef<HTMLDivElement | null>(null);
  // Layout, а не обычный effect: прокрутка до отрисовки не даёт кадру с
  // текстом появиться в неправильном положении и дёрнуться.
  useLayoutEffect(() => {
    const node = ref.current;
    if (node) node.scrollTop = node.scrollHeight;
  }, [text]);
  if (!text) {
    return (
      <div style={{ flex: 1, display: "flex", alignItems: "center", font: "500 12px/1.3 var(--font-sans)", color: "var(--text-2)", opacity: 0.6 }}>
        {t("Говорите — текст появится здесь")}
      </div>
    );
  }
  return (
    <div
      ref={ref}
      style={{
        flex: 1,
        overflow: "hidden",
        wordBreak: "break-word",
        font: "500 14px/1.45 var(--font-sans)",
        color: "var(--text)",
        opacity: 0.96,
        textAlign: "left",
      }}
    >
      {text}
    </div>
  );
}

function LevelWaveform({ levels }: { levels: number[] }) {
  const bars = useMemo(() => levels.slice(-24), [levels]);
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "center", gap: 3, height: 28 }}>
      {bars.map((level, index) => {
        const visualLevel = Math.sqrt(Math.max(0, level));
        const active = level > 0.04;
        // Brighter orange as the bar gets louder: interpolate from a warm
        // amber at low levels toward a vivid orange at the top so speech
        // reads clearly against the dark pill (levels are perceptual 0..1).
        const height = Math.max(5, Math.round(5 + visualLevel * 21));
        return <span key={index} style={{ width: 3, height: `${height}px`, background: active ? "linear-gradient(180deg, #ffc27a 0%, #ff8a3d 55%, #ff6a2c 100%)" : "rgba(255,255,255,0.13)", borderRadius: 4, opacity: active ? 1 : 0.66, boxShadow: active ? `0 0 ${6 + visualLevel * 8}px rgba(255,138,61,${0.35 + visualLevel * 0.35})` : "none", transition: "height 80ms ease, background 120ms ease, opacity 120ms ease" }}/>;
      })}
    </div>
  );
}

function ProgressStrip({ label }: { label?: string }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
      <div style={{ position: "relative", height: 4, borderRadius: 999, background: "rgba(255,255,255,0.08)", overflow: "hidden", flex: 1 }}>
        <div style={{ position: "absolute", inset: 0, width: "40%", borderRadius: 999, background: "linear-gradient(90deg, transparent, var(--accent), transparent)", animation: "progress-sweep 1.15s ease-in-out infinite" }}/>
      </div>
      {label && <span style={{ font: "500 11px/1 var(--font-sans)", color: "var(--text-2)", whiteSpace: "nowrap" }}>{label}</span>}
    </div>
  );
}
