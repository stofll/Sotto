import { describe, expect, it } from "vitest";
import { normalizeHotkeyKey } from "./hotkey";

describe("physical hotkey capture", () => {
  it.each([
    ["@", "Digit2", "2"], ["ц", "KeyW", "w"], ["W", "KeyW", "w"],
    ["+", "Equal", "equal"], ["F12", "F12", "f12"],
    ["2", "Numpad2", "numpad2"], ["ArrowDown", "Numpad2", "numpad2"], ["Enter", "NumpadEnter", "numpadenter"], ["Escape", "Escape", "escape"],
    ["ArrowLeft", "ArrowLeft", "left"], ["Unidentified", "", null],
  ])("maps %s / %s to %s", (key, code, expected) => {
    expect(normalizeHotkeyKey({ key, code })).toBe(expected);
  });
});
