// What the overlay is entitled to assert in each phase of the cycle.
//
// The regression this file exists for: «N символов вставлено» was shown on
// `whisper-done`, that is before LLM processing and before insertion. With a slow
// model the overlay announced completion, hid itself after 1.8 s, and the text
// arrived some twenty seconds later — into nothing.

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
        // Even when the length is known from somewhere, in this phase it is not
        // about the insertion.
        expect(detail.kind).toBe("progress");
        expect(JSON.stringify(detail)).not.toContain("42");
    });

    // The numbers here are literal on purpose rather than derived from
    // POLISHING_LABEL_AFTER_MS: a test that imports the constant and counts from
    // it travels along with it and stops asserting anything. A mutation run
    // caught exactly that — zeroing the threshold did not fail the test.
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
        // Seconds round down: 7.4 s is "7 s", not "8 s".
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
