import type { ModelInfo } from "../bridge/types";
import { getLocale, t, tPlural } from "../i18n";

// Показывается, только когда бэкенд не ответил на `list_models`. Значения
// обязаны совпадать с каталогом в `model.rs`, иначе резервный список врёт о
// весе и требованиях моделей.
const FALLBACK_MODELS: ModelInfo[] = [
  { id: "tiny", label: "Whisper tiny", size: "75 MB", ram: "~0.4 GB", downloaded: false, selected: false, loaded: false },
  { id: "base", label: "Whisper base", size: "142 MB", ram: "~0.6 GB", downloaded: false, selected: false, loaded: false },
  { id: "small", label: "Whisper small", size: "466 MB", ram: "~1.4 GB", downloaded: false, selected: false, loaded: false },
  { id: "medium", label: "Whisper medium", size: "785 MB", ram: "~2.3 GB", downloaded: false, selected: false, loaded: false },
  { id: "large-v3", label: "Whisper large-v3", size: "3.1 GB", ram: "~6.0 GB", downloaded: false, selected: false, loaded: false },
  { id: "turbo", label: "Whisper turbo", size: "834 MB", ram: "~1.7 GB", downloaded: false, selected: false, recommended: true, loaded: false },
];

const WINDOWS_BUNDLE_FALLBACKS: ModelInfo[] = [
  {
    id: "gigaam-v3",
    label: "GigaAM v3",
    size: "214 MB",
    ram: "~0.5 GB",
    downloaded: false,
    selected: false,
    loaded: false,
    engine: "sherpa-onnx",
    compute_backend: "CPU",
    cpu_only: true,
    family: "GigaAM",
    languages: ["ru"],
  },
  {
    id: "parakeet-tdt-v3",
    label: "Parakeet TDT v3",
    size: "639 MB",
    ram: "~1.4 GB",
    downloaded: false,
    selected: false,
    loaded: false,
    engine: "sherpa-onnx",
    compute_backend: "CPU",
    cpu_only: true,
    family: "Parakeet",
    languages: null,
  },
];

export function fallbackModels(): ModelInfo[] {
  // The real list comes from Rust and is authoritative. This fallback is only
  // for a transient bridge failure; do not show a Windows-only model on a Mac
  // build where Sherpa is intentionally not linked.
  const platform = typeof navigator === "undefined" ? "" : `${navigator.platform} ${navigator.userAgent}`;
  return /windows/i.test(platform) ? [...FALLBACK_MODELS, ...WINDOWS_BUNDLE_FALLBACKS] : FALLBACK_MODELS;
}

/**
 * Языки модели, если их список закрыт.
 *
 * Приходит из бэкенда отдельным полем, а не выводится из движка или семейства:
 * на одном и том же sherpa живут одноязычный GigaAM, многоязычный Parakeet и
 * SenseVoice с пятью языками, а среди Whisper'ов английские сборки соседствуют
 * с многоязычными.
 */
export function modelLanguages(model: ModelInfo | undefined): string[] | null {
  const languages = model?.languages;
  return languages && languages.length ? languages : null;
}

/** Умеет ли модель этот язык. `auto` проходит всегда. */
export function supportsLanguage(model: ModelInfo | undefined, language: string): boolean {
  if (!language || language === "auto") return true;
  const languages = modelLanguages(model);
  return languages === null || languages.includes(language);
}

/**
 * Язык, на который переключаются настройки, когда выбранная модель не умеет
 * текущий. `null` — переключать не нужно.
 */
export function fallbackLanguage(model: ModelInfo | undefined, language: string | undefined): string | null {
  if (supportsLanguage(model, language ?? "auto")) return null;
  return modelLanguages(model)?.[0] ?? null;
}

// Метаданные видны прямо в карточке: так каталог остаётся плотным, но для
// сравнения моделей не приходится наводить курсор на каждую.
//
// «Потоковая» стоит здесь, а не значком у названия: это такой же параметр
// модели, как вес и язык, а значок читается как состояние — вроде
// «загружена» — и обещает событие, которого нет.
export function modelMetadata(model: ModelInfo): string {
  const devices = model.cpu_only ? "CPU" : (model.compute_backend || "CPU/GPU");
  // Квантование в скобках при весе, а не отдельным пунктом: это ответ на
  // вопрос «почему столько весит», и врозь их читать незачем.
  const weight = model.quantization ? `${model.size} (${model.quantization})` : model.size;
  // Потоковость сюда не входит: она стоит рядом с языками, где собраны
  // свойства самой модели, а здесь — цена, которую за неё платят.
  return [
    weight,
    model.ram ? `RAM ${model.ram}` : null,
    devices,
  ].filter(Boolean).join(" · ");
}

export function formatDownloadBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${Math.round(bytes / 1024 / 1024)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
}

/** Событие прогресса скачивания, как оно приходит из бэкенда. */
export type DownloadProgressEvent = { model?: string; downloaded?: number; total?: number | null };

/**
 * Что показать в тосте по событию прогресса. `null` — не показывать ничего.
 *
 * Название уходит в заголовок, а байты — во вторую строку: вместе в одной
 * строке они не помещались, и обрезалось ровно то, ради чего на тост
 * смотрят. `cancelling` гасит хвост событий, отправленных до того, как
 * скачиватель заметил флаг отмены: иначе они возвращали бы «Скачиваю…» и
 * кнопку отмены поверх уже нажатой.
 */
export function downloadToastCopy(
  event: DownloadProgressEvent | undefined,
  label: string,
  cancelling: readonly string[],
  others = 0,
): { text: string; detail: string; progress: number | null; cancelModel?: string } | null {
  if (event?.model && cancelling.includes(event.model)) return null;
  const downloaded = Number(event?.downloaded) || 0;
  const total = typeof event?.total === "number" && event.total > 0 ? event.total : null;
  const bytes = total
    ? `${formatDownloadBytes(downloaded)} / ${formatDownloadBytes(total)}`
    : formatDownloadBytes(downloaded);
  return {
    text: t("Скачиваю {p0}", { p0: label }),
    // Тост один, а загрузок может идти несколько: без этого хвоста
    // остальные не видно вовсе, а полоска показывает чужой прогресс.
    detail: others > 0 ? `${bytes} · ${t("ещё {p0}", { p0: others })}` : bytes,
    progress: total ? Math.min(100, Math.round((downloaded / total) * 100)) : null,
    cancelModel: event?.model,
  };
}

/**
 * Название языка по коду на языке интерфейса.
 *
 * Берётся у платформы, а не из своей таблицы: поддерживаемых языков под сотню,
 * и переводить их названия руками — это словарь, который устареет раньше, чем
 * будет дописан. Код возвращается как есть, если платформа его не знает.
 */
export function languageName(code: string): string {
  try {
    const names = new Intl.DisplayNames([getLocale()], { type: "language" });
    const name = names.of(code);
    if (!name || name === code) return code.toUpperCase();
    return name.charAt(0).toUpperCase() + name.slice(1);
  } catch {
    return code.toUpperCase();
  }
}

/**
 * Языковая подпись в строке модели.
 *
 * До четырёх языков перечисляются поимённо — это ответ на вопрос «есть ли там
 * мой». Дальше перечисление перестаёт читаться, и остаётся счёт, а сам список
 * уезжает в подсказку и в фильтр.
 */
export function languageSummary(model: ModelInfo): string {
  const languages = modelLanguages(model);
  // Свой файл в папке моделей: архитектуры и языка мы про него не знаем, и
  // выдавать догадку за факт хуже, чем промолчать.
  if (!languages) return model.local ? t("Язык неизвестен") : t("Многоязычная");
  if (languages.length === 1) {
    if (languages[0] === "ru") return t("Только русский");
    if (languages[0] === "en") return t("Только английский");
    return languageName(languages[0]);
  }
  if (languages.length <= 4) return languages.map(languageName).join(", ");
  return tPlural(languages.length, ["{count} язык", "{count} языка", "{count} языков"]);
}

/** Полный список языков модели для подсказки. */
export function languageList(model: ModelInfo): string {
  return (modelLanguages(model) ?? []).map(languageName).join(", ");
}

export type NamedLanguage = { code: string; name: string };

/**
 * Коды — в список с названиями, без повторов и по алфавиту текущей локали.
 *
 * По названию, а не по коду: искать глазами будут по названию, а код рядом с
 * ним — уточнение.
 */
export function namedLanguages(codes: Iterable<string>): NamedLanguage[] {
  return [...new Set(codes)]
    .map((code) => ({ code, name: languageName(code) }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * Языки, по которым имеет смысл фильтровать каталог.
 *
 * Объединение языков всех моделей, а не фиксированная пара «русский плюс
 * английский»: фильтр обязан уметь то же, что умеет каталог.
 */
export function catalogLanguages(models: ModelInfo[]): NamedLanguage[] {
  const codes: string[] = [];
  for (const model of models) {
    for (const code of modelLanguages(model) ?? []) codes.push(code);
  }
  return namedLanguages(codes);
}

/**
 * Языки, которые имеет смысл предлагать для диктовки.
 *
 * Список берёт на себя модель: она и решает, что сумеет расшифровать, а
 * чужой язык у неё выходит не ошибкой, а мусором. Модель без объявленного
 * списка — свой файл в папке моделей — не повод сужать выбор догадкой:
 * предлагаем всё, что знает каталог, а отказать сможет сама модель.
 */
export function speechLanguages(model: ModelInfo | undefined, catalog: ModelInfo[]): NamedLanguage[] {
  const own = modelLanguages(model);
  return own ? namedLanguages(own) : catalogLanguages(catalog);
}

export type CatalogFilters = {
  query: string;
  /** Код языка, который модель обязана уметь, либо `all`. */
  language: string | "all";
  onlyDownloaded: boolean;
};

export const EMPTY_FILTERS: CatalogFilters = { query: "", language: "all", onlyDownloaded: false };

/**
 * Отбор моделей для страницы каталога.
 *
 * Поиск идёт по названию и по идентификатору: `turbo` и `large-v3-turbo` —
 * это одна и та же модель, и человек может помнить любое из имён. Регистр и
 * окружающие пробелы не значат ничего.
 */
export function filterModels(models: ModelInfo[], filters: CatalogFilters): ModelInfo[] {
  const query = filters.query.trim().toLowerCase();
  return models.filter((model) => {
    if (filters.onlyDownloaded && !model.downloaded && !model.local) return false;
    // Фильтр отвечает на вопрос «что распознает мой язык», поэтому
    // многоязычные модели проходят любой языковой фильтр.
    if (filters.language !== "all" && !supportsLanguage(model, filters.language)) return false;
    if (!query) return true;
    return `${model.label} ${model.id}`.toLowerCase().includes(query);
  });
}

/** Заголовок группы: имя семейства с бэкенда, свои файлы — отдельной. */
export function familyLabel(model: ModelInfo): string {
  return model.family ?? t("Свои файлы");
}

/**
 * Разбивка каталога по семействам.
 *
 * Порядок семейств — порядок их первого появления в списке от бэкенда: он
 * там осмысленный, а пересортировка по алфавиту ставила бы GigaAM перед
 * Whisper без всякой причины. Внутри семейства наверх всплывает загруженная
 * модель, затем остальные установленные: вопрос «что у меня уже есть»
 * задаётся чаще, чем «что ещё бывает».
 */
export function familySections<T extends ModelInfo>(models: T[]): Array<{ family: string; models: T[] }> {
  const order: string[] = [];
  const byFamily = new Map<string, T[]>();
  for (const model of models) {
    const family = familyLabel(model);
    if (!byFamily.has(family)) {
      byFamily.set(family, []);
      order.push(family);
    }
    byFamily.get(family)!.push(model);
  }
  return order.map((family) => ({
    family,
    models: [...byFamily.get(family)!].sort((a, b) => rank(a) - rank(b)),
  }));
}

function rank(model: ModelInfo): number {
  if (model.loaded) return 0;
  if (model.downloaded || model.local) return 1;
  return 2;
}
