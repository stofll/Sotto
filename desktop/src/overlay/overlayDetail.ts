// Что оверлей показывает во второй строке — отдельно от того, как он это
// рисует.
//
// Вынесено из OverlayApp, потому что сломалось именно решение, а не вёрстка:
// «N символов вставлено» показывалось по событию `whisper-done`, то есть до
// LLM-обработки и до вставки. Число считалось по черновику, а сама надпись
// утверждала то, чего ещё не произошло. Здесь это решение можно проверить
// тестом — тесты фронтенда идут без DOM, отрендерить компонент нечем.

import { t } from "../i18n";

/// Через сколько «Распознано» перестаёт молчать и признаётся, что чего-то
/// ждёт. Быстрый путь укладывается в этот интервал целиком, и подпись,
/// показанная на 200 мс, была бы просто миганием.
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
    /// Длина реально вставленного текста из `paste-done`. `null` — событие
    /// пришло без неё; тогда о вставке говорим без числа, но не молчим.
    pastedLength: number | null;
    /// Сколько прошло с `whisper-done`. Имеет смысл только в состоянии `done`.
    polishingMs: number;
    errorText: string;
    /// Проблема LLM (фолбэк на локальный текст). Известна не раньше вставки.
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

    // Распознано, но ещё не вставлено. На быстром пути это доли секунды; когда
    // текст чистит LLM — десятки секунд, и молчание об этом читалось как
    // «цикл закончился, текст потерялся».
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
