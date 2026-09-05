import type { ModelInfo } from "../bridge/types";
import { getLocale, t, tPlural } from "../i18n";

// Shown only when the backend did not answer `list_models`. The values must
// match the catalog in `model.rs`, otherwise the fallback list lies about the
// size and requirements of the models.
const FALLBACK_MODELS: ModelInfo[] = [
  { id: "tiny", label: "Whisper tiny", size: "75 MB", ram: "~0.4 GB", downloaded: false, selected: false, loaded: false },
  { id: "base", label: "Whisper base", size: "142 MB", ram: "~0.6 GB", downloaded: false, selected: false, loaded: false },
  { id: "small", label: "Whisper small", size: "466 MB", ram: "~1.4 GB", downloaded: false, selected: false, loaded: false },
  { id: "medium", label: "Whisper medium", size: "785 MB", ram: "~2.3 GB", downloaded: false, selected: false, loaded: false },
  { id: "large-v3", label: "Whisper large-v3", size: "3.1 GB", ram: "~6.0 GB", downloaded: false, selected: false, loaded: false },
  { id: "turbo", label: "Whisper turbo", size: "834 MB", ram: "~1.7 GB", downloaded: false, selected: false, recommended: true, loaded: false },
];

const BUNDLE_FALLBACKS: ModelInfo[] = [
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
  // for a transient bridge failure; match the platforms that link Sherpa.
  const platform = typeof navigator === "undefined" ? "" : `${navigator.platform} ${navigator.userAgent}`;
  return /windows|mac/i.test(platform) ? [...FALLBACK_MODELS, ...BUNDLE_FALLBACKS] : FALLBACK_MODELS;
}

/**
 * The model's languages, if its list is closed.
 *
 * It arrives from the backend as its own field rather than being derived from
 * the engine or the family: the very same sherpa hosts the monolingual GigaAM,
 * the multilingual Parakeet and SenseVoice with five languages, while among the
 * Whispers English-only builds sit next to multilingual ones.
 */
export function modelLanguages(model: ModelInfo | undefined): string[] | null {
  const languages = model?.languages;
  return languages && languages.length ? languages : null;
}

/** Whether the model supports this language. `auto` always passes. */
export function supportsLanguage(model: ModelInfo | undefined, language: string): boolean {
  if (!language || language === "auto") return true;
  const languages = modelLanguages(model);
  return languages === null || languages.includes(language);
}

/**
 * The language the settings switch to when the chosen model does not support
 * the current one. `null` — no switch needed.
 */
export function fallbackLanguage(model: ModelInfo | undefined, language: string | undefined): string | null {
  if (supportsLanguage(model, language ?? "auto")) return null;
  return modelLanguages(model)?.[0] ?? null;
}

// The metadata is visible right on the card: this keeps the catalog dense while
// sparing you from hovering over every model just to compare them.
//
// «Потоковая» belongs here rather than as a badge next to the name: it is a
// model parameter just like size and language, whereas a badge reads as a state
// — something like "loaded" — and promises an event that does not exist.
export function modelMetadata(model: ModelInfo): string {
  const devices = model.cpu_only ? "CPU" : (model.compute_backend || "CPU/GPU");
  // Quantisation goes in brackets next to the size rather than as its own item:
  // it answers the question "why does it weigh that much", and reading the two
  // apart serves no purpose.
  const weight = model.quantization ? `${model.size} (${model.quantization})` : model.size;
  // Streaming does not belong here: it sits next to the languages, where the
  // properties of the model itself are gathered, while this line is the price
  // paid for it.
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

/** A download progress event exactly as it arrives from the backend. */
export type DownloadProgressEvent = { model?: string; downloaded?: number; total?: number | null };

/**
 * What to show in the toast for a progress event. `null` — show nothing.
 *
 * The name goes into the headline and the bytes into the second line: together
 * on one line they did not fit, and what got truncated was exactly what people
 * look at the toast for. `cancelling` suppresses the tail of events sent before
 * the downloader noticed the cancel flag: otherwise they would bring back
 * «Скачиваю…» and a cancel button on top of one already pressed.
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
    // There is one toast but there may be several downloads: without this tail
    // the others are invisible entirely, and the bar shows someone else's
    // progress.
    detail: others > 0 ? `${bytes} · ${t("ещё {p0}", { p0: others })}` : bytes,
    progress: total ? Math.min(100, Math.round((downloaded / total) * 100)) : null,
    cancelModel: event?.model,
  };
}

/**
 * The name of a language by its code, in the UI language.
 *
 * Taken from the platform rather than from our own table: there are close to a
 * hundred supported languages, and translating their names by hand is a
 * dictionary that will go stale before it is finished. The code is returned as
 * is when the platform does not know it.
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
 * The language caption in a model row.
 *
 * Up to four languages are listed by name — that answers the question "is mine
 * in there". Beyond that an enumeration stops being readable, so only a count
 * remains and the list itself moves into the hint and the filter.
 */
export function languageSummary(model: ModelInfo): string {
  const languages = modelLanguages(model);
  // A user's own file in the models folder: we know neither its architecture nor
  // its language, and passing a guess off as fact is worse than staying silent.
  if (!languages) return model.local ? t("Язык неизвестен") : t("Многоязычная");
  if (languages.length === 1) {
    if (languages[0] === "ru") return t("Только русский");
    if (languages[0] === "en") return t("Только английский");
    return languageName(languages[0]);
  }
  if (languages.length <= 4) return languages.map(languageName).join(", ");
  return tPlural(languages.length, ["{count} язык", "{count} языка", "{count} языков"]);
}

/** The model's full language list, for the hint. */
export function languageList(model: ModelInfo): string {
  return (modelLanguages(model) ?? []).map(languageName).join(", ");
}

export type NamedLanguage = { code: string; name: string };

/**
 * Codes into a list with names, deduplicated and sorted by the current locale.
 *
 * By name rather than by code: people scan by name, and the code beside it is
 * a clarification.
 */
export function namedLanguages(codes: Iterable<string>): NamedLanguage[] {
  return [...new Set(codes)]
    .map((code) => ({ code, name: languageName(code) }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * The languages worth filtering the catalog by.
 *
 * The union of every model's languages rather than a fixed "Russian plus
 * English" pair: the filter must cover whatever the catalog covers.
 */
export function catalogLanguages(models: ModelInfo[]): NamedLanguage[] {
  const codes: string[] = [];
  for (const model of models) {
    for (const code of modelLanguages(model) ?? []) codes.push(code);
  }
  return namedLanguages(codes);
}

/**
 * The languages worth offering for dictation.
 *
 * The model owns this list: it decides what it can transcribe, and a foreign
 * language comes out of it not as an error but as garbage. A model with no
 * declared list — a user's own file in the models folder — is no reason to
 * narrow the choice by guessing: we offer everything the catalog knows, and the
 * model itself is free to refuse.
 */
export function speechLanguages(model: ModelInfo | undefined, catalog: ModelInfo[]): NamedLanguage[] {
  const own = modelLanguages(model);
  return own ? namedLanguages(own) : catalogLanguages(catalog);
}

export type CatalogFilters = {
  query: string;
  /** The language code the model must support, or `all`. */
  language: string | "all";
  onlyDownloaded: boolean;
};

export const EMPTY_FILTERS: CatalogFilters = { query: "", language: "all", onlyDownloaded: false };

/**
 * Filtering models for the catalog page.
 *
 * The search covers both the name and the identifier: `turbo` and
 * `large-v3-turbo` are the same model, and a person may remember either name.
 * Case and surrounding whitespace mean nothing.
 */
export function filterModels(models: ModelInfo[], filters: CatalogFilters): ModelInfo[] {
  const query = filters.query.trim().toLowerCase();
  return models.filter((model) => {
    if (filters.onlyDownloaded && !model.downloaded && !model.local) return false;
    // The filter answers the question "what will transcribe my language", so
    // multilingual models pass any language filter.
    if (filters.language !== "all" && !supportsLanguage(model, filters.language)) return false;
    if (!query) return true;
    return `${model.label} ${model.id}`.toLowerCase().includes(query);
  });
}

/** Group heading: the family name from the backend, own files in their own. */
export function familyLabel(model: ModelInfo): string {
  return model.family ?? t("Свои файлы");
}

/**
 * Splitting the catalog into families.
 *
 * Family order follows their first appearance in the backend's list: that order
 * is meaningful, whereas re-sorting alphabetically would put GigaAM ahead of
 * Whisper for no reason at all. Within a family the loaded model floats to the
 * top, then the rest of the installed ones: the question "what do I already
 * have" is asked more often than "what else exists".
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
