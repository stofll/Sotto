# Dictionary development plan

Status: the first version is implemented and has passed an independent Astra medium review. For a guide to what is available today see [dictionaries.md](dictionaries.md); this document records the agreed scope and the boundaries of further work.

## Goal

Turn dictionaries into a library of understandable, manageable term sets. A user should be able to inspect the contents before enabling a set, create a set for their own work, and adapt a built-in one.

Dictionaries remain a local feature with no mandatory cloud or LLM. The changes must not add noticeable latency between recording, recognition and paste.

## State before implementation

The «Processing → Text» page holds the shared personal dictionary, custom filler words and the toggles for built-in sets. The sources currently define one built-in set, «Development»; the interface receives its full word list but shows only the name and a count in the tooltip.

A built-in set is enabled by identifier and its contents are not copied into the personal dictionary. Processing merges personal words with the enabled sets, dropping case-insensitive duplicates; on such a match the personal spelling wins.

The local corrector fixes similar spellings, and for Whisper the dictionary also serves as a recognition prompt. Some short terms do not pass the corrector's constraints, so a term being in the dictionary does not guarantee a correction.

Starting points for the implementation: [the text processing page](../desktop/src/pages/OtherPages.tsx), [the frontend contracts](../desktop/src/bridge/types.ts), [dictionary sets and the corrector](../desktop/src-tauri/src/formatter.rs), [the commands and the Whisper prompt](../desktop/src-tauri/src/lib.rs). Re-check the current call sites and related tests before making changes.

## First version

### List and inspection

- Replace the small buttons with rows carrying a name, a term count and a toggle of their own.
- Open the contents by clicking the name, without changing the enabled state.
- Show the full term list, search within the set, a short description where one exists and an explicit «Enabled» / «Disabled» status.
- Allow any set to be inspected before it is enabled.
- Distinguish built-in sets from user ones. Built-in sets are readable and copyable; user sets are editable.
- Show the number of active unique terms without mixing it with the filler word count. Do not present that number as a guarantee that every term will be corrected.

### Creating and editing

- Add a «Create set» action: a name, an optional description and a list of terms, one per line.
- Support pasting a multi-line list, dropping blank lines and duplicates. Check compatibility with the current comma-separated input while keeping multi-word terms possible.
- Allow renaming, editing, duplicating, enabling, disabling and deleting a user's own sets.
- Provide a «Make a copy» action for a built-in set. The copy is independent of later updates to the built-in original.
- Perform creation and editing through a draft with explicit «Save» and «Cancel». A save failure must not destroy the entered data; offer a retry.
- Create the copy disabled, so that inspecting and preparing changes does not alter live processing. In the creation form, state explicitly whether the new set will be enabled after saving.
- On deletion, show the name of the set being removed and warn that its contents will be lost; disabling remains a separate action with no data loss.

### Page organisation

Move «Custom filler words» into «Cleanup», preserving their contents and existing behaviour. Leave terms, names and correct spellings in «Dictionaries».

Use the existing components, tokens, cards and i18n facilities. Choose how to open the details from the app's existing patterns and the space available; a separate new navigation section is not needed for the first version.

### Compatibility and application rules

- Convert the current personal list into an enabled set named «My words», preserving its contents and the processing result. Do not create a spare empty set for a new user.
- Preserve the selected built-in sets and the cleanup settings. Reloading or migrating again must not create duplicates.
- Allow several sets to be enabled at once; a disabled set takes part in neither correction nor the recognition prompt.
- Count identical terms once. Keep the user spelling's precedence over the built-in one on a case-insensitive match.
- When user sets carry conflicting spellings, show the options and let the user pick the effective one. Do not introduce a hidden precedence based on the order in which sets were enabled or opened.
- Before implementing, settle the boundary between an exact duplicate, a case difference and phonetically similar terms. Do not treat every similar word as a conflict, and do not change the fuzzy matching algorithm as part of organising sets.
- Explain the corrector's limits for unsupported terms in plain language. Do not drop such terms silently: the recognition prompt may have different limits.
- Do not hide the library on a load error: show the error and a retry action. Handle an empty library, an empty set and a search with no results.
- Preserve the existing dependencies on the processing settings; explain to the user explicitly when disabling processing prevents the dictionary from applying. Do not introduce a new global toggle without need.

## Implementation stages

1. Check the current contracts, configuration storage and every place the dictionary is applied: dictation, file transcription, reprocessing from history and preview. Pin down the exact rules for duplicates, conflicts and enablement on creation, without widening the scope of the first version.
2. Add storage for user sets and a safe transition from the old personal list. Assemble the effective dictionary in Rust through one shared mechanism; update the affected IPC contracts on both sides along with the bridge tests.
3. Implement the list, inspection and content search across built-in and user sets. Separate opening a set from toggling its effect.
4. Add creation, editing, copying and deletion, along with conflict handling and save errors. Move custom filler words into cleanup.
5. Verify processing compatibility and the interface, update the user-facing description of dictionaries, and report what was checked and what the limits are.

Do not rebuild the correction algorithm and do not add new data sources for the sake of the set editor. Reuse the existing processing and preview; do not add file reads or network requests to the hot dictation path.

## Verification and acceptance criteria

- A set can be opened and a term found in it before it is enabled; inspection activates nothing.
- A user can create a set by pasting a list, save it, edit it, disable it and enable it again; the data survives a restart.
- Copying a built-in set changes neither the original nor the effective dictionary until the copy is explicitly enabled.
- Old personal words and enabled built-in sets keep their behaviour after the transition; migrating again is safe.
- Duplicates, conflicts, blank lines and multi-word terms behave predictably and have regression tests. Deleting or disabling one set does not remove a term that remains in another active set.
- Load and save errors are visible, the retry works, and unsaved input is not lost.
- Both locales, the light and dark themes, keyboard navigation, focus when the editor opens and closes, long names and large lists are all checked.
- Corrections, and the absence of unwanted changes to ordinary text, are checked on synthetic examples. The existing dictionary usage paths and the Whisper prompt are checked; a text preview is not passed off as a check of audio recognition.
- The applicable checks from [testing.md](testing.md) are done, including i18n and the build budgets. Native scenarios are checked on the affected operating systems with isolated data; unchecked platforms and scenarios are listed explicitly.

Use [development.md](development.md) to prepare native checks, [architecture.md](architecture.md) for the app's boundaries, and [platforms.md](platforms.md) for claims about OS support. Automated tests must not read or migrate a live user configuration.

## After the first version

The following improvements are agreed as a direction but are not part of the first implementation:

- Importing and exporting sets, with a content preview and duplicate resolution before anything is applied.
- Search across all sets, showing the source and the enabled state.
- An explanation of corrections inside the existing preview: the original fragment, the result and the set it came from. Distinguish text correction from the prompt's effect on recognition.
- Adding a term from history into a chosen set.
- Excluding individual words from a built-in set, should practice show that copying is not enough.
- Binding sets to applications or work profiles, after assessing the real need for context switching.

Expand the catalogue of built-in sets only with checks on term quality and the risk of false corrections. A large word count is not a goal in itself.

## Clarifications from the pre-implementation review

A conflict is several different spellings sharing one key after trimming edge whitespace and lowercasing. Exact duplicates are counted once; phonetic similarity is not a conflict. The user picks the spelling before a conflicting active configuration is saved; the choice is stored explicitly and applies only while the chosen variant is present in the active user sets.

A new set and a copy are disabled by default; the state can be changed in the editor before saving. Editing an enabled set takes effect only after a successful save. An empty set is allowed, an empty name is not.

The old list is converted in memory when the configuration is read and written in the new shape on the next successful save. The term order and the previous choice of first spelling are preserved; reading again creates no copies.

Turning off local formatting still turns off text correction but not the Whisper prompt. The interface explains that difference. The first version does not change how case is preserved during correction: the chosen spelling remains an input to the existing algorithm, not a promise of a literal replacement in every context.

## Result verification

The frontend tests, the TypeScript check, i18n, the build and the budget checks for every window were run; the Rust tests, Clippy and rustfmt were run. Automated tests cover the migration, membership in the active sets, conflicts, the Whisper prompt, text correction, file integrity on a write failure and a successful retry.

In a separate native Windows build with a temporary configuration, the following were checked: migrating personal words, inspecting a disabled built-in set, saving an independent disabled copy, creating an enabled set, dropping exact duplicates on save, the explanation for short terms and picking a spelling on a conflict. The list was checked in both locales and both themes.

The independent review found focus being lost on the way to the confirmations; the fix passed a second review. The native check of the fixed keyboard transitions, of saving again after an artificial error, and of the interface state after a restart was interrupted by the user via Escape. Those UI scenarios, long names and very large lists need additional manual checking; macOS and recognition of a real recording were not checked as part of this work.
