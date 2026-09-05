// Keep every read-only hotkey surface aligned with the native fallback in
// `src-tauri/src/config.rs`. Persisted config still takes precedence.
export const DEFAULT_HOTKEY = "ctrl+shift+space";

export function normalizeHotkeyKey(e: Pick<KeyboardEvent, "key" | "code">): string | null {
  // Modifier keys come through with both e.key and e.code reflecting the name.
  if (e.key === "Control" || e.code === "ControlLeft" || e.code === "ControlRight") return "ctrl";
  if (e.key === "Meta" || e.code === "MetaLeft" || e.code === "MetaRight") return "cmd";
  if (e.key === "Alt" || e.code === "AltLeft" || e.code === "AltRight") return "alt";
  if (e.key === "Shift" || e.code === "ShiftLeft" || e.code === "ShiftRight") return "shift";
  const named: Record<string, string> = {
    Space: "space", Escape: "escape", Enter: "enter", Tab: "tab",
    Backspace: "backspace", Delete: "delete",
    ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down",
    Home: "home", End: "end", PageUp: "pageup", PageDown: "pagedown",
  };
  if (named[e.code]) return named[e.code];
  if (/^F\d{1,2}$/.test(e.code)) return e.code.toLowerCase();
  // Physical codes stay the same with Russian layouts and Shift pressed.
  if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3).toLowerCase();
  if (/^Digit[0-9]$/.test(e.code)) return e.code.slice(5);
  if (/^(Numpad[0-9]|NumpadAdd|NumpadSubtract|NumpadMultiply|NumpadDivide|NumpadDecimal|NumpadEnter|Minus|Equal|BracketLeft|BracketRight|Backslash|Semicolon|Quote|Backquote|Comma|Period|Slash|Insert)$/.test(e.code)) return e.code.toLowerCase();
  if (named[e.key]) return named[e.key];
  return null;
}
