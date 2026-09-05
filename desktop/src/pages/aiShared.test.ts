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
    // The instructions are in English, the lexical samples are not: the rule
    // holds by demonstration, and on an English pair of words it would stop
    // being demonstrated for Russian dictation.
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

// The threshold exists so as "not to run the LLM on a stray sneeze", but by
// default it silently ate ordinary short dictations: the text was inserted
// unprocessed, and from outside that looks like a broken LLM rather than a
// setting doing its job.
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

describe("llmRouteBlocker", () => {
  const withKey = { openai: { available: true, label: "OpenAI", masked: "sk-…12" } };
  const hybrid = mergeAi(null, { pipeline_mode: "hybrid" });
  const cloud = mergeAi(hybrid, { pipeline_mode: "cloud", base_url: "https://example.com/v1", stt_model: "whisper-1" });

  it("local needs no provider or key", () => {
    expect(llmRouteBlocker(null, {})).toBeNull();
  });

  it("accepts working flat settings without profiles", () => {
    expect(hybrid.profiles).toEqual([]);
    expect(llmRouteBlocker(hybrid, withKey)).toBeNull();
    expect(llmRouteBlocker(cloud, withKey)).toBeNull();
  });

  it("checks the saved flat settings even if a profile disagrees", () => {
    const profile = normalizeProfile(hybrid, { id: "default", provider: "openai" });
    expect(llmRouteBlocker({ ...hybrid, profiles: [profile], api_key_ref: "missing" }, withKey)).toBe("no_key");
  });

  it("identifies missing provider, model and key", () => {
    expect(llmRouteBlocker({ ...hybrid, provider: "" }, withKey)).toBe("no_provider");
    expect(llmRouteBlocker({ ...hybrid, model: " " }, withKey)).toBe("no_model");
    expect(llmRouteBlocker(hybrid, {})).toBe("no_key");
    expect(llmRouteBlocker(cloud, {})).toBe("no_key");
  });

  it.each([undefined, "", "  ", "/v1", "example.com", "file:///tmp/model"])("rejects cloud Base URL %s", (base_url) => {
    expect(llmRouteBlocker({ ...cloud, base_url }, withKey)).toBe("invalid_base_url");
  });

  it("allows local HTTP servers and does not require a URL for hybrid", () => {
    expect(llmRouteBlocker({ ...cloud, base_url: "http://localhost:8080/v1" }, withKey)).toBeNull();
    expect(llmRouteBlocker({ ...hybrid, base_url: "" }, withKey)).toBeNull();
  });

  it("uses stt_model for cloud, including an explicitly empty value", () => {
    expect(llmRouteBlocker({ ...cloud, provider: "", model: "" }, withKey)).toBeNull();
    expect(llmRouteBlocker({ ...cloud, stt_model: "" }, withKey)).toBe("no_model");
    expect(llmRouteBlocker({ ...cloud, stt_model: undefined }, withKey)).toBeNull();
    expect(llmRouteBlocker({ ...cloud, stt_model: undefined, model: "" }, withKey)).toBe("no_model");
  });
});
