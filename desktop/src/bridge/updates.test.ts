import { beforeEach, describe, expect, it, vi } from "vitest";
import { checkUpdate } from "./updates";
import { invoke } from "./invoke";

vi.mock("./invoke", () => ({ invoke: vi.fn() }));
const current = { available: false, current_version: "0.3.0" };

beforeEach(() => { vi.mocked(invoke).mockReset(); });

describe("shared update checks", () => {
  it("shares an overlapping check without caching later manual retries", async () => {
    vi.mocked(invoke).mockResolvedValue(current);
    const first = checkUpdate(false);
    expect(checkUpdate(false)).toBe(first);
    await expect(first).resolves.toEqual(current);
    await checkUpdate(false);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("does not reuse a check from a different release channel", async () => {
    vi.mocked(invoke).mockResolvedValue(current);
    const stable = checkUpdate(false);
    const beta = checkUpdate(true);
    expect(beta).not.toBe(stable);
    await Promise.all([stable, beta]);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("releases a failed check so the next request can retry", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Offline")).mockResolvedValue(current);
    await expect(checkUpdate(false)).rejects.toThrow("Offline");
    await expect(checkUpdate(false)).resolves.toEqual(current);
  });
});
