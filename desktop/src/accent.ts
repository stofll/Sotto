import { t } from "./i18n";

export const DEFAULT_ACCENT = "#e68a3d";

// A function rather than a constant: the labels are translated, and computed at
// import time they would be stuck in the default language.
export const ACCENT_PRESETS = () => ([
  { value: DEFAULT_ACCENT, label: t("Оранжевый") },
  { value: "#5b8def", label: t("Синий") },
  { value: "#3dc97c", label: t("Зелёный") },
  { value: "#9b75ef", label: t("Фиолетовый") },
]);
/** Any `#rrggbb`: the interface colour is free-form, the presets are shortcuts. */
export type AccentValue = string;

function channels(hex: string): [number, number, number] {
  const match = hex.match(/^#(.{2})(.{2})(.{2})$/)!;
  return [parseInt(match[1], 16), parseInt(match[2], 16), parseInt(match[3], 16)];
}
function hex(channel: number): string {
  return Math.round(Math.min(255, Math.max(0, channel))).toString(16).padStart(2, "0");
}
function mix(color: string, target: 0 | 255, amount: number): string {
  const [r, g, b] = channels(color);
  const towards = (channel: number) => channel + (target - channel) * amount;
  return `#${hex(towards(r))}${hex(towards(g))}${hex(towards(b))}`;
}
/** sRGB luminance, enough to tell «dark colour» from «light» for a text choice. */
function luminance(color: string): number {
  const [r, g, b] = channels(color);
  return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
}

// The four presets used to carry hand-picked companion colours. With a
// free-form colour there is nobody to hand-pick them, so they are derived:
// `strong` is the same colour a touch lighter for hover, and `ink` is the text
// written *on* the accent — near-black on a light accent, near-white on a dark
// one, so a navy or a lemon chosen in the picker both stay readable.
export function accentVariables(color: string): Record<string, string> {
  const [r, g, b] = channels(color);
  return {
    "--accent": color,
    "--accent-strong": mix(color, 255, 0.12),
    "--accent-ink": luminance(color) > 0.45 ? mix(color, 0, 0.9) : mix(color, 255, 0.92),
    "--accent-soft": `rgba(${r}, ${g}, ${b}, 0.14)`,
    "--accent-soft-2": `rgba(${r}, ${g}, ${b}, 0.26)`,
  };
}

export function applyAccent(color: string) {
  const root = document.documentElement;
  for (const [name, value] of Object.entries(accentVariables(resolveAccent(color)))) {
    root.style.setProperty(name, value);
  }
}

export function resolveAccent(value: unknown): AccentValue {
  return typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value) ? value.toLowerCase() : DEFAULT_ACCENT;
}

export function storedAccent(): AccentValue {
  try { return resolveAccent(window.localStorage.getItem("sotto.ui.accent")); }
  catch { return resolveAccent(undefined); }
}
