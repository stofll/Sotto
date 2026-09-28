import { describe, expect, it } from "vitest";
import { dependsOnNumLock, normalizeHotkeyKey } from "./hotkey";

describe("physical hotkey capture", () => {
  it.each([
    ["@", "Digit2", "2"], ["ц", "KeyW", "w"], ["W", "KeyW", "w"],
    ["+", "Equal", "equal"], ["F12", "F12", "f12"],
    ["2", "Numpad2", "numpad2"], ["ArrowDown", "Numpad2", "numpad2"], ["Enter", "NumpadEnter", "numpadenter"], ["Escape", "Escape", "escape"],
    ["ArrowLeft", "ArrowLeft", "left"], ["Unidentified", "", null],
  ])("maps %s / %s to %s", (key, code, expected) => {
    expect(normalizeHotkeyKey({ key, code })).toBe(expected);
  });

  it.each([
    ...Array.from({ length: 26 }, (_, i) => String.fromCharCode(65 + i)).map((c) => [`Key${c}`, c.toLowerCase()]),
    ...Array.from({ length: 10 }, (_, i) => [`Digit${i}`, `${i}`]),
    ...Array.from({ length: 24 }, (_, i) => [`F${i + 1}`, `f${i + 1}`]),
    ...Array.from({ length: 10 }, (_, i) => [`Numpad${i}`, `numpad${i}`]),
    ["NumpadAdd", "numpadadd"], ["NumpadSubtract", "numpadsubtract"], ["NumpadMultiply", "numpadmultiply"],
    ["NumpadDivide", "numpaddivide"], ["NumpadDecimal", "numpaddecimal"], ["NumpadEnter", "numpadenter"],
    ["Space", "space"], ["Enter", "enter"], ["Tab", "tab"], ["Backspace", "backspace"],
    ["Delete", "delete"], ["Insert", "insert"], ["ArrowRight", "right"], ["ArrowUp", "up"],
    ["Home", "home"], ["End", "end"], ["PageUp", "pageup"], ["PageDown", "pagedown"],
    ["Minus", "minus"], ["BracketLeft", "bracketleft"], ["BracketRight", "bracketright"], ["Backslash", "backslash"],
    ["Semicolon", "semicolon"], ["Quote", "quote"], ["Backquote", "backquote"], ["Comma", "comma"],
    ["Period", "period"], ["Slash", "slash"],
  ])("maps physical code %s to %s regardless of layout", (code, expected) => {
    // With NumLock off or a Russian layout, `key` differs from the key's label.
    expect(normalizeHotkeyKey({ key: "Unidentified", code })).toBe(expected);
  });

  it.each([
    ["Control", "ControlLeft", "ctrl"], ["Control", "ControlRight", "ctrl"],
    ["Alt", "AltLeft", "alt"], ["AltGraph", "AltRight", "alt"],
    ["Shift", "ShiftLeft", "shift"], ["Shift", "ShiftRight", "shift"],
    ["Meta", "MetaLeft", "cmd"], ["Meta", "MetaRight", "cmd"],
  ])("maps modifier %s / %s to %s", (key, code, expected) => {
    expect(normalizeHotkeyKey({ key, code })).toBe(expected);
  });

  it.each([
    ["CapsLock", "CapsLock"], ["NumLock", "NumLock"], ["ContextMenu", "ContextMenu"],
    ["Unidentified", "IntlBackslash"], ["Unidentified", "F25"],
  ])("ignores unsupported key %s / %s", (key, code) => {
    expect(normalizeHotkeyKey({ key, code })).toBeNull();
  });
});

describe("Num Lock dependency", () => {
  it.each([
    ["numpad0", true], ["numpad9", true], ["alt+numpad7", true], ["Ctrl + NumpadDecimal", true],
    ["numpadadd", false], ["numpaddivide", false], ["numpadenter", false],
    ["ctrl+0", false], ["f5", false], ["numpad0+ctrl", false], ["", false],
  ])("%s depends on Num Lock: %s", (hotkey, expected) => {
    expect(dependsOnNumLock(hotkey)).toBe(expected);
  });
});
