import { beforeEach, describe, expect, it, vi } from "vitest";
import { validateReplacementRules } from "./textFormatting";
import type { ReplacementRule } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("./invoke", () => ({ invoke }));
beforeEach(() => { invoke.mockReset(); });

describe("replacement validation bridge", () => {
  it("preserves source indices and Rust parser detail for the unsaved candidate", async () => {
    const rules: ReplacementRule[] = [{ id: "new", find: "[", replace: "x", enabled: false, match: "regex", case_sensitive: false, preserve_case: false, usage_count: 0 }];
    const errors = [{ index: 0, message: "regex parse error: unclosed character class" }];
    invoke.mockResolvedValue(errors);
    expect(await validateReplacementRules(rules)).toEqual(errors);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("validate_replacement_rules", { rules });
  });
  it("propagates validation failures so imports cannot treat an unavailable validator as success", async () => {
    invoke.mockRejectedValue(new Error("worker failed"));
    await expect(validateReplacementRules([])).rejects.toThrow("worker failed");
  });
});
