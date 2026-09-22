import { describe, expect, it, vi } from "vitest";
import { getModelAssessments } from "./modelAssessments";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("./invoke", () => ({ invoke }));

describe("model assessments bridge", () => {
  it("preserves unknown values and failure reasons", async () => {
    const values = [{ id: "custom", speed: { score: null }, memory: { status: "unknown" }, download: { insufficient: true, required_bytes: 100, available_bytes: 0 }, load_failed: true }];
    invoke.mockResolvedValueOnce(values);
    expect(await getModelAssessments()).toEqual(values);
    expect(invoke).toHaveBeenLastCalledWith("model_assessments");
  });
});
