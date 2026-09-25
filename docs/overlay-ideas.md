# Overlay ideas

Visual options for the recording indicator, gathered after [Overlay customization](overlay-customization.md). Current behavior stays in [Overlay appearance](overlay.md). References for these options are in [Overlay appearance references](overlay-refs.md).

This is a menu. Shipping the recipe does not mean building every shell below. A new shell is either a drawing inside a size row that already exists, or a new named layout with three sizes, S, M, and L.

## How a person would tune it

Settings → Overlay shows the same scene the live window mounts, at the chosen S, M, or L, on the existing placement schematic. A phase control switches that scene among recording, streaming, processing, insertion, and error.

Four choices are independent.

- Shell. Pill, bead, and glow are the shells that exist today. The names below are candidates.
- Level. Bars, ring, beam, a 5×5 dot matrix, or three segments. A shell may refuse a drawing that does not belong to it. The beam stays on glow.
- Recording slots. Timer, level, and live draft. The draft appears only while the model streams, and then uses the streaming or glow row that already exists.
- Motion. Quiet or soft. Quiet swaps the contents immediately. Soft crossfades the contents in about 160–200 ms. The system reduced-motion setting forces quiet. Durations are not sliders.

Palette, anchor, and edge offset stay as they are. Cancel, errors, the insertion line, and the limit countdown are outside the recipe. The countdown still appears when the timer slot is off.

The native window jumps to the chosen table row immediately. What animates is the contents of the window that is already the new size.

## Sizes that already exist

Logical pixels, from `window_size` in [overlay_preferences.rs](../desktop/src-tauri/src/overlay_preferences.rs).

| Layout | S | M | L |
| --- | --- | --- | --- |
| Pill | 280 × 60 | 308 × 64 | 360 × 72 |
| Bead | 64 × 64 | 72 × 72 | 80 × 80 |
| Streaming | 520 × 138 | 600 × 150 | 680 × 174 |
| Glow | 360 × 100 | 400 × 112 | 440 × 124 |

The window is already a rectangle. Pill, bead, and glow become capsules and circles in CSS. A square or rectangular look is a radius of 0 or 2 inside one of these boxes, until a row in the table below is actually added.

## Shells that exist today

**Pill.** Capsule. Timer and bars while recording. With a streaming model the window uses the streaming row and shows the draft. After recording, the timer column collapses and a status line takes the row.

**Bead.** Square window, circular ring. No timer. Streaming text stays hidden. An error or an LLM fallback warning uses the pill row so the message can be read. The next recording returns to the bead.

**Glow.** Rounded composer. A colored beam along the bottom edge rises with the voice. Timer and live draft stay in this shape, including errors. The beam is the level.

## Candidate shells

**Hairline.** Pill and streaming sizes. Near-black fill, a 1 px stroke in the palette color, no gradient rim and no inner glow. The timer is plain tabular digits, without its own capsule. The level is a 5×5 matrix. On streaming, the matrix stays in the top row and the draft sits beneath it.

**Quiet pill.** The current capsule and the current bars, with the timer slot off. The bars occupy the timer's space. The window does not shrink. This is a preset, not a fourth size.

**Card.** The streaming corner radius, 22 px outside and 18 px inside, and only when there is a draft or an error. A recording without text stays a pill. An empty tall card is not a candidate. The streaming window is already large over another application.

**Stack.** The one vertical addition. S 72 × 112, M 80 × 128, L 88 × 144. Matrix on top, timer below. A draft does not fit, so streaming and errors use the pill rows, the way the bead already does. These rows are not added until the stack is chosen.

**Island.** Not a shell. Pill at S, anchored to the top center, with a larger edge offset. The overlay still appears for a session and leaves with it.

**Plank.** Pill and streaming sizes with a 2 px radius, a 1 px palette stroke, and the current near-black fill. Timer on the left, bars on the right, cancel on hover. Streaming becomes a card with the same 2 px radius and the draft under the top row. This is the cheapest rectangular look, because the size rows already exist.

**Square.** Bead sizes, 64, 72, and 80. The drawing is a 5×5 matrix or three segments, not a ring. A timer does not fit. Errors and fallback warnings open into the plank.

**Brick.** A new row, only if the plank feels too wide. S 160 × 48, M 192 × 56, L 224 × 64. Matrix or three segments on the left, four timer digits on the right. A draft does not fit. Streaming and errors use the plank rows. These sizes stay out of the table until the brick is chosen.

Desktop blur through the shell is not a candidate. A transparent window above other applications often cannot sample what is behind it, and the glass becomes a flat gray slab.

## Level drawings

Bars, ring, and beam already exist. Two drawings can share their slot and the same `audio-level` events.

**5×5 matrix.** Dots light from the center outward with loudness. The color is the palette, flat. On a bead or a square it replaces the ring inside 64–80 px. On a pill or a plank it takes the waveform's place, about 28 × 28 inside the row.

**Three segments.** Quiet, middle, loud. A whole segment switches at once. Less motion than the bars, and easier to read at S.

Height and brightness follow the level immediately. The existing bars do not ease their height, because a reading arrives about every 33 ms and an eased bar smears. Pixel drawings quantize instead. A bar has four heights. A matrix lights rings of cells. A segment is on or off. There is no in-between opacity. Color may fade. The cell shape does not.

Digits stay the system monospaced face at an integer size. A pixel font is not a candidate for this window. The overlay has its own bundle budget.

A live draft is not redrawn as cells. On streaming, the top row may be cellular and the text under it stays ordinary type inside the rectangular frame.

## Motion

**Soft, for the rounded shells.** The window still snaps to the table row.

| Transition | What is visible | Window |
| --- | --- | --- |
| Recording appears | The shell fades in over 120 ms from a scale of 0.96, without rising from off screen | Recording row |
| Recording to processing, model load, or polishing | The timer column collapses in 200 ms, the bars crossfade to the label, the existing edge sweep runs | Same row |
| Recording to streaming | The draft fades into place. The unconfirmed tail of the phrase is dimmer | Streaming row, immediately |
| Streaming to processing | The draft fades, the processing label shows on the pill row | Pill row, immediately |
| Processing to insertion | The sweep stops, the line takes the success color, the counter enters in 200 ms | Same row |
| Processing to error | The text takes the error color. The shell does not shake | Bead and stack jump to the pill row |
| Recording limit | The digits and the dot take the warning color, even when the timer slot is off. Only the dot pulses | Same row |
| Hover | The cancel control fades in over 120 ms | Same row |
| Dismiss | The shell fades out over 100 ms | The window hides |

Soft covers appear, the timer collapse, the label swap, and dismiss. The window-size change stays instant in soft mode too. Quiet keeps the color changes for the limit, success, and error.

**Pixel and plank.** A scale of 0.96 and an animated radius break the grid. In soft mode the contents change in two or three steps, not by fading the whole shell.

| Transition | Pixel step |
| --- | --- |
| Appear | The frame draws in one frame, then the level cells light |
| Recording to processing | Cells go dark from left to right in three steps. A single blinking block sits beside the label |
| Recording to streaming | The top row stays. A rectangular draft opens under it. The window is already the streaming row |
| Processing to insertion | The block goes out. The frame takes the success color for one beat |
| Processing to error | Frame and text take the error color. Square and brick jump to the plank |
| Recording limit | The timer's dot cell swaps between the palette and the warning color twice a second |
| Dismiss | The cells go dark, then the frame |

The processing sweep that runs on the rounded shells is replaced, in the pixel drawing, by the blinking block. A traveling gradient does not sit on a cell grid.

## What these ideas leave out

Free placement, a window sized from its contents, user-authored markup, and actions that dictation does not have. An overlay that stays on screen while idle. A fourth size step. Per-user animation timings. A pixel typeface in the overlay bundle.

## Which ones to look at first

Hairline first, on the pill and streaming sizes, with the matrix and the flat stroke. Plank second, same sizes, radius 2, so a sharp rectangle and a pixel level can be compared one at a time. Stack waits until hairline has been seen on a real window. Brick waits until the plank has been seen and judged too wide.
