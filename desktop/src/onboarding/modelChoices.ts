import type { ModelInfo } from "../bridge/types";
import type { TabId } from "../components/Shell";
import { supportsLanguage } from "../pages/modelCatalog";

/** A short introduction uses the live catalogue without changing its recommendation.
 *  A downloaded current model outside the shortlist stays offered, so replaying
 *  the introduction never steers a working setup towards a new download. */
export function onboardingModels(models: ModelInfo[], language = "ru", os = "", current?: string) {
  const choices = ["turbo", "gigaam-v3", "nemotron-streaming"]
    .map((id) => models.find((model) => model.id === id))
    .filter((model): model is ModelInfo => !!model
      && (os !== "linux" || model.engine !== "sherpa-onnx")
      && supportsLanguage(model, language));
  const working = models.find((model) => model.id === current && model.downloaded);
  return working && !choices.includes(working) ? [working, ...choices] : choices;
}

export function onboardingStep(value?: number) {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 && value < 4 ? value : 0;
}

/** Where a finished or skipped introduction leaves the user: a working route
 *  (or one being downloaded) needs nothing more, otherwise the catalogue. */
export function onboardingExitTab(config: { model?: string; ai_processing?: { pipeline_mode?: string } | null }, models: ModelInfo[], downloading: boolean): TabId {
  const ready = config.ai_processing?.pipeline_mode === "cloud"
    || models.some((model) => model.id === config.model && model.downloaded);
  return ready || downloading ? "settings" : "models";
}
