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
  it("explains resource warnings and missing data without blocking selection", () => {
    expect(assessmentText().speed).toContain("неизвестна");
    expect(assessmentText(assessment).memory).toContain("Может не хватить");
    expect(assessmentText(assessment).memory).toContain("0 ГБ");
  });
  it("distinguishes reference provenance, learning and personal measurements", () => {
    const value = structuredClone(assessment);
    value.speed.source = "reference";
    value.speed.reference = "Test CPU";
    value.speed.samples = 2;
    expect(assessmentText(value).speed).toContain("Test CPU");
    expect(assessmentText(value).speed).toContain("2 из 5");
    value.speed = { ...value.speed, source: "personal", samples: 6, median_ms: 1200, audio_min: 10, audio_max: 20, unstable: true };
    expect(assessmentText(value).speed).toContain("1.2");
    expect(assessmentText(value).speed).toContain("нестабильна");
    expect(assessmentText(value).speed).not.toContain("Test CPU");
  });
  it("does not call a GPU preference confirmed acceleration", () => {
    const value = { ...assessment, compute: "gpu_unverified" as const, memory: { ...assessment.memory, status: "gpu_unknown" as const } };
    expect(assessmentText(value).compute).toContain("не подтверждено");
    expect(assessmentText(value).memory).toContain("не запрещает");
  });
  it("explains why unstable personal runs fall back to a reference", () => {
    const value = structuredClone(assessment);
    value.speed = { ...value.speed, source: "reference", reference: "Test CPU", score: 0.5, unstable: true, samples: 8 };
    expect(assessmentText(value).speed).toContain("Test CPU");
    expect(assessmentText(value).speed).toContain("Личные замеры нестабильны");
    expect(assessmentText(value).speed).not.toContain("На вашем компьютере:");
  });
});
