# Overlay appearance

Open **Settings → Overlay** to choose the recording indicator's shape, palette, size and screen position. Changes are saved automatically and apply to an open overlay. If saving fails, the controls return to the saved values; change the setting again to retry.

## Shapes and cancellation

The pill displays recording duration and audio levels. With a streaming model it expands into a card showing the live draft. After recording stops, the timer disappears and the pill shows processing progress until insertion finishes.

The bead shows audio levels and session status in a ring. Hover over it to reveal the cancel button; after insertion, that button dismisses the notification. Cancelling an active session uses the same cancellation path as the pill. The recording hotkey stops and processes a recording; it is not a cancellation shortcut.

Streaming recognition still works with the bead, but the live text is hidden. Errors and LLM fallback warnings expand the bead into a pill so their messages remain visible. The next recording returns to the selected shape.

## Color and size

Choose the app accent, one of five named palettes, or a custom hue and saturation. Graphite is neutral. Success, warning and error colors retain their meaning regardless of the selected palette. The overlay keeps its dark surface in both app themes.

The app accent is stored in the configuration. On the first successful settings load after upgrading, an older accent from browser storage is copied to configuration if no accent is already saved. An existing configuration value takes precedence.

Sizes S, M and L change the native window and its contents together. Custom color sliders preview their palette in Settings; they save when the adjustment ends.

## Position

Choose one of nine anchors and an edge offset from 0 to 512 logical pixels. Corner anchors apply the offset on both axes; edge anchors apply it on one. The center ignores the offset but remembers it for the next edge selection.

Offsets are measured from the overlay window to the full monitor boundary and scale with the display. Very large offsets are constrained to keep the window within the monitor. On HiDPI displays the default 96 logical pixels is a larger physical distance than the older fixed 96-pixel offset.

On Windows the next recording uses the monitor of the captured target window when available, falling back to the primary monitor. macOS currently uses the primary monitor. Changing appearance while the overlay is visible keeps it on its current monitor.
