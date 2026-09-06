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

/** Hosts that serve a model on this machine: there is no account behind them.
 *  `URL` strips the brackets of an IPv6 literal from `hostname`, but not every
 *  engine agrees, so both spellings are listed. */
const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "0.0.0.0", "::1", "[::1]"]);

/** Suffixes that name a machine on the local network rather than on the
 *  internet: mDNS (`nas.local`) and the reserved `.localhost` TLD. */
const LOCAL_SUFFIXES = [".local", ".localhost"];

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

/**
 * A provider served from this machine or this network.
 *
 * The single answer to that question. There were two, and they disagreed:
 * one knew about `*.localhost` and not `*.local`, the other the reverse, so
 * `http://nas.local` was local enough to skip the API-key step and not local
 * enough to skip the «this will spend tokens» confirmation, and
 * `https://ollama.localhost` got both backwards. Two callers ask it for two
 * reasons — whether there is a key to ask for, and whether a request costs
 * anything — but it is one property of the address.
 *
 * Anything not provably local is treated as remote: for the key step that
 * means asking for one, for the confirmation it means showing it. Both are
 * the cheap mistake.
 */
export function isLocalBaseUrl(raw: string | null | undefined): boolean {
  const url = raw ? parse(raw) : null;
  if (!url) return false;
  const host = url.hostname.toLowerCase();
  return LOCAL_HOSTS.has(host) || LOCAL_SUFFIXES.some((suffix) => host.endsWith(suffix));
}

/** The default profile name for a hand-typed address: its host, `api.groq.com`
 *  rather than «OpenAI-compatible», which every such profile would be called. */
export function baseUrlLabel(raw: string): string {
  return parse(raw)?.host ?? "";
}
