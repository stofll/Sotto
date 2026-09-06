import { t } from "../i18n";
import { isLocalBaseUrl } from "./baseUrlFormat";
import { catalogGroup } from "./aiShared";

/**
 * Validation of an API key value at the wizard step, before a profile exists.
 *
 * There is no single key format — worth keeping in mind while reading the rules
 * below. `sk-` is worn by OpenAI and DeepSeek, Anthropic adds `sk-ant-`,
 * OpenRouter uses `sk-or-`, Gemini issues `AIza…` with no hyphens at all, Groq
 * `gsk_…` with an underscore, Cerebras `csk-`, while Mistral and Together hand
 * out a bare string with no prefix. Local servers (LM Studio, Ollama, vLLM) do
 * not check the key at all, and a placeholder like `local` is routinely put
 * there.
 *
 * Hence the split into two levels. `error` is what is wrong for any provider:
 * empty, a space inside, non-printable characters, a placeholder left over from
 * the documentation. Only that blocks "Next". Everything provider-dependent —
 * the prefix and the length — comes back as `warn`: prefixes change without
 * notice, and a ban based on a stale list would cost the user a working key.
 *
 * `code` exists for tests and logs: messages get translated and edited, and
 * tying a rule's check to its text is a way to get a test that goes red on
 * copy-editing and stays silent when the condition is swapped.
 */
export type KeyCheckCode = "empty" | "whitespace" | "charset" | "placeholder" | "prefix" | "length";
export type KeyCheck = { level: "error" | "warn"; code: KeyCheckCode; message: string } | null;

/** The prefix a provider's key starts with today. */
const PREFIXES: Record<string, string> = {
  openai: "sk-",
  anthropic: "sk-ant-",
  gemini: "AIza",
  openrouter: "sk-or-",
  deepseek: "sk-",
  cerebras: "csk-",
  groq: "gsk_",
  xai: "xai-",
};

/** Values from documentation examples: copied but never replaced. */
const PLACEHOLDERS = [
  "sk-...",
  "sk-xxx",
  "your-api-key",
  "your_api_key",
  "api-key",
  "api_key",
  "<your-api-key>",
  "paste-your-key-here",
];

const MIN_PLAUSIBLE_LENGTH = 16;

export function checkApiKey(provider: string, presetId: string | null, raw: string): KeyCheck {
  const value = raw.trim();
  if (!value) return { level: "error", code: "empty", message: t("Введите значение ключа.") };

  // A space inside is almost always a string truncated while copying or two
  // halves spliced together; no provider puts spaces in a key. The printable
  // ASCII range below would catch it on its own: this rule comes earlier only
  // for a more precise message, which is why it is tested by `code` rather than
  // by the fact of being blocked.
  if (/\s/.test(value)) {
    return { level: "error", code: "whitespace", message: t("В ключе есть пробел — похоже, он скопирован не целиком.") };
  }
  // eslint-disable-next-line no-control-regex
  if (/[^\x21-\x7e]/.test(value)) {
    return { level: "error", code: "charset", message: t("В ключе есть символы, которых в токенах не бывает — проверьте, что скопирован именно ключ.") };
  }
  if (PLACEHOLDERS.includes(value.toLowerCase())) {
    return { level: "error", code: "placeholder", message: t("Это пример из документации, а не ключ.") };
  }

  // From here on these are guesses about a specific provider. Local servers do
  // not look at the key, so there is nothing to guess about.
  if (presetId && catalogGroup(presetId) === "local") return null;

  const expected = PREFIXES[presetId ?? provider];
  if (expected && !value.startsWith(expected)) {
    return { level: "warn", code: "prefix", message: t("Обычно такой ключ начинается с «{p0}». Проверьте, что он от этого провайдера.", { p0: expected }) };
  }
  if (value.length < MIN_PLAUSIBLE_LENGTH) {
    return { level: "warn", code: "length", message: t("Ключ короче, чем обычно выдают провайдеры — проверьте, что он скопирован целиком.") };
  }
  return null;
}

/** Whether the key step can be left behind. */
export function apiKeyBlocks(check: KeyCheck): boolean {
  return check?.level === "error";
}

/**
 * The value written into the slot when the endpoint does not check the key.
 *
 * «Без ключа» cannot mean an empty slot: `ai_process_text` treats an empty key
 * as `missing_api_key` and skips the LLM before it ever reaches the provider,
 * and `llmRouteBlocker` refuses a route whose slot holds nothing. A local
 * server accepts any token, so a placeholder is the honest way to say «нечего
 * спрашивать» — and it keeps the profile a normal one, with a slot that can be
 * filled in later if the server is put behind auth.
 */
export const LOCAL_KEY_VALUE = "local";

/** Whether the key step has anything to ask for at all. */
export function keyIsOptional(presetId: string | null, baseUrl: string): boolean {
  if (presetId && catalogGroup(presetId) === "local") return true;
  return isLocalBaseUrl(baseUrl);
}
