import { describe, expect, it } from "vitest";
import { hasUnsavedInput } from "./ProfileWizard";

describe("hasUnsavedInput", () => {
  // The report that started this: Base URL typed, «Далее» pressed, a key
  // entered — and the cross closed the wizard without a word.
  it("guards every step past the first", () => {
    expect(hasUnsavedInput({ step: 2, isCustom: true, baseUrl: "https://api.example.com/v1" })).toBe(true);
    expect(hasUnsavedInput({ step: 3, isCustom: false, baseUrl: "" })).toBe(true);
  });

  it("guards an address typed on the first step", () => {
    expect(hasUnsavedInput({ step: 1, isCustom: true, baseUrl: "https://api.example.com/v1" })).toBe(true);
  });

  // A question on every click would be answered without reading it, and the
  // click it protects costs one click to make again.
  it("says nothing about a card that was merely picked", () => {
    expect(hasUnsavedInput({ step: 1, isCustom: false, baseUrl: "" })).toBe(false);
    // A preset fills the address in by itself — nothing of the user's is in it.
    expect(hasUnsavedInput({ step: 1, isCustom: false, baseUrl: "https://api.deepseek.com/v1" })).toBe(false);
    // The blank, picked but not filled in.
    expect(hasUnsavedInput({ step: 1, isCustom: true, baseUrl: "   " })).toBe(false);
  });
});
