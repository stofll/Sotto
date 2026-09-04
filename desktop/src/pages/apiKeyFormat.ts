import { t } from "../i18n";

/**
 * Проверка значения API-ключа на шаге мастера, до создания профиля.
 *
 * Единого формата у ключей нет — это стоит держать в голове, читая правила
 * ниже. `sk-` носят OpenAI и DeepSeek, Anthropic добавляет `sk-ant-`,
 * OpenRouter — `sk-or-`, Gemini выдаёт `AIza…` вообще без дефисов, Groq —
 * `gsk_…` с подчёркиванием, Cerebras — `csk-`, а Mistral и Together отдают
 * голую строку без приставки. Локальные серверы (LM Studio, Ollama, vLLM)
 * не проверяют ключ вовсе, и туда штатно пишут заглушку вроде `local`.
 *
 * Отсюда деление на два уровня. `error` — то, что неверно у любого
 * провайдера: пусто, пробел внутри, непечатаемые символы, оставленный
 * плейсхолдер из документации. Только это блокирует «Далее». Всё, что
 * зависит от провайдера — приставка и длина — возвращается как `warn`:
 * приставки меняются без предупреждения, и запрет по устаревшему списку
 * стоил бы пользователю рабочего ключа.
 *
 * `code` существует ради тестов и логов: сообщения переводятся и правятся,
 * а привязывать проверку правила к его тексту — способ получить тест,
 * который краснеет на редактуре и молчит на подмене условия.
 */
export type KeyCheckCode = "empty" | "whitespace" | "charset" | "placeholder" | "prefix" | "length";
export type KeyCheck = { level: "error" | "warn"; code: KeyCheckCode; message: string } | null;

/** Провайдеры, которые ключ не проверяют: там уместна любая строка. */
const LOCAL_PRESETS = new Set(["lmstudio", "ollama", "vllm"]);

/** Приставка, с которой ключ провайдера начинается сегодня. */
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

/** Значения из примеров в документации: скопированы, но не заменены. */
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

  // Пробел внутри — почти всегда обрезанная при копировании строка или
  // склейка двух половин; ни один провайдер пробелов в ключ не кладёт.
  // Диапазон печатаемых ASCII ниже поймал бы его и сам: правило стоит
  // раньше только ради более точного сообщения, поэтому и проверяется
  // тестом по `code`, а не по факту блокировки.
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

  // Дальше — догадки про конкретного провайдера. Локальные серверы ключ не
  // смотрят, так что и гадать не о чем.
  if (presetId && LOCAL_PRESETS.has(presetId)) return null;

  const expected = PREFIXES[presetId ?? provider];
  if (expected && !value.startsWith(expected)) {
    return { level: "warn", code: "prefix", message: t("Обычно такой ключ начинается с «{p0}». Проверьте, что он от этого провайдера.", { p0: expected }) };
  }
  if (value.length < MIN_PLAUSIBLE_LENGTH) {
    return { level: "warn", code: "length", message: t("Ключ короче, чем обычно выдают провайдеры — проверьте, что он скопирован целиком.") };
  }
  return null;
}

/** Можно ли уйти с шага ключа дальше. */
export function apiKeyBlocks(check: KeyCheck): boolean {
  return check?.level === "error";
}
