// What the overlay shows on its second line — kept apart from how it draws it.
//
// Split out of OverlayApp because what broke was the decision, not the markup:
// «N символов вставлено» was shown on the `whisper-done` event, that is before
// LLM processing and before insertion. The number was counted from the draft
// while the caption asserted something that had not happened yet. Here the
// decision can be covered by a test — frontend tests run without a DOM, there is
// nothing to render the component with.

import { t } from "../i18n";

/// How long before «Распознано» stops keeping quiet and admits it is waiting
/// for something. The fast path fits entirely inside this interval, and a
/// caption shown for 200 ms would be nothing but a flicker.
export const POLISHING_LABEL_AFTER_MS = 600;

export type OverlayDetailState =
    | "recording"
    | "processing"
    | "loading"
    | "done"
    | "pasted"
    | "error";

export type OverlayDetail =
    | { kind: "waveform" }
    | { kind: "progress"; label?: string }
    | { kind: "text"; text: string }
    | { kind: "text"; text: string; warning: string };

export interface OverlayDetailInput {
    state: OverlayDetailState;
    /// The length of the text actually inserted, from `paste-done`. `null` — the
    /// event arrived without it; then we mention the insertion without a number
    /// rather than stay silent.
    pastedLength: number | null;
    /// Time elapsed since `whisper-done`. Meaningful only in the `done` state.
    polishingMs: number;
    errorText: string;
    /// An LLM problem (fallback to local text). Not known before insertion.
    aiProblem: string;
}

export function overlayDetail(input: OverlayDetailInput): OverlayDetail {
    const { state, pastedLength, polishingMs, errorText, aiProblem } = input;

    if (state === "recording") return { kind: "waveform" };
    if (state === "processing") return { kind: "progress" };
    if (state === "loading") {
        return { kind: "progress", label: t("Подготавливаю локальную модель") };
    }
    if (state === "error") {
        return { kind: "text", text: errorText || t("Запись не была обработана") };
    }

    // Transcribed but not inserted yet. On the fast path this is a fraction of a
    // second; when the LLM cleans the text it is tens of seconds, and staying
    // quiet about it read as "the cycle finished and the text was lost".
    if (state === "done") {
        return polishingMs < POLISHING_LABEL_AFTER_MS
            ? { kind: "progress" }
            : {
                kind: "progress",
                label: t("Обрабатываю текст · {p0} с", { p0: Math.floor(polishingMs / 1000) }),
            };
    }

    const text = pastedLength === null
        ? t("Текст вставлен")
        : t("{p0} символов вставлено", { p0: pastedLength });
    return aiProblem ? { kind: "text", text, warning: aiProblem } : { kind: "text", text };
}
