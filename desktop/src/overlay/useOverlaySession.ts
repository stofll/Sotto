import { useCallback, useEffect, useRef, useState } from "react";
import { invoke as tauriInvoke } from "../bridge/invoke";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { subscribe } from "../bridge/events";
import { isCurrentSession, isCurrentSessionOrUnscoped } from "../bridge/sessionEvents";
import type { ConfigResult } from "../bridge/types";
import { applyLocaleFromConfig, t } from "../i18n";
import { overlayPreferences, overlayLayout } from "./overlayPreferences";
import type { OverlayDetailState as OverlayState } from "./overlayDetail";

type PreviewPayload = { session_id: number; text: string };

// `done` means the speech is decoded; `pasted` means the text is in the
// window. They used to be one state, so a slow LLM pass produced an
// overlay that announced a character count before anything was inserted.

type AiProcessingPayload = { fallback?: boolean; skipped_reason?: string };
type TranscriptionPayload = { text?: string; length?: number; ai_processing?: AiProcessingPayload; ai_problem?: string };
type PastePayload = { session_id?: number; length?: number; ai_processing?: AiProcessingPayload };
type ErrorPayload = { session_id?: number; message?: string };

function shortAiProblem(payload?: TranscriptionPayload) {
  if (payload?.ai_problem) return payload.ai_problem;
  const ai = payload?.ai_processing;
  // An unconfigured provider is not a fallback: no request was made at all, and
  // before this line such a dictation arrived without a single word about why
  // unprocessed text was inserted in a mode with an LLM.
  if (ai?.skipped_reason === "missing_provider" || ai?.skipped_reason === "missing_api_key") {
    return t("LLM не настроена, вставлен локальный текст");
  }
  if (!ai?.fallback) return "";
  if (ai.skipped_reason === "provider_timeout") return t("LLM не ответила, вставлен локальный текст");
  if (ai.skipped_reason === "provider_quota_or_rate_limit") return t("Лимит LLM, вставлен локальный текст");
  return t("Ошибка LLM, вставлен локальный текст");
}

export function useOverlaySession() {
  const sessionId = useRef<number | null>(null);
  const initialConfig = useRef<Promise<void> | null>(null);
  const [config, setConfig] = useState<ConfigResult | null>(null);
  const preferences = overlayPreferences(config?.overlay);
  function belongsToCurrentSession(payload: unknown) {
    return isCurrentSession(payload, sessionId.current);
  }

  function belongsToCurrentSessionOrIsUnscoped(payload: unknown) {
    return isCurrentSessionOrUnscoped(payload, sessionId.current);
  }

  // The overlay is a separate webview window with its own JS context, so the
  // main window's language does not carry over by itself. We read it once on
  // mount and listen for later changes; this adds no IPC at recording start.
  useEffect(() => {
    let disposed = false;
    let updated = false;
    const apply = (next: ConfigResult) => {
      if (disposed) return;
      setConfig(next);
      applyLocaleFromConfig(next.ui_language);
    };
    const unlisten = subscribe<ConfigResult>("config-updated", (next) => {
      updated = true;
      apply(next);
    });
    initialConfig.current = tauriInvoke<ConfigResult>("get_config")
      .then((next) => { if (!updated) apply(next); })
      .catch(() => { if (!disposed && !updated) applyLocaleFromConfig(undefined); });
    return () => { disposed = true; unlisten(); };
  }, []);
  const [state, setState] = useState<OverlayState | null>(null);
  const [recordingStartedAt, setRecordingStartedAt] = useState(Date.now());
  const [recordingStoppedAt, setRecordingStoppedAt] = useState<number | null>(null);
  // Length of the text that actually went into the window, reported by
  // `paste-done`. Deliberately NOT derived from the `whisper-done` payload:
  // that text is the pre-LLM draft, so counting it announced a number for
  // characters that were never inserted.
  const [pastedLength, setPastedLength] = useState<number | null>(null);
  // When decoding finished, so "Распознано" can start showing how long the
  // post-processing has been running.
  const [decodedAt, setDecodedAt] = useState<number | null>(null);
  // The streaming model's growing hypothesis. It lives only while recording:
  // after the stop its place is taken by the final text, and showing the draft
  // and the result at once means showing two different answers to one question.
  const [previewText, setPreviewText] = useState("");
  // The session the live preview is enabled for. Stored as a number rather than
  // a flag: the enable event and `recording-started` come from different places
  // and their order is not guaranteed — the number shows they are about one and
  // the same dictation.
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

    if (sessionId.current === null) {
      void hide().finally(release);
      return;
    }
    void tauriInvoke<boolean>("cancel_recording", { sessionId: sessionId.current })
      // A refusal (`false`) means final delivery already claimed the session,
      // so nothing was cancelled. That is a reason not to *claim* a cancel,
      // not a reason to keep the pill on screen — hide either way.
      .then((cancelled) => {
        if (!cancelled) console.warn("cancelRecording: backend refused, session already committing");
      })
      .catch(() => {})
      .finally(() => hide().finally(release));
  }, [state]);

  const resetDetails = (startedAt = Date.now()) => {
    isClosingRef.current = false;
    setIsClosing(false);
    setRecordingStartedAt(startedAt);
    setRecordingStoppedAt(null);
    setPastedLength(null);
    setDecodedAt(null);
    setErrorText("");
    setAiProblem("");
  };

  useEffect(() => {
    const win = getCurrentWebviewWindow();
    const isOverlayState = (value: unknown): value is OverlayState => {
      return typeof value === "string" && ["recording", "processing", "loading", "done", "pasted", "error"].includes(value);
    };
    const applyOverlayState = (next: OverlayState) => {
      isClosingRef.current = false;
      setIsClosing(false);
      setState(next);
      if (next === "recording") resetDetails();
      if (next === "processing") {
        setRecordingStoppedAt((current) => current ?? Date.now());
      }
    };
    const resetOverlayState = () => {
      sessionId.current = null;
      resetDetails();
      setState(null);
      setPreviewText("");
      setArmedSession(null);
    };

    let disposed = false;
    const unlistenState = win.listen<string>("overlay-state", (e) => {
      if (disposed || !isOverlayState(e.payload)) return;
      applyOverlayState(e.payload);
    });
    // Rust emits this right before window.hide() so the next window.show()
    // doesn't flash the previous-recording UI before a fresh state arrives.
    const unlistenReset = win.listen("overlay-reset", () => {
      if (!disposed) resetOverlayState();
    });
    const registrations = [unlistenState, unlistenReset];
    const stops: Array<() => void> = [];
    for (const registration of registrations) {
      void registration.then((stop) => {
        if (disposed) stop(); else stops.push(stop);
      }).catch(() => {});
    }
    void Promise.all(registrations).then(async () => {
      await initialConfig.current;
      if (disposed) return;
      await tauriInvoke("overlay_ready");
      if (disposed) return;
      const current = await tauriInvoke<string | null>("current_state");
      if (disposed) return;
      if (isOverlayState(current)) applyOverlayState(current);
      else if (!current) void tauriInvoke("hide").catch(() => {});
    }).catch(() => {});
    return () => {
      disposed = true;
      stops.forEach((stop) => stop());
    };
  }, []);

  // A separate overlay shape for a dictation with a streaming model. The signal
  // is the model itself, not the presence of text: otherwise the window changes
  // shape mid-phrase, at exactly the moment somebody is looking at it.
  // Arrived text is the safety net for when the enable event missed the window
  // warm-up: there is nowhere to show a hypothesis inside the pill.
  const streaming = state === "recording" && (armedSession !== null || previewText.length > 0);
  const needsText = state === "error" || (state === "pasted" && !!aiProblem);
  const layout = overlayLayout(preferences.form, streaming, needsText);
  useEffect(() => {
    void tauriInvoke("set_overlay_presentation", { streaming, needsText }).catch(() => {});
  }, [streaming, needsText]);

  useEffect(() => {
    const unlisteners = [
      subscribe<PreviewPayload>("transcription-delta", (payload) => {
        // An event from the previous dictation must not append to the current
        // one: Rust already filters by session, but a window between the stop
        // and the next start exists all the same.
        if (sessionId.current === null || payload?.session_id !== sessionId.current) return;
        setPreviewText(payload.text ?? "");
      }),
      subscribe<{ session_id?: number; armed?: boolean }>("live-preview-armed", (payload) => {
        setArmedSession(payload?.armed ? (payload.session_id ?? null) : null);
      }),
      subscribe<number>("recording-started", (payload) => {
        sessionId.current = payload;
        setPreviewText("");
        // We do not overwrite the mark if it already arrived for this same
        // dictation: the order of these two events is not guaranteed.
        setArmedSession((current) => (current === payload ? current : null));
        setState("recording");
        resetDetails();
      }),
      subscribe<number>("recording-stopped", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        setPreviewText("");
        setArmedSession(null);
        setState("processing");
        setRecordingStoppedAt(Date.now());
      }),
      subscribe<number>("whisper-started", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        setState("processing");
        setRecordingStoppedAt((current) => current ?? Date.now());
      }),
      // Decoded, not delivered. The payload is the raw whisper output —
      // local formatting and the LLM pass still have to run — so nothing
      // here may claim a length or an outcome. `paste-done` does that.
      subscribe<TranscriptionPayload>("whisper-done", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        setState("done");
        setPastedLength(null);
        setAiProblem("");
        setDecodedAt(Date.now());
      }),
      // The text is in the window: the only moment a character count is
      // true, and the first moment the LLM outcome is known.
      subscribe<PastePayload>("paste-done", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        sessionId.current = null;
        setState("pasted");
        setPastedLength(typeof payload?.length === "number" ? payload.length : null);
        setAiProblem(shortAiProblem(payload));
        setDecodedAt(null);
      }),
      // Transcribed but could not be inserted. A separate event, because whisper
      // has nothing to do with it and the overlay would otherwise keep waiting
      // for an insertion that will never come.
      subscribe<ErrorPayload>("paste-failed", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        sessionId.current = null;
        setState("error");
        setDecodedAt(null);
        setErrorText(payload?.message ?? t("Не удалось вставить текст в активное окно."));
      }),
      subscribe<ErrorPayload>("whisper-failed", (payload) => {
        if (!belongsToCurrentSessionOrIsUnscoped(payload)) return;
        sessionId.current = null;
        setState("error");
        setErrorText(
          payload?.message
            ?? t("Не удалось распознать речь. Откройте «Настройки → Модели» и убедитесь, что модель скачана."),
        );
      }),
      subscribe<ErrorPayload>("whisper-load-failed", (payload) => {
        setState("error");
        setErrorText(
          payload?.message
            ?? t("Не удалось загрузить модель. Откройте «Настройки → Модели» и попробуйте снова."),
        );
      }),
      subscribe<unknown>("whisper-empty", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        sessionId.current = null;
        setState(null);
        // Overlay hides via Rust's hide() call (subscribe_engine_events).
      }),
      subscribe<unknown>("whisper-cancelled", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        sessionId.current = null;
        setState(null);
        // Overlay will hide via Rust's hide() call on cancellation.
      }),
    ];
    return () => unlisteners.forEach((stop) => stop());
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") handleClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [handleClose]);

  return {
    state, config, preferences, layout, sessionId: sessionId.current, streaming, recordingStartedAt, recordingStoppedAt,
    pastedLength, decodedAt, previewText, errorText, aiProblem, isClosing, handleClose,
  };
}

export type OverlaySession = ReturnType<typeof useOverlaySession>;
