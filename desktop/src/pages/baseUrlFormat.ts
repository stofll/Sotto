import { t } from "../i18n";

/**
 * Validation of an OpenAI-compatible Base URL before a profile exists.
 *
 * The address used to be checked only after the fact: `llmRouteBlocker` looks
 * at the already saved config, so a typo in the wizard turned into a profile
 * that silently processes nothing. Here the same question is asked while the
 * field is still on screen.
 *
 * Two levels, as with the API key. `error` is what cannot work at all: an empty
 * value, a string that is not a URL, a scheme other than http/https. Everything
 * about the shape of the path is `warn` — providers do not agree on it
 * (`/v1`, `/openai/v1`, `/inference/v1`, and Ollama serves `/v1` off a local
 * port), and refusing a working address because it does not look usual would be
 * worse than the typo we are trying to catch.
 *
 * `code` exists for tests: messages get translated and edited, and tying a test
 * to their text makes it go red on copy-editing.
 */
export type UrlCheckCode = "empty" | "malformed" | "scheme" | "endpoint" | "suffix";
export type UrlCheck = { level: "error" | "warn"; code: UrlCheckCode; message: string } | null;

/** Hosts that serve a model on this machine: there is no account behind them. */
const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "0.0.0.0", "::1", "[::1]"]);

/** The full path of a chat request. Pasted from the docs instead of its root. */
const CHAT_SUFFIXES = ["/chat/completions", "/completions", "/responses", "/messages"];

function parse(raw: string): URL | null {
  try {
    return new URL(raw.trim());
  } catch {
    return null;
  }
}

/** Trailing slashes off: providers are appended to this string, not joined. */
export function normalizeBaseUrl(raw: string): string {
  return raw.trim().replace(/\/+$/, "");
}

export function checkBaseUrl(raw: string): UrlCheck {
  const value = normalizeBaseUrl(raw);
  if (!value) return { level: "error", code: "empty", message: t("Введите Base URL.") };

  const url = parse(value);
  if (!url) {
    return { level: "error", code: "malformed", message: t("Не похоже на адрес. Пример: https://api.example.com/v1") };
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    return { level: "error", code: "scheme", message: t("Адрес должен начинаться с http:// или https://.") };
  }

  const path = url.pathname.replace(/\/+$/, "");
  if (CHAT_SUFFIXES.some((suffix) => path.endsWith(suffix))) {
    return { level: "warn", code: "endpoint", message: t("Похоже на адрес запроса, а не на корень API — путь до /v1 обычно достаточен.") };
  }
  if (!/\/v\d+$/.test(path)) {
    return { level: "warn", code: "suffix", message: t("Обычно адрес заканчивается на /v1 — сверьтесь с документацией провайдера.") };
  }
  return null;
}

/** Whether the value can be left behind. A warning is a hint, not a refusal. */
export function baseUrlBlocks(check: UrlCheck): boolean {
  return check?.level === "error";
}

/** A local server: it accepts any token, so the key step has nothing to ask. */
export function isLocalBaseUrl(raw: string): boolean {
  const url = parse(raw);
  if (!url) return false;
  return LOCAL_HOSTS.has(url.hostname) || url.hostname.endsWith(".local");
}

/** The default profile name for a hand-typed address: its host, `api.groq.com`
 *  rather than «OpenAI-compatible», which every such profile would be called. */
export function baseUrlLabel(raw: string): string {
  return parse(raw)?.host ?? "";
}
