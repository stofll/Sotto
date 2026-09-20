# Dictionary sets: decisions and further development

What the feature can do today and how it applies is described in the [dictionary guide](dictionaries.md). What is kept here is the reasoning behind the decisions taken, and ideas for extending them; the ideas listed are not a promise of implementation or of a date.

## Boundaries and the reasoning behind the decisions

Dictionaries are a local library of terms, with no mandatory cloud or LLM. Inspecting and preparing a set is separate from enabling it, so that a user can study the contents before processing changes. An independent copy of a built-in set makes it adaptable without the risk of losing the changes when the app updates.

The choice between conflicting spellings is stored explicitly: the result must not depend on the order in which sets are enabled or opened. A case conflict and a fuzzy similarity are different problems; organising the library is no reason to make the corrector more aggressive. The rules for conflicts and priority, and the limits of correction, are given in [Spelling and limits](dictionaries.md#spelling-and-limits).

Migrating from the old personal list preserves the result of processing, including the earlier choice of the first spelling. Reading it again creates no copies; an empty old list needs no «My words» set. The migration and the membership rules are tested next to the implementation, in [dictionaries.rs](../desktop/src-tauri/src/dictionaries.rs).

Correcting text and prompting recognition have different limits. A short term must not be dropped from the library merely because the local corrector does not support it: it may still be useful to Whisper. The text preview checks the processing of a finished string, whereas recognition quality needs a recording.

Growing the library must not add file reads, network requests or noticeable latency to the dictation path. It uses the shared assembly of the effective dictionary and the existing preview; new data sources or matching algorithms need a justification of their own. An extension of the catalogue is judged by the quality of its terms and the risk of false corrections, not by the number of words.

## Possible extensions

- Importing and exporting sets, with a preview of the contents and duplicates resolved before anything is applied.
- Search across every set, showing the source and whether it is enabled.
- An explanation of corrections in the existing preview: the original fragment, the result and the set it came from. Correcting text and a prompt's effect on recognition must stay distinguishable.
- Adding a term from history to a chosen set.
- Excluding individual words from a built-in set, should practice show that copying it is not enough.
- Binding sets to applications or working profiles, once the need for switching context has been assessed.

## Checking further changes

Beyond the [general checks](testing.md), what matters for the library is that a draft survives a write failure and that a retry succeeds, the keyboard transitions between the editor and the confirmations, the state after a restart, long names and large lists. These scenarios need checking explicitly; a finished editor implementation does not by itself confirm they work on every platform.

Checks of dictation, file transcription, reprocessing history and the preview must preserve the distinction between correction and the Whisper prompt. Checking recognition needs a real recording; the frontend preview and string tests are no substitute. Native checks are run against isolated data per [development.md](development.md); platform support is defined by [platforms.md](platforms.md).

## Sources

- [DictionaryLibrary.tsx](../desktop/src/pages/DictionaryLibrary.tsx) — the library and the editor.
- [dictionaries.rs](../desktop/src-tauri/src/dictionaries.rs) — membership, conflicts, migration and their tests.
- [formatter.rs](../desktop/src-tauri/src/formatter.rs) — correction and text processing.
- [Architecture](architecture.md) — component boundaries and the processing paths.
