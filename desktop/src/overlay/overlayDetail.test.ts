// Что оверлей вправе утверждать в каждой фазе цикла.
//
// Регрессия, ради которой этот файл существует: «N символов вставлено»
// показывалось по `whisper-done`, то есть до LLM-обработки и до вставки.
// При медленной модели оверлей объявлял готовность, прятался через 1.8 с,
// а текст приезжал секунд через двадцать — в пустоту.

import { describe, expect, it } from "vitest";
import { overlayDetail } from "./overlayDetail";

const base = {
    pastedLength: null as number | null,
    polishingMs: 0,
    errorText: "",
    aiProblem: "",
};

describe("overlayDetail", () => {
    it("ничего не обещает, пока текст только распознан", () => {
        const detail = overlayDetail({ ...base, state: "done", pastedLength: 42 });
        // Даже если длина откуда-то известна — в этой фазе она не про вставку.
        expect(detail.kind).toBe("progress");
        expect(JSON.stringify(detail)).not.toContain("42");
    });

    // Числа здесь намеренно литеральные, а не выведенные из
    // POLISHING_LABEL_AFTER_MS: тест, который импортирует константу и
    // считает от неё, переезжает вместе с ней и перестаёт что-либо
    // утверждать. Мутационный прогон ловил ровно это — обнуление порога
    // не роняло тест.
    it("на быстром пути не мигает подписью про обработку", () => {
        const detail = overlayDetail({ ...base, state: "done", polishingMs: 200 });
        expect(detail).toEqual({ kind: "progress" });
    });

    it("через полсекунды с лишним подпись уже появляется", () => {
        const detail = overlayDetail({ ...base, state: "done", polishingMs: 900 });
        expect(detail.kind).toBe("progress");
        expect("label" in detail && detail.label).toBeTruthy();
    });

    it("на медленной модели показывает, сколько уже ждёт", () => {
        const detail = overlayDetail({ ...base, state: "done", polishingMs: 7400 });
        expect(detail.kind).toBe("progress");
        // Секунды округляются вниз: 7.4 с — это «7 с», а не «8 с».
        expect("label" in detail && detail.label).toContain("7");
    });

    it("счётчик символов появляется только после вставки", () => {
        const detail = overlayDetail({ ...base, state: "pasted", pastedLength: 128 });
        expect(detail.kind).toBe("text");
        expect("text" in detail && detail.text).toContain("128");
    });

    it("вставка без длины в событии не молчит, а говорит без числа", () => {
        const detail = overlayDetail({ ...base, state: "pasted", pastedLength: null });
        expect(detail.kind).toBe("text");
        expect("text" in detail && detail.text.length).toBeGreaterThan(0);
    });

    it("нулевая длина — это ноль, а не «неизвестно»", () => {
        const detail = overlayDetail({ ...base, state: "pasted", pastedLength: 0 });
        expect("text" in detail && detail.text).toContain("0");
    });

    it("проблема LLM показывается рядом со вставкой, не вместо неё", () => {
        const detail = overlayDetail({
            ...base,
            state: "pasted",
            pastedLength: 10,
            aiProblem: "Лимит LLM",
        });
        expect("text" in detail && detail.text).toContain("10");
        expect("warning" in detail && detail.warning).toBe("Лимит LLM");
    });

    it("ошибка вытесняет всё остальное", () => {
        const detail = overlayDetail({
            ...base,
            state: "error",
            errorText: "Модель не загрузилась",
        });
        expect(detail).toEqual({ kind: "text", text: "Модель не загрузилась" });
    });

    it("ошибка без текста всё равно объясняет, что произошло", () => {
        const detail = overlayDetail({ ...base, state: "error" });
        expect("text" in detail && detail.text.length).toBeGreaterThan(0);
    });

    it("запись рисует уровень, распознавание — полосу", () => {
        expect(overlayDetail({ ...base, state: "recording" })).toEqual({ kind: "waveform" });
        expect(overlayDetail({ ...base, state: "processing" })).toEqual({ kind: "progress" });
    });
});
