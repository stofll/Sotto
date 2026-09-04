// Unit tests for the LLM-profile logic — the second blind spot behind the
// recent bugs (a phantom "OpenAI" profile appeared on a clean install and
// could not be deleted). These pin the contract that profiles are exactly
// what the user created: none on a fresh install, no synthesized default.

import { describe, it, expect } from "vitest";
import {
  DEFAULT_AI,
  profilesForAi,
  activeConfigFromProfile,
  normalizeProfile,
  profileKeyRef,
  mergeAi,
  PLAIN_SYSTEM_PROMPT,
  STRUCTURED_SYSTEM_PROMPT,
  SYSTEM_PROMPT_PRESETS,
  textProfileFor,
  llmRouteBlocker,
  type LlmProfile,
} from "./aiShared";

describe("textProfileFor", () => {
  const voice: LlmProfile = normalizeProfile(mergeAi(null, {}), { id: "voice", name: "Voice", provider: "openai", model: "gpt-4o-mini" });
  const other: LlmProfile = normalizeProfile(mergeAi(null, {}), { id: "other", name: "Other", provider: "anthropic", model: "claude-haiku-4-5" });
  const profiles = [voice, other];

  it("inherits the voice profile when no text profile is set", () => {
    expect(textProfileFor(mergeAi(null, {}), profiles, voice)).toBe(voice);
  });

  it("inherits on an old config that has no such field at all", () => {
    expect(textProfileFor(null, profiles, voice)).toBe(voice);
  });

  it("uses the chosen profile when it differs", () => {
    const ai = mergeAi(null, { text_profile_id: "other" });
    expect(textProfileFor(ai, profiles, voice).id).toBe("other");
  });

  it("falls back to voice when the chosen profile was deleted", () => {
    const ai = mergeAi(null, { text_profile_id: "deleted" });
    expect(textProfileFor(ai, profiles, voice)).toBe(voice);
  });

  it("treats an explicit pointer at the voice profile as inheritance", () => {
    const ai = mergeAi(null, { text_profile_id: "voice" });
    expect(textProfileFor(ai, profiles, voice)).toBe(voice);
  });
});

describe("profilesForAi", () => {
  it("returns [] for a null config", () => {
    expect(profilesForAi(null)).toEqual([]);
  });

  it("returns [] on a clean install (no saved profiles) — no phantom", () => {
    const clean = mergeAi(null, {}); // DEFAULT_AI, profiles: []
    expect(profilesForAi(clean)).toEqual([]);
  });

  it("returns [] when profiles is explicitly empty even if flat fields are set", () => {
    const ai = mergeAi(null, { provider: "openai", model: "gpt-4o-mini", profiles: [] });
    expect(profilesForAi(ai)).toEqual([]);
  });

  it("returns the saved profiles, normalized, when present", () => {
    const ai = mergeAi(null, {
      profiles: [{ id: "p1", provider: "anthropic", model: "claude-haiku-4-5" } as LlmProfile],
    });
    const result = profilesForAi(ai);
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe("p1");
    expect(result[0].provider).toBe("anthropic");
    expect(result[0].model).toBe("claude-haiku-4-5");
    // normalizeProfile fills in a key ref for non-default ids.
    expect(result[0].api_key_ref).toBe("key_p1");
  });
});

describe("profileKeyRef", () => {
  it("maps the default profile to its provider slot", () => {
    expect(profileKeyRef({ id: "default", provider: "openai", api_key_ref: "" })).toBe("openai");
  });

  it("maps a custom profile to key_<id> when no explicit ref", () => {
    expect(profileKeyRef({ id: "p1", provider: "openai", api_key_ref: "" })).toBe("key_p1");
  });

  it("honours an explicit api_key_ref", () => {
    expect(profileKeyRef({ id: "p1", provider: "openai", api_key_ref: "shared" })).toBe("shared");
  });
});

// The presets are the product: a rule that goes missing here shows up as the
// LLM quietly rewriting words the user actually said.
describe("system prompt presets", () => {
  const presets = [
    ["plain", PLAIN_SYSTEM_PROMPT],
    ["structured", STRUCTURED_SYSTEM_PROMPT],
  ] as const;

  it.each(presets)("%s forbids replacing words with synonyms", (_id, prompt) => {
    // The reported failure: «мало-мальский» came back as «малый».
    expect(prompt).toContain("Do NOT replace words with synonyms");
    // Инструкции переведены на английский, образцы лексики — нет: правило
    // держится показом, а на английской паре слов оно перестало бы
    // демонстрироваться для русской диктовки.
    expect(prompt).toContain("мало-мальск");
  });

  it.each(presets)("%s keeps the paragraph rules", (_id, prompt) => {
    expect(prompt).toContain("PARAGRAPHS");
    expect(prompt).toContain("{{language}}");
    expect(prompt).toContain("<dictation>");
  });

  it.each(presets)("%s demonstrates paragraphs without demonstrating rewriting", (_id, prompt) => {
    // The old example normalised «дипсик» to «DeepSeek» — the exact operation
    // the prompt bans two paragraphs earlier.
    expect(prompt).toContain("Splitting example:");
    expect(prompt).not.toContain("DeepSeek");
  });

  it("differs only in the list/format blocks", () => {
    expect(STRUCTURED_SYSTEM_PROMPT).toContain("LISTS");
    expect(PLAIN_SYSTEM_PROMPT).not.toContain("LISTS");
    expect(PLAIN_SYSTEM_PROMPT).toContain("NO *, -, #");
  });

  it("exposes both presets to the picker with distinct prompts", () => {
    const options = SYSTEM_PROMPT_PRESETS();
    expect(options.map((preset) => preset.id)).toEqual(["plain", "structured"]);
    expect(options[0].prompt).not.toBe(options[1].prompt);
  });
});

describe("activeConfigFromProfile", () => {
  it("promotes a profile's fields to the active flat config and stores the list", () => {
    const base = mergeAi(null, {});
    const profile = normalizeProfile(base, { id: "p1", provider: "gemini", model: "gemini-2.5-flash" });
    const next = activeConfigFromProfile(base, profile, [profile]);
    expect(next.active_profile_id).toBe("p1");
    expect(next.provider).toBe("gemini");
    expect(next.model).toBe("gemini-2.5-flash");
    expect(next.api_key_ref).toBe("key_p1");
    expect(next.profiles).toHaveLength(1);
  });
});

// Порог существует ради «не гонять LLM на случайный чих», но по умолчанию он
// молча съедал обычные короткие диктовки: текст вставлялся необработанным, и
// со стороны это выглядит как сломанная LLM, а не как сработавшая настройка.
describe("порог минимальной длительности", () => {
  it("по умолчанию не отсекает ничего", () => {
    expect(DEFAULT_AI.llm_min_duration_seconds).toBe(0);
  });

  it("новый профиль наследует нулевой порог, а не выдуманный", () => {
    const profile = normalizeProfile(mergeAi(null, {}), { id: "p1", provider: "openai" });
    expect(profile.llm_min_duration_seconds).toBe(0);
  });

  it("явный порог профиля доезжает до плоского поля, которое читает Rust", () => {
    const base = mergeAi(null, {});
    const profile = normalizeProfile(base, { id: "p1", provider: "openai", llm_min_duration_seconds: 30 });
    expect(activeConfigFromProfile(base, profile, [profile]).llm_min_duration_seconds).toBe(30);
  });

  it("нулевой порог профиля не подменяется значением из старого конфига", () => {
    const base = mergeAi(null, { llm_min_duration_seconds: 30 });
    const profile = normalizeProfile(base, { id: "p1", provider: "openai", llm_min_duration_seconds: 0 });
    expect(activeConfigFromProfile(base, profile, [profile]).llm_min_duration_seconds).toBe(0);
  });
});

// Гибридный режим без провайдера вёл себя как локальный: Rust проставлял
// skipped_reason, текст вставлялся необработанным, и на экране об этом не было
// ни слова. Тесты держат ворота ровно такими же, как на стороне Rust.
describe("llmRouteBlocker", () => {
  const withKey = { openai: { available: true, label: "OpenAI", masked: "sk-…12" } };

  it("режиму «только локально» провайдер не нужен", () => {
    expect(llmRouteBlocker(mergeAi(null, { pipeline_mode: "local" }), [], {})).toBeNull();
  });

  it("гибрид без единого профиля называет причину", () => {
    expect(llmRouteBlocker(mergeAi(null, { pipeline_mode: "hybrid" }), [], {})).toBe("no_profile");
  });

  it("облачный режим судится по тем же воротам", () => {
    const base = mergeAi(null, { pipeline_mode: "cloud", active_profile_id: "default" });
    const profile = normalizeProfile(base, { id: "default", provider: "openai", model: "gpt-4o-mini" });
    expect(llmRouteBlocker(base, [profile], {})).toBe("no_key");
    expect(llmRouteBlocker(base, [], {})).toBe("no_profile");
  });

  it("профиль без сохранённого ключа не считается рабочим маршрутом", () => {
    const base = mergeAi(null, { pipeline_mode: "hybrid", active_profile_id: "default" });
    const profile = normalizeProfile(base, { id: "default", provider: "openai", model: "gpt-4o-mini" });
    expect(llmRouteBlocker(base, [profile], {})).toBe("no_key");
  });

  it("профиль без модели не считается рабочим маршрутом", () => {
    const base = mergeAi(null, { pipeline_mode: "hybrid", active_profile_id: "default" });
    const profile = { ...normalizeProfile(base, { id: "default", provider: "openai" }), model: "" };
    expect(llmRouteBlocker(base, [profile], withKey)).toBe("no_model");
  });

  it("настроенный профиль с ключом претензий не вызывает", () => {
    const base = mergeAi(null, { pipeline_mode: "hybrid", active_profile_id: "default" });
    const profile = normalizeProfile(base, { id: "default", provider: "openai", model: "gpt-4o-mini" });
    expect(llmRouteBlocker(base, [profile], withKey)).toBeNull();
  });

  it("указатель на удалённый профиль читается как первый в списке — как и на странице", () => {
    const base = mergeAi(null, { pipeline_mode: "hybrid", active_profile_id: "deleted" });
    const profile = normalizeProfile(base, { id: "default", provider: "openai", model: "gpt-4o-mini" });
    expect(llmRouteBlocker(base, [profile], withKey)).toBeNull();
  });
});
