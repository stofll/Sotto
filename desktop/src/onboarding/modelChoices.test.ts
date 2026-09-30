import { describe, expect, it } from "vitest";
import { onboardingModels, onboardingStep } from "./modelChoices";
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
  it("resumes a valid step and recovers safely from hand-edited values", () => {
    expect(onboardingStep(2)).toBe(2);
    for (const invalid of [undefined, -1, 4, 1.5, NaN]) expect(onboardingStep(invalid)).toBe(0);
  });
});
