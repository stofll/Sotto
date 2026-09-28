# Overlay customization

What a person can do with the overlay constructor is described in [Overlay appearance](overlay.md). This note keeps the decisions behind it and the constraints its code has to hold across the native window and the frontend.

The direction came from a compositional recording HUD: one shell that rearranges itself around the pieces it contains. The reference clip is the Halogen case study at <https://x.com/sashabirukoff/status/2103156002220589129>. Sotto keeps its own pieces. Screenshot, restart, and delete actions from that clip are not part of dictation.

## Sizes stay named

Window size is a closed table of named layouts, each with an S, M, and L row. `window_size` in [overlay_preferences.rs](../desktop/src-tauri/src/overlay_preferences.rs) is the native source. `WINDOW_SIZE` in [overlayRecipe.ts](../desktop/src/overlay/overlayRecipe.ts) mirrors it for the scene and the constructor's stage, and the two change together.

A shell does not measure its content. A part fills a region of its shell's window, and a state that needs more room switches to a row that already exists: the bead, the stack, and the mini open into the pill row for an error, and the pill and the island open into the streaming row for a draft.

A new look is either a drawing inside an existing row or a new named layout with three rows. The stack and the mini are the layouts added for the constructor.

## What a recipe holds

One active recipe is stored as `overlay.recipe`: a shell, the element in each of its regions and how each is drawn, the style, the motion, and the processing, insertion, and cancel options. Color, size, anchor, and edge offset stay the overlay's own preferences, so a Sotto template never moves or recolors the overlay. A user template in `overlay.templates`, up to eight, brings the color and size it was saved with.

The native side reads only what decides the window: the shell, whether the pill row holds a draft, and how long the inserted note stays. The frontend normalizes everything else field by field, so an old or hand-edited recipe draws the closest thing it can. The native validator checks the shell and keeps nested or oversized values out of the configuration file.

Without a recipe, the legacy `form` draws the overlay. Saving a recipe keeps `form` in step with the closest shell, so an older Sotto still shows a similar overlay.

## What stays outside the recipe

Processing, insertion, errors, the limit countdown, and cancel remain in every recipe; the recipe chooses only how they look. The cancel button can be moved and redrawn but not removed, because it is the only way to stop a recording with the mouse. The countdown still appears without a timer part, because it is the warning before a recording stops itself.

## The constructor

The constructor is a mode of the Settings page, not a separate window. Its stage mounts the same scene as the overlay window, driven by a simulated voice, and a phase control shows each state without dictating into another application.

Every change is saved at once through one queue, so quick clicks land in the order they were made. A failed save returns the stage to the saved recipe.

## Out of scope

Free placement of pieces, a window sized from its content, user-authored markup or styles, and actions that dictation does not have. Idle UI stays absent: the overlay appears for a session and leaves when the session ends.

## Checks

Beyond the [general checks](testing.md), a recipe has to round-trip through configuration, fall back when a field is invalid, and survive a failed save. Every layout keeps the same window size at S, M, and L on the native side and in the frontend, including the pill row a compact shell opens into for an error. The limit countdown remains visible without a timer part. The constructor's stage and the overlay window must show the same scene for the same recipe and phase. Native placement on the affected operating system is still required; a unit test of the size table does not show that the window landed on the right monitor.

## References

Before the constructor was built, look-and-placement examples were collected in [Overlay appearance references](overlay-refs.md), and the options explored were listed in [Overlay ideas](overlay-ideas.md). Both record that exploration: where they speak of what exists today, they mean the three shapes that existed then. Neither is a list of features to build.
