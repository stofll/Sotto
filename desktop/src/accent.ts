import { t } from "./i18n";
import { parseRgb } from "./color";

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
  // Release CSS can shorten #ffffff to #fff.
  const rgb = parseRgb(hex);
  if (!rgb) throw new Error("Unsupported interface color");
  return rgb;
}
function hex(channel: number): string {
  return Math.round(Math.min(255, Math.max(0, channel))).toString(16).padStart(2, "0");
}
function mix(color: string, target: string, amount: number): string {
  const rgb = channels(color);
  const other = channels(target);
  return `#${rgb.map((channel, index) => hex(channel + (other[index] - channel) * amount)).join("")}`;
}
function luminance(color: string): number {
  const [r, g, b] = channels(color).map((channel) => {
    const value = channel / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

// Retain the hue where possible, but ensure text works on every supplied surface.
function readableColor(color: string, backgrounds: string[]): string {
  const surfaces = backgrounds.map(luminance);
  const contrast = (candidate: string) => {
    const light = luminance(candidate);
    return Math.min(...surfaces.map((surface) => (Math.max(light, surface) + 0.05) / (Math.min(light, surface) + 0.05)));
  };
  const target = contrast("#ffffff") > contrast("#000000") ? "#ffffff" : "#000000";
  for (let step = 0; step <= 100; step++) {
    const candidate = mix(color, target, step / 100);
    if (contrast(candidate) >= 4.5) return candidate;
  }
  return target;
}

export function accentVariables(color: string, surfaces: string[] = []): Record<string, string> {
  const [r, g, b] = channels(color);
  const hover = mix(color, "#ffffff", 0.12);
  const variables: Record<string, string> = {
    "--accent": color,
    "--accent-strong": hover,
    "--accent-ink": readableColor(luminance(color) > 0.179 ? mix(color, "#000000", 0.9) : mix(color, "#ffffff", 0.92), [color, hover]),
    "--accent-soft": `rgba(${r}, ${g}, ${b}, 0.14)`,
    "--accent-soft-2": `rgba(${r}, ${g}, ${b}, 0.26)`,
  };
  if (surfaces.length) {
    const backgrounds = surfaces.flatMap((surface) => [surface, mix(surface, color, 0.14), mix(surface, color, 0.26)]);
    variables["--accent-text"] = readableColor(color, backgrounds);
  }
  return variables;
}

export function applyAccent(color: string) {
  const root = document.documentElement;
  const style = getComputedStyle(root);
  const surfaces = [0, 1, 2, 3, 4, 5].map((index) => style.getPropertyValue(`--bg-${index}`).trim());
  for (const [name, value] of Object.entries(accentVariables(resolveAccent(color), surfaces))) {
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
