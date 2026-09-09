# Dictionaries

Open **Processing → Text → Dictionaries** to manage names, brands and specialist terms. Open a set by its name to inspect every term and search its contents before enabling it; its switch controls whether it participates in processing.

Several sets can be enabled together. Sotto combines their terms without repeating identical entries; disabling or deleting one set does not remove a term that remains in another enabled set.

## Create and customize

Choose **Create set**, enter a name and optionally a description, and paste terms on separate lines or separated by commas. A term can contain spaces, such as `Claude Code`. Empty lines and exact duplicates are removed when saving.

New sets start disabled. Select **Enable this set after saving** if you want the terms to apply immediately after saving; otherwise you can enable the set later in the library. Changes in the editor take effect only after a successful save, and Cancel leaves the saved set unchanged.

Built-in sets are read-only. Use **Create copy** to make an independent, initially disabled copy that you can rename and edit; future application updates do not overwrite the copy. You can also copy your own sets.

Existing personal dictionary entries appear in **My words** with their previous behavior preserved. The conversion happens when configuration is read and is persisted with the next successful settings save. Filler phrases to remove are located under **Cleanup**, separate from terms to spell correctly.

## Spelling and limits

User terms take precedence over built-in terms when they differ only in letter case. If enabled user sets contain multiple spellings of the same case-insensitive term, choose the desired spelling before saving. The choice applies while that spelling is present in an enabled user set; output capitalization also follows the existing correction rules and the source text.

The dictionary corrects similar spellings after recognition. Whisper additionally receives enabled terms as recognition hints; disabling local formatting stops text correction but leaves these Whisper hints active. Other engines do not necessarily support recognition hints, and dictionary entries do not guarantee that a term will be recognized or corrected.

The editor identifies terms that are too short for the local corrector after its matching normalization. They remain stored and available to Whisper hints. Use the existing text preview to check post-processing; evaluating speech recognition itself requires an audio recording.

The initial text preview, suggested replacement rules and input examples follow the interface language. Once you edit or clear the preview, your text stays unchanged when switching languages or visiting another settings page during the same app session. Changing the interface language does not translate saved rules or dictionary contents; built-in Russian filler phrases remain Russian.

The library and editor operate locally and do not require an LLM or cloud service. For optional cloud processing and its data flow, see [Privacy](privacy.md).
