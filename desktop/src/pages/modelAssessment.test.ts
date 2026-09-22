import { describe, expect, it } from "vitest";
import { speedPresentation, downloadSpaceText } from "./modelAssessment";
import type { ModelAssessment } from "../bridge/modelAssessments";

const assessment: ModelAssessment = {
  id: "tiny", compute: "cpu", load_failed: false,
  memory: { status: "low", score: 0, required_bytes: 1024 ** 3, available_bytes: 0 },
  speed: { score: null, source: "unknown" },
};

describe("model speed presentation", () => {
  it.each([undefined, null, NaN, Infinity])("keeps missing metrics distinct from low speed: %s", (score) => {
    expect(speedPresentation(score).fill).toBeNull();
    expect(speedPresentation(score).label).toBe("Нет замера");
  });
  it.each([[0, 100 / 3], [0.499, 100 / 3], [0.5, 200 / 3], [0.799, 200 / 3], [0.8, 100], [1, 100]])("uses exactly three levels at score %s", (score, fill) => {
    expect(speedPresentation(score).fill).toBe(fill);
    expect(speedPresentation(score).hint).toContain("Оценка по сравнительным тестам");
  });
  it("reports disk capacity independently of RAM, rounding conservatively", () => {
    expect(downloadSpaceText(assessment)).toBe("");
    const disk = { ...assessment, download: { insufficient: true, required_bytes: 1.001 * 1024 ** 3, available_bytes: 0 } };
    expect(downloadSpaceText(disk)).toContain("1.01 ГБ");
    expect(downloadSpaceText(disk)).toContain("свободно 0 ГБ");
  });
});
