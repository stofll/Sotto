# Overlay appearance

Open **Settings → Advanced → Overlay** to choose the recording indicator's shape, palette, size and screen position. Changes are saved automatically and apply to an open overlay. If saving fails, the controls return to the saved values; change the setting again to retry.

## Shapes and cancellation

The pill displays recording duration and audio levels. With a streaming model it expands into a card showing the live draft. After recording stops, the timer disappears and the pill shows processing progress until insertion finishes.

The bead shows audio levels and session status in a ring. Streaming recognition still works, but the live text is hidden. Errors and LLM fallback warnings expand the bead into a pill so their messages remain visible. The next recording returns to the selected shape.

The glow is a rounded composer. A colorful beam along the bottom edge rises with the voice; it is the MIT-licensed voice-glow construction, driven from Sotto's existing `audio-level` events rather than a microphone graph. Live draft text appears only for streaming models. After recording stops, status such as processing is centered in the window, with the timer along the bottom. Unlike the bead, it keeps streaming text and error messages in the same shape.

Every shape hides the cancel control until the pointer is over the overlay or the control is focused from the keyboard. After insertion, the same control dismisses the notification. Cancelling an active session uses one path for all shapes. The recording hotkey stops and processes a recording; it is not a cancellation shortcut.

The timer on the pill and the glow can be hidden from Overlay settings. The bead has no timer. Hiding it does not change the native window size; the waveform uses the space the timer occupied.

## Color and size

Palettes are picked as colors: **Overlay color** is a caption with a row of rectangular swatches under it, sitting beside the size, offset and timer controls. The name of a palette appears on hover rather than under the row. Choose copper, graphite, lagoon, violet, or a custom hue and saturation. Graphite is neutral. Success, warning and error colors retain their meaning regardless of the selected palette. The overlay keeps its dark surface in both app themes.

The overlay color is independent of the interface color. Older configurations using the retired app-accent, coal or amber palettes migrate to copper when loaded. The migration is written to disk on the next successful settings save.

**Interface color** now lives in **Settings → Advanced**. Four presets are shortcuts; the swatch with the rainbow ring opens the system color picker and any `#rrggbb` value is accepted. The companion tokens are derived from it: hover gets a lighter shade, and the text printed on the accent flips between near-black and near-white so a pale yellow and a navy both stay readable. The color is applied while the picker is being dragged and written to the configuration once the dragging settles.

The interface color is stored in the configuration and applies to both Settings and the tray popup. Accent text and keyboard focus outlines adjust to the current light or dark theme to stay readable. If saving fails, the interface returns to its saved color; select the color again to retry.

On the first successful settings load after upgrading, an older accent from browser storage is copied to configuration if no value is already saved. An existing configuration value takes precedence.

Sizes S, M and L change the native window and its contents together. Custom color sliders preview their palette in Settings; they save when the adjustment ends.

## Position

Position is chosen on a schematic 16:9 screen representing 1920 × 1080 logical pixels, rather than the current monitor. The rectangle is split into nine clickable anchor zones; a dashed outline marks the selected zone.

The overlay and its edge offset share one scale that adapts to the preview width, so changes remain proportional throughout the 0–512 range. The preview includes the native window's transparent margins; its dimensions mirror [overlay.css](../desktop/src/overlay/overlay.css) and the window sizes in [overlay_preferences.rs](../desktop/src-tauri/src/overlay_preferences.rs), and must be updated alongside them.

The edge offset runs from 0 to 512 logical pixels. The offset field sits with shape and size and has a reset button that returns it to the default 25; the button is inactive when the offset is already the default or the anchor is the center. Corner anchors apply the offset on both axes; edge anchors apply it on one. The center ignores the offset but remembers it for the next edge selection.

Offsets are measured from the overlay window to the full monitor boundary and scale with the display. Very large offsets are constrained to keep the window within the monitor. The default is 25 logical pixels. An explicitly saved offset is preserved; use the reset button to apply the default.

On Windows the next recording uses the monitor of the captured target window when available, falling back to the primary monitor. macOS currently uses the primary monitor. Changing appearance while the overlay is visible keeps it on its current monitor.
