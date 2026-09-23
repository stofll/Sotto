import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// A fresh module per test: the English strings are cached once loaded.
async function i18n() {
  vi.resetModules();
  return import(".");
}

beforeEach(() => { vi.stubGlobal("document", { documentElement: { lang: "ru" } }); });
afterEach(() => { vi.unstubAllGlobals(); });

describe("interface locale", () => {
  it("applies English only once its strings have loaded", async () => {
    const { getLocale, setLocale, t } = await i18n();
    const switching = setLocale("en");
    expect(getLocale()).toBe("ru");
    expect(t("Сохранить")).toBe("Сохранить");
    await switching;
    expect(getLocale()).toBe("en");
    expect(t("Сохранить")).toBe("Save");
  });

  it("keeps the latest choice when English arrives after a switch back", async () => {
    const { getLocale, setLocale } = await i18n();
    const english = setLocale("en");
    await setLocale("ru");
    await english;
    expect(getLocale()).toBe("ru");
  });
});
