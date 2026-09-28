# Overlay appearance

Open **Settings → Overlay** to choose the recording indicator's template, palette, size, level sensitivity and screen position, or to build your own in the constructor. Overlay and Advanced are separate sections, both collapsed when Settings opens. Changes are saved automatically and apply to an open overlay. If saving fails, the controls return to the saved values; change the setting again to retry.

## Templates and the constructor

The overlay card offers four Sotto templates (pill, bead, glow and orb) and a row of your own templates. A template changes what the overlay is made of and how it moves; it never moves the overlay on screen. Sotto templates keep the current color and size, while your own templates bring the color and size they were saved with.

**Open the constructor** turns the Settings page into an editor. Pick a shell (the mini is the smallest: one short row for a level and a timer, which opens into the pill row only to show an error), then add parts from the library: the level (bars, pixels, oscilloscope, dot matrix, ring, orb and others), the timer, a recording indicator, the language and model, and the streaming draft. Every part is drawn live with a simulated voice. Click a part to add it or redraw the element already in place, or drag it onto a region of the preview; a region that cannot hold it says why. Click an element in the preview to see the drawings that fit there or remove it; dragging it out of the shell and releasing it there removes it too. Click an empty region to see the parts that fit there and are not in the shell yet; a part already in place moves by dragging. The Build tab is grouped by the phases of a dictation. A group folds to a one-line summary, the open groups are remembered, and working in a group or picking its phase under the preview opens it and shows that phase. Recording holds the parts. Processing sets a sign (dots, an arc, the dot matrix, a block cursor or none), a light running along the stroke, the word "Processing" beside the sign, and the pace of all three; the matrix brings its own pattern and density options. After insertion sets the green flash, whether the check mark is followed by the character count, and how long the note stays (1, 1.8 or 3.5 seconds; a note with an LLM warning stays at least 1.8 seconds so the warning can be read). Options a shell cannot use stay visible but dimmed, with the reason on hover. The Style tab sets corners, stroke, fill, voice glow, font, motion, color and size. The light fill draws the parts dark on a light shell tinted with the overlay color, and the hue and saturation sliders under the palette fine-tune the color: moving them turns a preset into a custom palette that starts from the preset's tone. The overlay card in Settings keeps only the palette swatches. In the constructor, the four brackets just outside the shell's corners drag it between sharp, soft and round corners. The screen position stays in the overlay card in Settings; the constructor's **On screen** view only shows it.

Every overlay has a cancel button, because it is the only way to stop a recording with the mouse: it can be moved and redrawn but not removed. The Build tab and a click on the button in the preview choose a cross, a stop square or the word "Cancel", its place (the start or end of the row, the card's footer or corner, the top or bottom of the stack) and whether it is always visible or appears on hover. In the bead, the stack and the mini the button sits over the parts, so it appears only on hover and has no word; in the pill and the island a hover-only button also floats over the end of the row instead of taking room from the level. The preview shows a hover-only button half visible.

A phase control under the preview shows recording, streaming, processing, insertion, error and the limit countdown, or runs them as a scenario. What happens after recording is not part of the recipe: the list under the preview explains it for the current shell. The dot matrix has its own speech and processing patterns and a density of 5 × 5, 7 × 7 or 9 × 9; while text is processed, the matrix replaces the usual status animation.

**Level sensitivity** sets how loud speech must be to fill the level. Choose High for a quiet microphone whose level barely moves, and Low for one whose level stays pinned at the top. It changes only the drawing, not the recording, and templates do not change it.

Save the result with **Save as template** in the constructor; the **+** tile in the overlay card opens the constructor. Up to eight templates are kept; each has a menu to apply, edit in the constructor, rename, or delete it. Editing applies the template and adds a **Save to** button that writes the changes back into it. Deleting a template does not change the overlay on screen.

A shell keeps one corner radius per size and corner style in every state, including the streaming card and the error row. The recipe is stored in the configuration as `overlay.recipe`, templates as `overlay.templates`, and the legacy `form` is kept in step with the closest shell so an older Sotto still shows a similar overlay. The stack and the mini are the shells with window sizes of their own: the stack is 72 × 112, 80 × 128 and 88 × 144, and the mini is 132 × 40, 148 × 44 and 168 × 48.

## Shapes and cancellation

This section describes the three shapes a configuration without a constructor recipe uses. The pill displays recording duration and audio levels. With a streaming model it expands into a card showing the live draft. After recording stops, the timer disappears and the pill shows processing progress until insertion finishes.

The bead shows audio levels and session status in a ring. Streaming recognition still works, but the live text is hidden. Errors and LLM fallback warnings expand the bead into a pill so their messages remain visible. The next recording returns to the selected shape.

The glow is a rounded composer. A colorful beam along the bottom edge rises with the voice; it is the MIT-licensed voice-glow construction, driven from Sotto's existing `audio-level` events rather than a microphone graph. Live draft text appears only for streaming models. After recording stops, status such as processing is centered in the window, with the timer along the bottom. Unlike the bead, it keeps streaming text and error messages in the same shape.

Every shape hides the cancel control until the pointer is over the visible overlay or the control is focused from the keyboard; the transparent part of the window around a small shell, such as the space above the captions chip, does not count. The short "inserted" note that follows has no control: it disappears by itself. Cancelling an active session uses one path for all shapes. The recording hotkey stops and processes a recording; it is not a cancellation shortcut.

Without a recipe, the timer on the pill and the glow can be hidden from Overlay settings; with one, the timer is a part placed in the constructor. The bead has no timer. Hiding it does not change the native window size; the waveform uses the space the timer occupied.

A recording stops by itself at the **Recording limit** from Advanced settings (15 minutes by default, or no limit) and is transcribed like any other, so one left running in toggle mode does not grow without bound. Five minutes before the stop, or a third of a shorter limit, the timer starts counting down in the warning color, even when it is hidden in settings. The legacy bead has no timer and shows no countdown. In a constructor recipe without a timer, the countdown temporarily replaces the parts inside the shell; the cancel button remains available.

## Color and size

Palettes are picked as colors: **Overlay color** is a caption with a row of rectangular swatches under it, sitting beside the size, offset and timer controls. The name of a palette appears on hover rather than under the row. Graphite is the default for configurations without a saved palette; choose copper, lagoon, violet, or a custom hue and saturation for another color. An explicitly saved palette remains unchanged. Success, warning and error colors retain their meaning regardless of the selected palette. The overlay keeps its dark surface in both app themes.

The overlay color is independent of the interface color. Older configurations using the retired app-accent, coal or amber palettes migrate to copper when loaded. The migration is written to disk on the next successful settings save.

**Interface color** now lives in **Settings → Advanced**. Four presets are shortcuts; the swatch with the rainbow ring opens the system color picker and any `#rrggbb` value is accepted. The companion tokens are derived from it: hover gets a lighter shade, and the text printed on the accent flips between near-black and near-white so a pale yellow and a navy both stay readable. The color is applied while the picker is being dragged and written to the configuration once the dragging settles.

The interface color is stored in the configuration and applies to Settings. Accent text and keyboard focus outlines adjust to the current light or dark theme to stay readable. If saving fails, the interface returns to its saved color; select the color again to retry.

On the first successful settings load after upgrading, an older accent from browser storage is copied to configuration if no value is already saved. An existing configuration value takes precedence.

Sizes S, M and L change the native window and its contents together. Custom color sliders preview their palette in Settings; they save when the adjustment ends.

## Position

Position is chosen on a schematic 16:9 screen representing 1920 × 1080 logical pixels, rather than the current monitor. The rectangle is split into nine clickable anchor zones, each marked by a dot in the overlay's own color; a dashed outline marks the selected zone, whose dot gives way to the preview standing there.

The overlay and its edge offset share one scale that adapts to the preview width, so changes remain proportional throughout the 0–512 range. The preview includes the native window's transparent margins; its dimensions mirror [overlay.css](../desktop/src/overlay/overlay.css) and the window sizes in [overlay_preferences.rs](../desktop/src-tauri/src/overlay_preferences.rs), and must be updated alongside them.

The edge offset runs from 0 to 512 logical pixels. The offset field sits with shape and size and has a reset button that returns it to the default 25; the button is inactive when the offset is already the default or the anchor is the center. Corner anchors apply the offset on both axes; edge anchors apply it on one. The center ignores the offset but remembers it for the next edge selection.

Offsets are measured from the overlay window to the full monitor boundary and scale with the display. Very large offsets are constrained to keep the window within the monitor. The default is 25 logical pixels. An explicitly saved offset is preserved; use the reset button to apply the default.

On Windows the next recording uses the monitor of the captured target window when available, falling back to the primary monitor. macOS currently uses the primary monitor. Changing appearance while the overlay is visible keeps it on its current monitor.

On Windows, positioning uses the monitor work area so the overlay stays clear of the taskbar. The edge offset is measured from that work area. Left-click the tray icon to open Settings; right-click to open the native menu containing only **Quit**. Start and stop dictation with the global hotkey while the target editor has focus.
