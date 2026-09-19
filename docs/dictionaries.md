# Dictionaries

Open **Processing → Text → Dictionaries** to manage names, brands and specialist terms. Open a set by its name to inspect every term and search its contents before enabling it; its switch controls whether it participates in processing.

Several sets can be enabled together. Sotto combines their terms without repeating identical entries; disabling or deleting one set does not remove a term that remains in another enabled set.

## Create and customize

Choose **Create** in the Dictionaries header, enter a name and optionally a description, and paste terms on separate lines or separated by commas. A term can contain spaces, such as `Claude Code`. Empty lines and exact duplicates are removed when saving.

New sets and copies are saved disabled; enable them with the switch in the library when needed. Editing an existing set preserves its enabled state. Changes in the editor take effect only after a successful save; closing it with the cross or Escape asks whether to discard unsaved changes.

Built-in sets are read-only. Use **Create copy** to make an independent, initially disabled copy that you can rename and edit; future application updates do not overwrite the copy. You can also copy your own sets.

Existing personal dictionary entries appear in **My words** with their previous behavior preserved. The conversion happens when configuration is read and is persisted with the next successful settings save. Words to delete rather than spell correctly live under **Cleanup**; see [Verbal tics](#verbal-tics).

## Spelling and limits

User terms take precedence over built-in terms when they differ only in letter case. If enabled user sets contain multiple spellings of the same case-insensitive term, choose the desired spelling before saving. The choice applies while that spelling is present in an enabled user set; output capitalization also follows the existing correction rules and the source text.

The dictionary corrects similar spellings after recognition. Whisper additionally receives enabled terms as recognition hints; disabling local formatting stops text correction but leaves these Whisper hints active. Other engines do not necessarily support recognition hints, and dictionary entries do not guarantee that a term will be recognized or corrected.

The editor identifies terms that are too short for the local corrector after its matching normalization. They remain stored and available to Whisper hints. Use the existing text preview to check post-processing; evaluating speech recognition itself requires an audio recording.

The initial text preview, suggested replacement rules and input examples follow the interface language. Once you edit or clear the preview, your text stays unchanged when switching languages or visiting another settings page during the same app session. Changing the interface language does not translate saved rules or dictionary contents, and it does not change which verbal tics are removed: those follow their own switches and the dictation language.

The library and editor operate locally and do not require an LLM or cloud service. For optional cloud processing and its data flow, see [Privacy](privacy.md).

## Verbal tics

Verbal tics are removed under **Processing → Text → Cleanup**, separately from the dictionaries: a dictionary makes a word come out spelled correctly, this list deletes the word entirely. Open **List** on the **Remove verbal tics** row to see every word the step removes and how many of them you have switched off.

The built-in words come in sets, one per language, and the set for the language you dictate in is listed first. The Russian set is on. The English set ships switched off, because its commonest fillers — `like`, `well`, `right` — are ordinary words that a whole-word match cannot tell apart from padding; switch the set on to accept that trade. Within an enabled set, click any single word to stop removing that one while the rest of the set keeps working.

Your own words are added below the sets and work in any language, whichever sets are enabled. Type a word or phrase and press Enter, or separate several with commas to add them at once; each one becomes a chip with its own remove button. Everything in this dialog is saved as you change it.

Filler sounds are a separate switch, **Remove fillers**. The Russian sounds apply to every dictation, because Cyrillic cannot match text written in another alphabet. The English sounds — `uh`, `umm`, `hmm` — apply only when the dictation language is set to English, since `er` and `um` are ordinary words in German and Dutch; a dictation language of **Auto** does not enable them.
