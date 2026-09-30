import type { ModelInfo } from "../bridge/types";
import { supportsLanguage } from "../pages/modelCatalog";

/** A short introduction uses the live catalogue without changing its recommendation. */
export function onboardingModels(models: ModelInfo[], language = "ru", os = "") {
  return ["turbo", "gigaam-v3", "nemotron-streaming"]
    .map((id) => models.find((model) => model.id === id))
    .filter((model): model is ModelInfo => !!model
      && (os !== "linux" || model.engine !== "sherpa-onnx")
      && supportsLanguage(model, language));
}

export function onboardingStep(value?: number) {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 && value < 4 ? value : 0;
}
