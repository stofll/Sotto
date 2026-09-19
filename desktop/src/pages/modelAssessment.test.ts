import { describe, expect, it } from "vitest";
import { assessmentText, meterPercent } from "./modelAssessment";
import type { ModelAssessment } from "../bridge/modelAssessments";

const assessment: ModelAssessment = {
  id: "tiny", compute: "cpu", load_failed: false,
  memory: { status: "low", score: 0, required_bytes: 1024 ** 3, available_bytes: 0 },
  speed: { score: null, source: "unknown", samples: 0, median_ms: null, audio_min: null, audio_max: null, cold: false, unstable: false, reference: null },
};

describe("model meter presentation", () => {
  it("keeps unknown different from the slowest measured value", () => {
    expect(meterPercent(null)).toBeNull();
    expect(meterPercent(NaN)).toBeNull();
    expect(meterPercent(0)).toBe(0);
    expect(meterPercent(1.1)).toBe(100);
    expect(meterPercent(-1)).toBe(0);
  });
  it("keeps missing estimates explicit and explains low memory", () => {
    expect(assessmentText().speed).toContain("нет оценки");
    expect(assessmentText(assessment).memory).toContain("может не хватить");
    expect(assessmentText(assessment).memory).toContain("1 ГБ");
    expect(assessmentText(assessment).memory).toContain("0 ГБ");
  });
  it.each([[0.9, "Быстрая"], [0.6, "Умеренная"], [0.3, "Медленная"]])("gives a plain speed estimate for score %s", (score, label) => {
    const value = structuredClone(assessment);
    value.speed = { ...value.speed, score: Number(score), source: "reference", reference: "Test CPU", samples: 2 };
    expect(assessmentText(value).speed).toContain(label);
    expect(assessmentText(value).speed).not.toContain("Test CPU");
    expect(assessmentText(value).speed).not.toContain("из 5");
  });
  it("does not treat RAM as available GPU memory", () => {
    const value = { ...assessment, compute: "gpu_unverified" as const, memory: { ...assessment.memory, status: "gpu_unknown" as const } };
    expect(assessmentText(value).memory).toContain("памяти видеокарты");
    expect(assessmentText(value).memory).not.toContain("ГБ");
  });
  it("does not subtract an already loaded model's memory again", () => {
    const value = { ...assessment, memory: { ...assessment.memory, status: "loaded" as const } };
    expect(assessmentText(value).memory).toContain("уже загружена");
    expect(assessmentText(value).memory).not.toContain("не хватить");
  });
});
