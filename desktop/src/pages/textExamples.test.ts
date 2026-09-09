import { afterEach, describe, expect, it, vi } from "vitest";
import { setLocale, type Locale } from "../i18n";
import { replacementExamples, textPreview } from "./textExamples";
import fixtures from "./textExamples.fixture.json";

afterEach(() => { setLocale("ru"); vi.unstubAllGlobals(); });

function locale(value: Locale) {
  vi.stubGlobal("document", { documentElement: { lang: "ru" } });
  setLocale(value);
}

describe("localized text examples", () => {
  it.each(fixtures)("uses the backend-verified $locale sample and rules", (fixture) => {
    locale(fixture.locale as Locale);
    expect(textPreview(null)).toBe(fixture.sample);
    expect(replacementExamples()).toEqual(fixture.rules);
    for (const [find] of replacementExamples()) expect(textPreview(null)).toContain(find);
  });

  it("switches untouched samples, but preserves edited, empty and sample-identical drafts", () => {
    locale("ru");
    const typedSample = textPreview(null);
    locale("en");
    expect(textPreview(null)).not.toBe(typedSample);
    expect(textPreview(typedSample)).toBe(typedSample);
    expect(textPreview("мой собственный текст")).toBe("мой собственный текст");
    expect(textPreview("")).toBe("");
    locale("ru");
    expect(textPreview("my own text")).toBe("my own text");
  });

  it("does not translate a rule already added from a suggestion", () => {
    locale("ru");
    const savedRule = [...replacementExamples()[0]];
    locale("en");
    expect(savedRule).toEqual(fixtures[0].rules[0]);
    expect(replacementExamples()[0]).toEqual(fixtures[1].rules[0]);
  });
});
