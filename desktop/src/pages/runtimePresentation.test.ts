import { describe, expect, it } from "vitest";
import { actualDeviceLabel, actualEngineLabel, actualModelLabel } from "./runtimePresentation";

describe("runtime presentation", () => {
  it("prefers the actually loaded GigaAM model over persisted turbo", () => {
    const runtime = { loaded_model: "gigaam-v3", engine: "sherpa-onnx" };
    expect(actualModelLabel(runtime, "turbo")).toBe("gigaam-v3");
    expect(actualEngineLabel(runtime)).toBe("sherpa-onnx");
  });

  it("does not invent a model while the engine is unloaded", () => {
    expect(actualModelLabel({ loaded_model: null }, "Модель не загружена")).toBe("Модель не загружена");
    expect(actualEngineLabel({ engine: null })).toBe("STT");
  });

  it("uses the effective cloud STT route instead of a stale local model", () => {
    const runtime = {
      loaded_model: "gigaam-v3",
      engine: "sherpa-onnx",
      device: "cpu",
      active_model: "whisper-1",
      active_engine: "cloud-stt",
      active_device: "cloud",
    };

    expect(actualModelLabel(runtime, "Модель не загружена")).toBe("whisper-1");
    expect(actualEngineLabel(runtime)).toBe("cloud-stt");
    expect(actualDeviceLabel(runtime)).toBe("cloud");
  });

  it("does not fall back to a stale local model when cloud STT is unconfigured", () => {
    const runtime = { loaded_model: "gigaam-v3", active_model: null, active_engine: "cloud-stt", active_device: "cloud" };
    expect(actualModelLabel(runtime, "Модель не загружена")).toBe("Модель не загружена");
  });
});
