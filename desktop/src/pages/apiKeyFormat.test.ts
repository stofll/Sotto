import { describe, expect, it } from "vitest";
import { apiKeyBlocks, checkApiKey } from "./apiKeyFormat";

const OPENAI_KEY = "sk-proj-0123456789abcdefghij";

describe("checkApiKey", () => {
  it("blocks an empty field", () => {
    expect(checkApiKey("openai", null, "")?.code).toBe("empty");
    expect(checkApiKey("openai", null, "   ")?.code).toBe("empty");
    expect(apiKeyBlocks(checkApiKey("openai", null, ""))).toBe(true);
  });

  // The printable ASCII range would catch a space on its own, so what is
  // checked is exactly what the rule exists for: its own message instead of the
  // generic one.
  it("names the space rather than lumping it in with junk characters", () => {
    const check = checkApiKey("openai", null, "sk-proj-0123 456789abcdef");
    expect(check?.code).toBe("whitespace");
    expect(apiKeyBlocks(check)).toBe(true);
  });

  it("blocks characters that do not occur in tokens", () => {
    const check = checkApiKey("openai", null, "sk-ключ-0123456789abcd");
    expect(check?.code).toBe("charset");
    expect(apiKeyBlocks(check)).toBe(true);
  });

  it("blocks a placeholder copied out of the docs", () => {
    const check = checkApiKey("openai", null, "your-api-key");
    expect(check?.code).toBe("placeholder");
    expect(apiKeyBlocks(check)).toBe(true);
  });

  it("accepts a well-formed key without comment", () => {
    expect(checkApiKey("openai", null, OPENAI_KEY)).toBeNull();
  });

  it("trims before judging", () => {
    expect(checkApiKey("openai", null, `  ${OPENAI_KEY}\n`)).toBeNull();
  });

  // There is no single format: prefixes differ, and some providers have none at
  // all. So a mismatch is a warning rather than a ban: the list of prefixes will
  // go stale sooner than the keys will.
  it("warns about a foreign prefix but lets it through", () => {
    const check = checkApiKey("anthropic", null, OPENAI_KEY);
    expect(check?.code).toBe("prefix");
    expect(check?.level).toBe("warn");
    expect(apiKeyBlocks(check)).toBe(false);
  });

  it("knows the prefixes that are not sk-", () => {
    expect(checkApiKey("gemini", null, "AIzaSy0123456789abcdefghij")).toBeNull();
    expect(checkApiKey("compatible", "groq", "gsk_0123456789abcdefghij")).toBeNull();
    expect(checkApiKey("compatible", "cerebras", "csk-0123456789abcdefghij")).toBeNull();
    expect(checkApiKey("compatible", "groq", "csk-0123456789abcdefghij")?.code).toBe("prefix");
  });

  it("says nothing about providers whose key format is not fixed", () => {
    expect(checkApiKey("compatible", "mistral", "0123456789abcdef0123456789abcdef")).toBeNull();
    expect(checkApiKey("compatible", "together", "0123456789abcdef0123456789abcdef")).toBeNull();
  });

  it("leaves local servers alone — they accept any string", () => {
    expect(checkApiKey("compatible", "ollama", "local")).toBeNull();
    expect(checkApiKey("compatible", "lmstudio", "x")).toBeNull();
  });

  it("warns about an implausibly short key elsewhere", () => {
    const check = checkApiKey("compatible", "mistral", "abc123");
    expect(check?.code).toBe("length");
    expect(apiKeyBlocks(check)).toBe(false);
  });
});
