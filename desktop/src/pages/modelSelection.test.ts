import { describe, expect, it, vi } from "vitest";
import { loadThenPersistModel } from "./modelSelection";

describe("model selection contract", () => {
  it("does not persist an unavailable model or relabel the old engine", async () => {
    const load = vi.fn<(modelId: string) => Promise<unknown>>()
      .mockRejectedValueOnce(new Error("MODEL_MISSING"));
    const persist = vi.fn<(patch: { model: string }) => Promise<unknown>>();

    await expect(loadThenPersistModel("gigaam-v3", "turbo", load, persist))
      .rejects.toThrow("MODEL_MISSING");

    expect(load).toHaveBeenCalledWith("gigaam-v3");
    expect(persist).not.toHaveBeenCalled();
    expect(load).toHaveBeenCalledTimes(1);
  });

  it("loads before persisting the new selection", async () => {
    const calls: string[] = [];
    const load = vi.fn(async (modelId: string) => { calls.push(`load:${modelId}`); });
    const persist = vi.fn(async (patch: { model: string }) => { calls.push(`save:${patch.model}`); return {}; });

    await loadThenPersistModel("gigaam-v3", "turbo", load, persist);

    expect(calls).toEqual(["load:gigaam-v3", "save:gigaam-v3"]);
  });

  it("restores the old engine if config persistence fails", async () => {
    const calls: string[] = [];
    const load = vi.fn(async (modelId: string) => { calls.push(`load:${modelId}`); });
    const persist = vi.fn(async () => { throw new Error("CONFIG_WRITE_FAILED"); });

    await expect(loadThenPersistModel("gigaam-v3", "turbo", load, persist))
      .rejects.toThrow("CONFIG_WRITE_FAILED");
    expect(calls).toEqual(["load:gigaam-v3", "load:turbo"]);
  });
});
