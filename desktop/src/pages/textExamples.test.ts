import { afterEach, describe, expect, it, vi } from "vitest";
import { setLocale, type Locale } from "../i18n";
import { replacementExamples, textPreview } from "./textExamples";
import fixtures from "./textExamples.fixture.json";

afterEach(async () => { await setLocale("ru"); vi.unstubAllGlobals(); });

async function locale(value: Locale) {
  vi.stubGlobal("document", { documentElement: { lang: "ru" } });
  await setLocale(value);
}

describe("localized text examples", () => {
  it.each(fixtures)("uses the backend-verified $locale sample and rules", async (fixture) => {
    await locale(fixture.locale as Locale);
    expect(textPreview(null)).toBe(fixture.sample);
    expect(replacementExamples()).toEqual(fixture.rules);
    for (const [find] of replacementExamples()) expect(textPreview(null)).toContain(find);
  });

  it("switches untouched samples, but preserves edited, empty and sample-identical drafts", async () => {
    await locale("ru");
    const typedSample = textPreview(null);
    await locale("en");
    expect(textPreview(null)).not.toBe(typedSample);
    expect(textPreview(typedSample)).toBe(typedSample);
    expect(textPreview("мой собственный текст")).toBe("мой собственный текст");
    expect(textPreview("")).toBe("");
    await locale("ru");
    expect(textPreview("my own text")).toBe("my own text");
  });

  it("does not translate a rule already added from a suggestion", async () => {
    await locale("ru");
    const savedRule = [...replacementExamples()[0]];
    await locale("en");
    expect(savedRule).toEqual(fixtures[0].rules[0]);
    expect(replacementExamples()[0]).toEqual(fixtures[1].rules[0]);
  });
});
