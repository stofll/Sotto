import { describe, expect, it } from "vitest";
import { radiusAfterDrag } from "./RadiusHandle";

describe("radiusAfterDrag", () => {
  it("rounds the corners as a corner is dragged into the shell", () => {
    expect(radiusAfterDrag("sharp", 14)).toBe("soft");
    expect(radiusAfterDrag("sharp", 30)).toBe("round");
  });

  it("sharpens them as a corner is dragged out", () => {
    expect(radiusAfterDrag("round", -14)).toBe("soft");
  });

  it("stays within the three styles and ignores a small jitter", () => {
    expect(radiusAfterDrag("round", 200)).toBe("round");
    expect(radiusAfterDrag("sharp", -200)).toBe("sharp");
    expect(radiusAfterDrag("soft", 4)).toBe("soft");
  });
});
