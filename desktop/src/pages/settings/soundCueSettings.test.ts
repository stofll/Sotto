import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn(() => Promise.resolve());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { amplitudeAt, cueSetting, positionOf, previewCue, CUE_RANGE_DB, OVERALL_RANGE_DB } = await import("./soundCueSettings");

describe("sound cue loudness scale", () => {
  it("spans the decibel range from the bottom to full scale", () => {
    expect(amplitudeAt(100, OVERALL_RANGE_DB)).toBe(1);
    expect(amplitudeAt(0, OVERALL_RANGE_DB)).toBeCloseTo(0.01);
    expect(amplitudeAt(0, CUE_RANGE_DB)).toBeCloseTo(10 ** (-1.5));
    expect(amplitudeAt(-20, OVERALL_RANGE_DB)).toBe(amplitudeAt(0, OVERALL_RANGE_DB));
  });

  it("puts saved volumes back where their slider left them", () => {
    for (const position of [0, 13, 50, 77, 100]) {
      expect(positionOf(amplitudeAt(position, OVERALL_RANGE_DB), OVERALL_RANGE_DB)).toBe(position);
    }
  });

  it("places the old volume presets inside the slider's range", () => {
    // Saved before the slider existed: Quiet, Medium, Loud.
    expect(positionOf(0.15, OVERALL_RANGE_DB)).toBe(59);
    expect(positionOf(0.35, OVERALL_RANGE_DB)).toBe(77);
    expect(positionOf(0.7, OVERALL_RANGE_DB)).toBe(92);
  });

  it("clamps what the slider cannot show", () => {
    expect(positionOf(0, OVERALL_RANGE_DB)).toBe(0);
    expect(positionOf(0.0001, OVERALL_RANGE_DB)).toBe(0);
    expect(positionOf(4, OVERALL_RANGE_DB)).toBe(100);
  });
});

describe("sound cue settings", () => {
  it("fills in the defaults for a cue that was never changed", () => {
    expect(cueSetting(null, "start")).toEqual({ source: "theme", theme: null, volume: 1, file_name: "" });
    expect(cueSetting({ sound_cues: { done: { source: "mute" } } } as never, "done"))
      .toEqual({ source: "mute", theme: null, volume: 1, file_name: "" });
  });

  it("previews the final amplitude and stays silent for a muted cue", () => {
    invoke.mockClear();
    previewCue("stop", "sonar", { source: "custom", theme: null, volume: 0.5, file_name: "chime.mp3" }, 0.4);
    expect(invoke).toHaveBeenCalledWith("preview_sound_cue", { cue: "stop", theme: "sonar", source: "custom", volume: 0.2 });
    invoke.mockClear();
    previewCue("stop", "sonar", { source: "mute", theme: null, volume: 1, file_name: "" }, 0.4);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("previews a cue in its own theme over the overall one", () => {
    invoke.mockClear();
    previewCue("error", "wood", { source: "theme", theme: "sonar", volume: 1, file_name: "" }, 0.5);
    expect(invoke).toHaveBeenCalledWith("preview_sound_cue", { cue: "error", theme: "sonar", source: "theme", volume: 0.5 });
  });
});
