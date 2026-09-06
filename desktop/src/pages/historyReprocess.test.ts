import { describe, expect, it } from "vitest";
import { reprocessProfileOptions, reprocessPrompt } from "./HistoryPage";
import { DEFAULT_AI, PLAIN_SYSTEM_PROMPT, mergeAi, presetPrompt } from "./aiShared";

const structured = presetPrompt("structured");

const withProfiles = mergeAi(DEFAULT_AI, {
  provider: "compatible",
  model: "qwen3-27b",
  system_prompt: PLAIN_SYSTEM_PROMPT,
  profiles: [
    { id: "voice", name: "Диктовка", provider: "compatible", model: "qwen3-27b", api_key_ref: "slot-voice", prompt_preset: "plain", system_prompt: "" },
    { id: "precise", name: "Точный", provider: "anthropic", model: "claude-opus-5", api_key_ref: "slot-precise", prompt_preset: "structured", system_prompt: "" },
  ],
} as Partial<typeof DEFAULT_AI>);

describe("reprocessPrompt", () => {
  // Rust has no notion of presets: an unedited profile stores an empty
  // `system_prompt`, and sending nothing would run it on the dictation prompt.
  it("expands the chosen profile's preset", () => {
    expect(reprocessPrompt(withProfiles, "precise")).toBe(structured);
    expect(reprocessPrompt(withProfiles, "precise")).not.toBe(withProfiles.system_prompt);
  });

  it("keeps a prompt the profile edited by hand", () => {
    const edited = mergeAi(withProfiles, {
      profiles: withProfiles.profiles!.map((p) => p.id === "precise" ? { ...p, system_prompt: "перепиши аккуратно" } : p),
    });
    expect(reprocessPrompt(edited, "precise")).toBe("перепиши аккуратно");
  });

  it("has nothing to say about an id no profile answers to", () => {
    expect(reprocessPrompt(withProfiles, "deleted")).toBeUndefined();
  });
});

describe("reprocessProfileOptions", () => {
  // `CustomSelect` renders nothing when its value matches no option, so the
  // flat route — what an empty id runs on — always gets a row of its own.
  it("keeps a row for the flat route when no profile is selected", () => {
    const options = reprocessProfileOptions(withProfiles, "");
    expect(options[0].value).toBe("");
    expect(options.map((o) => o.value)).toEqual(["", "voice", "precise"]);
  });

  it("lists only the profiles when one of them is selected", () => {
    expect(reprocessProfileOptions(withProfiles, "voice").map((o) => o.value)).toEqual(["voice", "precise"]);
  });

  it("still offers a row on a config that has no profiles", () => {
    expect(reprocessProfileOptions(mergeAi(DEFAULT_AI, {}), "")).toHaveLength(1);
  });
});
