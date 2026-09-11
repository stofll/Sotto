import { describe, expect, it, vi } from "vitest";
import { getModelAssessments, resetModelAssessment } from "./modelAssessments";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("./invoke", () => ({ invoke }));

describe("model assessments bridge", () => {
  it("preserves unknown values and failure reasons", async () => {
    const values = [{ id: "custom", speed: { score: null }, memory: { status: "unknown" }, load_failed: true }];
    invoke.mockResolvedValueOnce(values);
    expect(await getModelAssessments()).toEqual(values);
    expect(invoke).toHaveBeenLastCalledWith("model_assessments");
  });
  it("resets only the requested model and reports failure", async () => {
    invoke.mockRejectedValueOnce(new Error("PERFORMANCE_BUSY"));
    await expect(resetModelAssessment("turbo")).rejects.toThrow("PERFORMANCE_BUSY");
    expect(invoke).toHaveBeenLastCalledWith("reset_model_assessment", { id: "turbo" });
  });
});
