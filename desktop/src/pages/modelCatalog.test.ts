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
    // Подпись и идентификатор расходятся намеренно: человек может помнить
    // модель по имени из документации, а не по тому, как её назвали в списке.
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

    // Многоязычная модель проходит любой языковой фильтр, потому что умеет
    // и то и другое; модель с закрытым списком — только свой.
    expect(filterModels(models, { ...ALL, language: "ru" }).map((m) => m.id)).toEqual(["gigaam-v3", "turbo"]);
    expect(filterModels(models, { ...ALL, language: "en" }).map((m) => m.id)).toEqual(["small.en", "sense-voice", "turbo"]);
  });

  it("counts a file found on disk as downloaded", () => {
    // У своего файла `downloaded` не выставлен — он не из каталога, но лежит
    // на диске и работать с ним можно.
    const models = [model({ id: "my-finetune", local: true }), model({ id: "small" })];

    expect(filterModels(models, { ...ALL, onlyDownloaded: true }).map((m) => m.id)).toEqual(["my-finetune"]);
  });

  it("does not guess the language of a file it knows nothing about", () => {
    expect(languageSummary(model({ id: "my-finetune", local: true }))).toBe("Язык неизвестен");
    expect(languageSummary(model({ id: "small" }))).toBe("Многоязычная");
    expect(languageSummary(model({ id: "gigaam-v3", languages: ["ru"] }))).toBe("Только русский");
    // До четырёх языков перечисляем поимённо: это ответ на вопрос «есть ли
    // там мой», а «3 языка» — нет.
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
    // Японский идёт первым по порядку в каталоге и вторым по алфавиту:
    // так тест видит саму сортировку, а не совпадение порядков.
    const languages = catalogLanguages([
      model({ id: "sense-voice", languages: ["ja", "ru"] }),
      model({ id: "gigaam-v3", languages: ["ru"] }),
      model({ id: "own-file", local: true }),
    ]);

    // Дубли схлопнуты, порядок — по названию в текущей локали, а не по коду.
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

    // Whisper первым, потому что первым пришёл, а не по алфавиту.
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

    // Байты — во второй строке и целиком: длинное название не должно их
    // выдавливать, ради них на тост и смотрят.
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
    // Скачиватель замечает флаг не мгновенно, и эти события вернули бы
    // «Скачиваю…» и кнопку отмены поверх уже нажатой.
    const copy = downloadToastCopy(
      { model: "turbo", downloaded: 1024 * 1024, total: 2 * 1024 * 1024 },
      "Turbo",
      ["turbo"],
    );

    expect(copy).toBeNull();
    // Отмена одной загрузки не глушит другую.
    expect(downloadToastCopy({ model: "tiny", downloaded: 1024 * 1024, total: null }, "Tiny", ["turbo"])).not.toBeNull();
  });
});

describe("model metadata", () => {
  it("puts the quantisation next to the weight and nothing about streaming", () => {
    // Квантование — ответ на вопрос «почему столько весит», врозь их читать
    // незачем. Потоковость сюда не входит: она стоит рядом с языками, где
    // собраны свойства модели, а здесь — цена, которую за них платят.
    const streaming = model({ id: "zipformer-ru-streaming", size: "27 MB", ram: "~1 GB", cpu_only: true, streaming: true, quantization: "int8" });
    expect(modelMetadata(streaming)).toBe("27 MB (int8) · RAM ~1 GB · CPU");

    // Своему файлу квантование неизвестно, и скобки с пустотой внутри хуже
    // отсутствия скобок.
    const own = model({ id: "my-finetune", size: "75 MB", ram: "~1 GB", cpu_only: true, local: true });
    expect(modelMetadata(own)).toBe("75 MB · RAM ~1 GB · CPU");
  });
});

describe("several downloads at once", () => {
  it("names one model and says how many others are running", () => {
    // Тост один, а загрузок может идти несколько: без этого хвоста
    // остальные не видно вовсе, а полоска показывает чужой прогресс.
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
    // Чужой язык у модели выходит не ошибкой, а мусором, поэтому список
    // предлагает её собственный — и по алфавиту, а не в порядке манифеста.
    const picked = speechLanguages(model({ id: "parakeet", languages: ["ru", "de"] }), catalog);

    expect(picked).toEqual([
      { code: "de", name: "Немецкий" },
      { code: "ru", name: "Русский" },
    ]);
  });

  it("does not narrow the choice for a model that declares nothing", () => {
    // Свой файл в папке моделей: чего он умеет, мы не знаем, и догадка тут
    // отняла бы у пользователя языки, которые модель прекрасно понимает.
    const picked = speechLanguages(model({ id: "my-finetune", local: true }), catalog);

    expect(picked.map((item) => item.code)).toEqual(["en", "ru", "ja"]);
  });
});
