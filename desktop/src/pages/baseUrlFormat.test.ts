import { describe, expect, it } from "vitest";
import { baseUrlBlocks, baseUrlLabel, checkBaseUrl, isLocalBaseUrl, normalizeBaseUrl } from "./baseUrlFormat";

describe("checkBaseUrl", () => {
  it("blocks an empty field", () => {
    expect(checkBaseUrl("")?.code).toBe("empty");
    expect(baseUrlBlocks(checkBaseUrl("   "))).toBe(true);
  });

  it("blocks a string that is not an address", () => {
    expect(checkBaseUrl("api.example.com/v1")?.code).toBe("malformed");
    expect(baseUrlBlocks(checkBaseUrl("api.example.com/v1"))).toBe(true);
  });

  // Typing the host without a scheme parses as a URL — `localhost:` becomes the
  // protocol — so the scheme rule has to be checked, not assumed unreachable.
  it("blocks a scheme the client cannot speak", () => {
    expect(checkBaseUrl("localhost:1234/v1")?.code).toBe("scheme");
    expect(checkBaseUrl("ftp://api.example.com/v1")?.code).toBe("scheme");
  });

  it("accepts the addresses of the built-in presets", () => {
    expect(checkBaseUrl("https://api.groq.com/openai/v1")).toBeNull();
    expect(checkBaseUrl("https://api.fireworks.ai/inference/v1")).toBeNull();
    expect(checkBaseUrl("http://localhost:11434/v1")).toBeNull();
  });

  // Both remaining rules are guesses about a shape, so neither may refuse: an
  // address that works must go through even when it looks unusual.
  it("warns about the request path but lets it through", () => {
    const check = checkBaseUrl("https://api.example.com/v1/chat/completions");
    expect(check?.code).toBe("endpoint");
    expect(baseUrlBlocks(check)).toBe(false);
  });

  it("warns when the version segment is missing but lets it through", () => {
    const check = checkBaseUrl("https://api.example.com");
    expect(check?.code).toBe("suffix");
    expect(baseUrlBlocks(check)).toBe(false);
  });

  it("does not count a trailing slash as a different address", () => {
    expect(checkBaseUrl("https://api.deepseek.com/v1/")).toBeNull();
    expect(normalizeBaseUrl("  https://api.deepseek.com/v1//  ")).toBe("https://api.deepseek.com/v1");
  });
});

describe("isLocalBaseUrl", () => {
  it.each([
    "http://localhost:1234/v1",
    "http://127.0.0.1:11434/v1",
    "http://0.0.0.0:8000/v1",
    "http://[::1]:8080/v1",
    "HTTP://LocalHost:11434",
  ])("recognises %s as this machine", (url) => {
    expect(isLocalBaseUrl(url)).toBe(true);
  });

  /** The two names a machine on the local network goes by. Each of these used
   *  to be local to one of the two helpers and remote to the other, so the key
   *  step and the «this will spend tokens» confirmation disagreed about the
   *  same address. */
  it.each([
    "http://nas.local:8000/v1",
    "https://ollama.localhost/v1",
  ])("recognises %s as this network", (url) => {
    expect(isLocalBaseUrl(url)).toBe(true);
  });

  /** Anything not provably local is treated as paid and as needing a key:
   *  both are the cheap mistake. */
  it.each([
    "https://api.openai.com/v1",
    "https://localhost.example.com/v1",
    "https://notlocalhost/v1",
    "example.com",
    "",
    "   ",
    undefined,
    null,
  ])("treats %s as remote", (url) => {
    expect(isLocalBaseUrl(url)).toBe(false);
  });
});

describe("baseUrlLabel", () => {
  it("names the profile after the host, port included", () => {
    expect(baseUrlLabel("https://api.example.com/v1")).toBe("api.example.com");
    expect(baseUrlLabel("http://localhost:1234/v1")).toBe("localhost:1234");
    expect(baseUrlLabel("не адрес")).toBe("");
  });
});
