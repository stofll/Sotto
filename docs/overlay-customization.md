# Overlay customization

Current appearance, placement, and cancellation are described in [Overlay appearance](overlay.md). This note records the direction for letting a person compose that indicator. It is not shipped behavior, and it is not a promise of a date.

The direction comes from a compositional recording HUD: one shell that rearranges itself around the pieces it contains. The reference clip is the Halogen case study at <https://x.com/sashabirukoff/status/2103156002220589129>. Sotto keeps its own pieces. Screenshot, restart, and delete actions from that clip are not part of dictation.

## Sizes stay named

Window size stays a closed table of named steps. The steps are the existing S, M, and L. Each pair of layout and size keeps the dimensions already returned by `window_size` in [overlay_preferences.rs](../desktop/src-tauri/src/overlay_preferences.rs).

Turning a piece on or off fills a reserved region of that window. It does not invent a width or height, and the shell is not measured at runtime. The timer already works this way: hiding it leaves the window as it is and gives the space to the waveform.

A new look is added by naming a layout and writing three rows, one per size. Streaming text, the bead, and the glow already have those rows. A combination that cannot fit the current layout uses an existing row, the way the bead already becomes the pill when an error must be readable.

The table is the only geometry source for the native window and for the settings preview. [overlay.css](../desktop/src/overlay/overlay.css) and the preview read those sizes rather than keeping a third copy.

## What a person composes

One active recipe is stored in configuration. It has a container, an ordered list of recording slots, a palette, a size, an anchor, and an edge offset.

The containers are the shapes that exist today: pill, bead, and glow. Palette, anchor, and edge offset stay as they are.

Recording slots are a closed set:

- Level, drawn as bars, a ring, the glow beam, or a dot matrix.
- Timer.
- Live draft, and only while the active model streams.

The order of slots inside the row can change. Their coordinates cannot. The grammar places them in the regions of the chosen size.

Anything after the recording starts is outside the recipe. Processing, insertion, errors, the limit countdown, and cancel stay mandatory. The countdown still appears when the timer slot is off, because it is the warning before a recording stops itself. Cancel still appears only on hover or keyboard focus.

The three shapes shipped today are presets of this recipe.

| Preset | Container | Recording slots |
| --- | --- | --- |
| Pill | pill | Timer and level. The live draft is added when the model streams, using the streaming row of the size table. |
| Bead | bead | Level. Text appears only for an error or an LLM fallback warning, on the pill row. |
| Glow | glow | Beam, timer, and live draft, on the glow row. |

Loading an older configuration turns `overlay.form` and `show_timer` into this recipe in memory. The migrated value is written on the next successful settings save, which is how a retired palette is already handled.

## Settings

The editor stays in Settings → Overlay. It does not open a separate window.

The stage shows the same scene the overlay window mounts, at the selected S, M, or L, on the existing 16:9 placement schematic. A phase control switches that scene among recording, streaming, processing, and error, so the recipe can be checked without dictating into another application.

Under the stage sit the three presets, then toggles for the slots the current container can hold, then container, size, palette, anchor, and offset. Choosing a preset replaces the recipe. Editing a preset stores the result as the person's recipe.

Saving stays immediate and updates an overlay that is already open. If the save fails, the controls return to the stored recipe.

Several named recipes can wait. The first version stores one active recipe plus the built-in presets.

## Preparation before the editor

1. Extract the overlay scene from [OverlayApp.tsx](../desktop/src/overlay/OverlayApp.tsx) so the live window and the settings stage mount one component. The miniature drawn beside the controls today is replaced by that scene.
2. Leave `window_size(layout, size)` as the geometry source. Extend it only when a named layout is added. Point the settings stage at the same numbers.
3. Keep the decision of what a phase shows in [overlayDetail.ts](../desktop/src/overlay/overlayDetail.ts). The recipe chooses recording slots. It does not replace that decision.
4. Read the old form and timer flag as a recipe, and cover the three presets with the tests that already lock pill, bead, and glow sizes and phase changes.

The editor ships after those four steps. Each step should leave the three current looks pixel-matched to today's windows.

## Out of scope

Free placement of pieces, a window sized from its content, user-authored markup or styles, and actions that dictation does not have. The dot matrix is a way to draw the level slot. Idle UI stays absent: the overlay appears for a session and leaves when the session ends.

## Checks

Beyond the [general checks](testing.md), a recipe has to round-trip through configuration, fall back when a field is invalid, and survive a failed save. Pill, bead, and glow presets must keep their current window sizes at S, M, and L, including the streaming row and the bead's expansion for an error. The limit countdown remains visible when the timer slot is off. The settings stage and the overlay window must show the same scene for the same recipe and phase. Native placement on the affected operating system is still required; a unit test of the size table does not show that the window landed on the right monitor.

## References

Look-and-placement examples collected for this work live in [Overlay appearance references](overlay-refs.md). They are not a list of features to copy.
