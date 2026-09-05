import { afterEach, describe, expect, it, vi } from "vitest";
import type { ModelInfo } from "../bridge/types";
import { catalogLanguages, downloadToastCopy, fallbackModels, familySections, filterModels, languageSummary, modelMetadata, speechLanguages, supportsLanguage } from "./modelCatalog";

describe("fallback catalog platforms", () => {
  afterEach(() => vi.unstubAllGlobals());

  it.each(["MacIntel", "Macintosh", "Windows NT 10.0"])("offers Sherpa on %s", (platform) => {
    vi.stubGlobal("navigator", { platform, userAgent: "" });
    expect(fallbackModels().some((model) => model.id === "gigaam-v3")).toBe(true);
  });

  it.each(["Linux x86_64", ""])("keeps unsupported platforms Whisper-only: %s", (platform) => {
    vi.stubGlobal("navigator", { platform, userAgent: "" });
    expect(fallbackModels().every((model) => model.engine !== "sherpa-onnx")).toBe(true);
  });
});

function model(patch: Partial<ModelInfo> & { id: string }): ModelInfo {
  return {
    label: patch.id,
    size: "1 GB",
    ram: "~2 GB",
    downloaded: false,
    selected: false,
    loaded: false,
    ...patch,
  } as ModelInfo;
}

const ALL: CatalogFiltersLike = { query: "", language: "all", onlyDownloaded: false };
type CatalogFiltersLike = Parameters<typeof filterModels>[1];

describe("model catalog filtering", () => {
  it("searches the id as well as the label", () => {
    // The label and the identifier differ on purpose: a person may remember the
    // model by its name from the docs rather than by what the list calls it.
    const models = [model({ id: "turbo", label: "Быстрая" }), model({ id: "gigaam-v3", label: "GigaAM v3" })];

    expect(filterModels(models, { ...ALL, query: "turbo" }).map((m) => m.id)).toEqual(["turbo"]);
    expect(filterModels(models, { ...ALL, query: "  GIGAAM " }).map((m) => m.id)).toEqual(["gigaam-v3"]);
  });

  it("filters by what a model can transcribe, not by what it is", () => {
    const models = [
      model({ id: "gigaam-v3", languages: ["ru"] }),
      model({ id: "small.en", languages: ["en"] }),
      model({ id: "sense-voice", languages: ["zh", "en", "ja", "ko", "yue"] }),
      model({ id: "turbo" }),
    ];

    // A multilingual model passes any language filter because it can do both; a
    // model with a closed list passes only its own.
    expect(filterModels(models, { ...ALL, language: "ru" }).map((m) => m.id)).toEqual(["gigaam-v3", "turbo"]);
    expect(filterModels(models, { ...ALL, language: "en" }).map((m) => m.id)).toEqual(["small.en", "sense-voice", "turbo"]);
  });

  it("counts a file found on disk as downloaded", () => {
    // A user's own file has no `downloaded` flag — it is not from the catalog,
    // yet it lies on disk and can be worked with.
    const models = [model({ id: "my-finetune", local: true }), model({ id: "small" })];

    expect(filterModels(models, { ...ALL, onlyDownloaded: true }).map((m) => m.id)).toEqual(["my-finetune"]);
  });

  it("does not guess the language of a file it knows nothing about", () => {
    expect(languageSummary(model({ id: "my-finetune", local: true }))).toBe("Язык неизвестен");
    expect(languageSummary(model({ id: "small" }))).toBe("Многоязычная");
    expect(languageSummary(model({ id: "gigaam-v3", languages: ["ru"] }))).toBe("Только русский");
    // Up to four languages are listed by name: that answers "is mine in there",
    // while "3 languages" does not.
    expect(languageSummary(model({ id: "sense-voice", languages: ["zh", "en", "ja"] })))
      .toBe("Китайский, Английский, Японский");
    expect(languageSummary(model({ id: "many", languages: ["ru", "en", "de", "fr", "it"] })))
      .toBe("5 языков");
  });

  it("treats auto as supported by every model", () => {
    const gigaam = model({ id: "gigaam-v3", languages: ["ru"] });
    expect(supportsLanguage(gigaam, "auto")).toBe(true);
    expect(supportsLanguage(gigaam, "en")).toBe(false);
  });
});

describe("catalog language list", () => {
  it("offers every language the catalogue can transcribe, by name", () => {
    // Japanese comes first in catalog order and second alphabetically: this way
    // the test sees the sorting itself rather than a coincidence of orders.
    const languages = catalogLanguages([
      model({ id: "sense-voice", languages: ["ja", "ru"] }),
      model({ id: "gigaam-v3", languages: ["ru"] }),
      model({ id: "own-file", local: true }),
    ]);

    // Duplicates are collapsed; the order is by name in the current locale, not
    // by code.
    expect(languages).toEqual([
      { code: "ru", name: "Русский" },
      { code: "ja", name: "Японский" },
    ]);
  });
});

describe("model catalog families", () => {
  it("keeps families in backend order and floats what is already on disk", () => {
    const sections = familySections([
      model({ id: "tiny", family: "Whisper" }),
      model({ id: "gigaam-v3", family: "GigaAM" }),
      model({ id: "turbo", family: "Whisper", downloaded: true }),
      model({ id: "large-v3", family: "Whisper", downloaded: true, loaded: true }),
      model({ id: "my-finetune", local: true }),
    ]);

    // Whisper comes first because it arrived first, not alphabetically.
    expect(sections.map((section) => section.family)).toEqual(["Whisper", "GigaAM", "Свои файлы"]);
    expect(sections[0].models.map((m) => m.id)).toEqual(["large-v3", "turbo", "tiny"]);
  });

  it("files a model without a family under the user's own files", () => {
    const sections = familySections([model({ id: "my-finetune", local: true })]);
    expect(sections.map((section) => section.family)).toEqual(["Свои файлы"]);
  });
});

describe("download toast copy", () => {
  it("keeps the byte counter out of the truncated headline", () => {
    const copy = downloadToastCopy(
      { model: "parakeet-streaming-en", downloaded: 128 * 1024 * 1024, total: 632 * 1024 * 1024 },
      "Parakeet Streaming EN",
      [],
    );

    // The bytes go on the second line and in full: a long name must not squeeze
    // them out, they are what the toast is looked at for.
    expect(copy?.detail).toBe("128 MB / 632 MB");
    expect(copy?.text).toContain("Parakeet Streaming EN");
    expect(copy?.progress).toBe(20);
    expect(copy?.cancelModel).toBe("parakeet-streaming-en");
  });

  it("shows what is downloaded when the server never said how much there is", () => {
    const copy = downloadToastCopy({ model: "tiny", downloaded: 5 * 1024 * 1024, total: null }, "Tiny", []);

    expect(copy?.detail).toBe("5 MB");
    expect(copy?.progress).toBeNull();
  });

  it("ignores the bytes still arriving after the user pressed cancel", () => {
    // The downloader does not notice the flag instantly, and these events would
    // bring back «Скачиваю…» and a cancel button on top of one already pressed.
    const copy = downloadToastCopy(
      { model: "turbo", downloaded: 1024 * 1024, total: 2 * 1024 * 1024 },
      "Turbo",
      ["turbo"],
    );

    expect(copy).toBeNull();
    // Cancelling one download does not silence another.
    expect(downloadToastCopy({ model: "tiny", downloaded: 1024 * 1024, total: null }, "Tiny", ["turbo"])).not.toBeNull();
  });
});

describe("model metadata", () => {
  it("puts the quantisation next to the weight and nothing about streaming", () => {
    // Quantisation answers "why does it weigh that much", and reading the two
    // apart serves no purpose. Streaming does not belong here: it sits next to
    // the languages, where the model's properties are gathered, while this is
    // the price paid for them.
    const streaming = model({ id: "zipformer-ru-streaming", size: "27 MB", ram: "~1 GB", cpu_only: true, streaming: true, quantization: "int8" });
    expect(modelMetadata(streaming)).toBe("27 MB (int8) · RAM ~1 GB · CPU");

    // A user's own file has no known quantisation, and brackets with emptiness
    // inside are worse than no brackets at all.
    const own = model({ id: "my-finetune", size: "75 MB", ram: "~1 GB", cpu_only: true, local: true });
    expect(modelMetadata(own)).toBe("75 MB · RAM ~1 GB · CPU");
  });
});

describe("several downloads at once", () => {
  it("names one model and says how many others are running", () => {
    // There is one toast but there may be several downloads: without this tail
    // the others are invisible entirely, and the bar shows someone else's
    // progress.
    const copy = downloadToastCopy(
      { model: "turbo", downloaded: 512 * 1024 * 1024, total: 1024 * 1024 * 1024 },
      "Turbo",
      [],
      2,
    );

    expect(copy?.text).toContain("Turbo");
    expect(copy?.detail).toBe("512 MB / 1.0 GB · ещё 2");
  });

  it("says nothing about others when there are none", () => {
    const copy = downloadToastCopy({ model: "turbo", downloaded: 1024 * 1024, total: null }, "Turbo", [], 0);
    expect(copy?.detail).toBe("1 MB");
  });
});

describe("speech languages", () => {
  const catalog = [
    model({ id: "sense-voice", languages: ["ja", "ru"] }),
    model({ id: "moonshine", languages: ["en"] }),
  ];

  it("offers exactly what the loaded model can transcribe", () => {
    // A foreign language comes out of a model as garbage rather than an error,
    // so the list offers its own — alphabetically, not in manifest order.
    const picked = speechLanguages(model({ id: "parakeet", languages: ["ru", "de"] }), catalog);

    expect(picked).toEqual([
      { code: "de", name: "Немецкий" },
      { code: "ru", name: "Русский" },
    ]);
  });

  it("does not narrow the choice for a model that declares nothing", () => {
    // A user's own file in the models folder: we do not know what it can do, and
    // a guess here would take away languages the model understands perfectly
    // well.
    const picked = speechLanguages(model({ id: "my-finetune", local: true }), catalog);

    expect(picked.map((item) => item.code)).toEqual(["en", "ru", "ja"]);
  });
});
