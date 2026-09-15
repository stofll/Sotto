import { t } from "./i18n";

// A function rather than a constant: the labels are translated, and computed at
// import time they would be stuck in the default language. The colour values
// stay literal types, so AccentValue is still a union of hex strings.
export const ACCENT_OPTIONS = () => ([
  { value: "#e68a3d", strong: "#f5993f", ink: "#1a1208", label: t("Оранжевый") },
  { value: "#5b8def", strong: "#6f9bf3", ink: "#091226", label: t("Синий") },
  { value: "#3dc97c", strong: "#4ed688", ink: "#082416", label: t("Зелёный") },
  { value: "#9b75ef", strong: "#a886f3", ink: "#180a2c", label: t("Фиолетовый") },
] as const);
export type AccentValue = ReturnType<typeof ACCENT_OPTIONS>[number]["value"];

export function applyAccent(hex: string) {
  const options = ACCENT_OPTIONS();
  const opt = options.find((o) => o.value.toLowerCase() === hex.toLowerCase()) ?? options[0];
  const root = document.documentElement;
  root.style.setProperty("--accent", opt.value);
  root.style.setProperty("--accent-strong", opt.strong);
  root.style.setProperty("--accent-ink", opt.ink);
  const m = opt.value.match(/^#(.{2})(.{2})(.{2})$/);
  if (m) {
    const r = parseInt(m[1], 16), g = parseInt(m[2], 16), b = parseInt(m[3], 16);
    root.style.setProperty("--accent-soft", `rgba(${r}, ${g}, ${b}, 0.14)`);
    root.style.setProperty("--accent-soft-2", `rgba(${r}, ${g}, ${b}, 0.26)`);
  }
}

export function resolveAccent(value: unknown): AccentValue {
  return ACCENT_OPTIONS().find((option) => option.value === value)?.value ?? "#e68a3d";
}

export function storedAccent(): AccentValue {
  try { return resolveAccent(window.localStorage.getItem("sotto.ui.accent")?.toLowerCase()); }
  catch { return resolveAccent(undefined); }
}
