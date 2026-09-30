import { describe, expect, it } from "vitest";
import { onboardingExitTab, onboardingModels, onboardingStep } from "./modelChoices";
import type { ModelInfo } from "../bridge/types";

const models: ModelInfo[] = [
  { id: "other", label: "Other", size: "1 MB", ram: "1 MB", downloaded: false, selected: false, recommended: false },
  { id: "nemotron-streaming", label: "Nemotron", size: "651 MB", ram: "1 GB", downloaded: false, selected: false, languages: ["ru", "en"], streaming: true, engine: "sherpa-onnx" },
  { id: "gigaam-v3", label: "GigaAM", size: "214 MB", ram: "1 GB", downloaded: false, selected: false, languages: ["ru"], engine: "sherpa-onnx" },
  { id: "turbo", label: "Whisper turbo", size: "834 MB", ram: "1 GB", downloaded: false, selected: false, recommended: true, engine: "whisper.cpp" },
];

describe("onboarding model choices", () => {
  it("puts turbo first without changing catalogue flags", () => {
    const before = structuredClone(models);
    for (const os of ["windows", "macos"]) {
      expect(onboardingModels(models, "ru", os).map((model) => model.id)).toEqual(["turbo", "gigaam-v3", "nemotron-streaming"]);
    }
    expect(models).toEqual(before);
  });
  it("excludes models that cannot recognize the configured language or run on Linux", () => {
    expect(onboardingModels(models, "en", "macos").map((model) => model.id)).toEqual(["turbo", "nemotron-streaming"]);
    expect(onboardingModels(models, "de", "windows").map((model) => model.id)).toEqual(["turbo"]);
    expect(onboardingModels(models, "ru", "linux").map((model) => model.id)).toEqual(["turbo"]);
    expect(onboardingModels([], "ru", "windows")).toEqual([]);
  });
  it("keeps a working model outside the shortlist on offer", () => {
    const working = [...models.slice(0, 3), { ...models[0], id: "large-v3", label: "Whisper large-v3", downloaded: true }, models[3]];
    expect(onboardingModels(working, "ru", "windows", "large-v3").map((model) => model.id)).toEqual(["large-v3", "turbo", "gigaam-v3", "nemotron-streaming"]);
    expect(onboardingModels(working, "ru", "windows", "other").map((model) => model.id)).toEqual(["turbo", "gigaam-v3", "nemotron-streaming"]);
    expect(onboardingModels(working, "ru", "windows", "turbo").map((model) => model.id)).toEqual(["turbo", "gigaam-v3", "nemotron-streaming"]);
  });
  it("leaves a working or downloading route on Settings and sends the rest to the catalogue", () => {
    const downloaded = models.map((model) => ({ ...model, downloaded: model.id === "turbo" }));
    expect(onboardingExitTab({ model: "turbo" }, downloaded, false)).toBe("settings");
    expect(onboardingExitTab({ model: "gigaam-v3" }, downloaded, false)).toBe("models");
    expect(onboardingExitTab({ model: "gigaam-v3" }, downloaded, true)).toBe("settings");
    expect(onboardingExitTab({ model: "gigaam-v3", ai_processing: { pipeline_mode: "cloud" } }, downloaded, false)).toBe("settings");
  });
  it("resumes a valid step and recovers safely from hand-edited values", () => {
    expect(onboardingStep(2)).toBe(2);
    for (const invalid of [undefined, -1, 4, 1.5, NaN]) expect(onboardingStep(invalid)).toBe(0);
  });
});
