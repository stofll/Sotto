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

type AiProcessingPayload = { fallback?: boolean; skipped_reason?: string; late_pending?: boolean };
type TranscriptionPayload = { text?: string; length?: number; ai_processing?: AiProcessingPayload; ai_problem?: string };
type PastePayload = { session_id?: number; length?: number; ai_processing?: AiProcessingPayload };
type ErrorPayload = { session_id?: number; message?: string };

function shortAiProblem(payload?: TranscriptionPayload) {
  if (payload?.ai_problem) return payload.ai_problem;
  const ai = payload?.ai_processing;
  // An unconfigured provider is not a fallback: no request was made at all, and
  // before this line such a dictation arrived without a single word about why
  // unprocessed text was inserted in a mode with an LLM.
  if (ai?.skipped_reason === "missing_provider" || ai?.skipped_reason === "missing_api_key" || ai?.skipped_reason === "missing_system_prompt") {
    return t("LLM не настроена, вставлен локальный текст");
  }
  if (!ai?.fallback) return "";
  // Still running: the answer, if it comes, goes into the history entry.
  if (ai.skipped_reason === "provider_timeout" && ai.late_pending) return t("LLM не успела — ответ появится в истории");
  if (ai.skipped_reason === "provider_timeout") return t("LLM не ответила, вставлен локальный текст");
  if (ai.skipped_reason === "provider_quota_or_rate_limit") return t("Лимит LLM, вставлен локальный текст");
  return t("Ошибка LLM, вставлен локальный текст");
}

/** The visible body of the overlay. The window around it is transparent and
 *  can be much larger (the captions chip), so hovering the window is not
 *  hovering the overlay. */
const OVERLAY_BODY = ".ovs-skin, .ovs-close, .overlay-shell";
export const isOverlayBody = (target: EventTarget | Element | null) =>
  target instanceof Element && target.closest(OVERLAY_BODY) !== null;

export function useOverlaySession() {
  const liveStateVersion = useRef(0);
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
  // When the recording will stop by itself, once the length limit has warned.
  const [limitAt, setLimitAt] = useState<number | null>(null);
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
  // Keys the drawings to one dictation. Unlike `sessionId`, which is released
  // as soon as the result arrives, it holds until the next recording starts,
  // so the inserted note and an error keep the scene they transition from.
  const [dictationKey, setDictationKey] = useState<number | null>(null);
  const [errorText, setErrorText] = useState("");
  const [aiProblem, setAiProblem] = useState("");
  // The history entry a late LLM answer was written into: what its note's
  // buttons copy and open.
  const [lateEntryId, setLateEntryId] = useState<number | null>(null);
  const [isClosing, setIsClosing] = useState(false);
  const isClosingRef = useRef(false);
  // Rust is about to conceal the window: play the exit. A new state cancels it.
  const [leaving, setLeaving] = useState(false);
  const [hovered, setHovered] = useState(false);

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

    if (state === "pasted" || state === "error" || state === "late") {
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
    setLimitAt(null);
    setPastedLength(null);
    setDecodedAt(null);
    setErrorText("");
    setAiProblem("");
  };

  useEffect(() => {
    const win = getCurrentWebviewWindow();
    const isOverlayState = (value: unknown): value is OverlayState => {
      return typeof value === "string" && ["recording", "processing", "loading", "done", "pasted", "error", "late"].includes(value);
    };
    const applyOverlayState = (next: OverlayState) => {
      isClosingRef.current = false;
      setIsClosing(false);
      setLeaving(false);
      setState(next);
      if (next === "recording") resetDetails();
      if (next === "processing") {
        setRecordingStoppedAt((current) => current ?? Date.now());
      }
    };
    const resetOverlayState = () => {
      sessionId.current = null;
      setLeaving(false);
      resetDetails();
      setState(null);
      setPreviewText("");
      setArmedSession(null);
      setHovered(false);
    };

    let disposed = false;
    const unlistenState = win.listen<string>("overlay-state", (e) => {
      if (disposed || !isOverlayState(e.payload)) return;
      liveStateVersion.current++;
      applyOverlayState(e.payload);
    });
    // Rust emits this right before window.hide() so the next window.show()
    // doesn't flash the previous-recording UI before a fresh state arrives.
    const unlistenReset = win.listen("overlay-reset", () => {
      if (!disposed) {
        liveStateVersion.current++;
        resetOverlayState();
      }
    });
    // Native hit-test after show: the overlay can appear under the cursor,
    // which never fires pointerenter. CSS :hover on an inactive WKWebView
    // is equally unreliable until a click.
    // Rust reports where the pointer is in the window, or null outside it.
    const unlistenPointer = win.listen<{ x: number; y: number } | null>("overlay-pointer", (event) => {
      if (disposed) return;
      const at = event.payload;
      setHovered(at !== null && isOverlayBody(document.elementFromPoint(at.x, at.y)));
    });
    const unlistenLeaving = win.listen("overlay-leaving", () => {
      if (!disposed) {
        liveStateVersion.current++;
        setLeaving(true);
      }
    });
    const registrations = [unlistenState, unlistenReset, unlistenPointer, unlistenLeaving];
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
      const version = liveStateVersion.current;
      const current = await tauriInvoke<string | null>("current_state");
      if (disposed || version !== liveStateVersion.current) return;
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
  // The late-answer note is all text and buttons, like an error: a bead opens into a pill for it.
  const needsText = state === "error" || state === "late" || (state === "pasted" && !!aiProblem);
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
        liveStateVersion.current++;
        sessionId.current = payload;
        setDictationKey(payload);
        setPreviewText("");
        // We do not overwrite the mark if it already arrived for this same
        // dictation: the order of these two events is not guaranteed.
        setArmedSession((current) => (current === payload ? current : null));
        setLeaving(false);
        setState("recording");
        resetDetails();
      }),
      subscribe<{ session_id?: number; remaining_seconds?: number }>("recording-limit", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        setLimitAt(Date.now() + (payload?.remaining_seconds ?? 0) * 1000);
      }),
      subscribe<number>("recording-stopped", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        liveStateVersion.current++;
        setPreviewText("");
        setArmedSession(null);
        setState("processing");
        setRecordingStoppedAt(Date.now());
      }),
      subscribe<number>("whisper-started", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        liveStateVersion.current++;
        setState("processing");
        setRecordingStoppedAt((current) => current ?? Date.now());
      }),
      // Decoded, not delivered. The payload is the raw whisper output —
      // local formatting and the LLM pass still have to run — so nothing
      // here may claim a length or an outcome. `paste-done` does that.
      subscribe<TranscriptionPayload>("whisper-done", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        liveStateVersion.current++;
        setState("done");
        setPastedLength(null);
        setAiProblem("");
        setDecodedAt(Date.now());
      }),
      // The text is in the window: the only moment a character count is
      // true, and the first moment the LLM outcome is known.
      subscribe<PastePayload>("paste-done", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        liveStateVersion.current++;
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
        liveStateVersion.current++;
        sessionId.current = null;
        setState("error");
        setDecodedAt(null);
        setErrorText(payload?.message ?? t("Не удалось вставить текст в активное окно."));
      }),
      subscribe<ErrorPayload>("whisper-failed", (payload) => {
        if (!belongsToCurrentSessionOrIsUnscoped(payload)) return;
        liveStateVersion.current++;
        sessionId.current = null;
        setState("error");
        setErrorText(
          payload?.message
            ?? t("Не удалось распознать речь. Откройте раздел «Модели» и убедитесь, что модель скачана."),
        );
      }),
      subscribe<ErrorPayload>("whisper-load-failed", (payload) => {
        liveStateVersion.current++;
        setState("error");
        setErrorText(
          payload?.message
            ?? t("Не удалось загрузить модель. Откройте раздел «Модели» и попробуйте снова."),
        );
      }),
      subscribe<unknown>("whisper-empty", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        liveStateVersion.current++;
        sessionId.current = null;
        setState(null);
        // Overlay hides via Rust's hide() call (subscribe_engine_events).
      }),
      subscribe<{ entry_id?: number }>("llm-late-answer", (payload) => {
        if (typeof payload?.entry_id === "number") setLateEntryId(payload.entry_id);
      }),
      subscribe<unknown>("whisper-cancelled", (payload) => {
        if (!belongsToCurrentSession(payload)) return;
        liveStateVersion.current++;
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
    state, config, preferences, layout, dictationKey, streaming, recordingStartedAt, recordingStoppedAt, limitAt,
    pastedLength, decodedAt, previewText, errorText, aiProblem, isClosing, leaving, hovered, setHovered, handleClose,
    lateEntryId,
  };
}

export type OverlaySession = ReturnType<typeof useOverlaySession>;
