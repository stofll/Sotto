# Dictionaries

Open **Processing → Text → Dictionaries** to manage names, brands and specialist terms. Open a set by its name to inspect every term and search its contents before enabling it; its switch controls whether it participates in processing.

Several sets can be enabled together. Sotto combines their terms without repeating identical entries; disabling or deleting one set does not remove a term that remains in another enabled set.

## Built-in sets

**AI and voice** covers AI companies, assistants, model families and speech services, including Meta, Meta AI, OpenAI, Anthropic, Google DeepMind, Microsoft, NVIDIA, DeepSeek, Ollama, OpenRouter and ElevenLabs. **Apps and work** covers communication, documents, design and collaboration, including Notion, Figma, Slack, Telegram, Microsoft and Google apps. Both sets are initially disabled; enable the ones you need in the library. They do not change existing settings automatically.

**Development** also includes infrastructure, analytics, speech and testing names such as Vercel, Cloudflare, PostHog, Whisper, GigaAM, Playwright and REST Assured, plus AGENTS.md, worktree, main and short technical acronyms. Existing terms stay in this set even when they also appear in another set, so an update does not require enabling a new set to retain earlier corrections. There is no separate testing set.

Built-in catalog terms are corrected only through reviewed recognition variants, for example «опн роутер» → `OpenRouter` and «ноушен» → `Notion`; canonical spellings also serve as Whisper hints. Names that coincide with ordinary words or names are left unchanged: «мета», «клод», «мейн», «кодекс», «лама», «курсор», «обсидиан» and «дропбокс», which may refer to a UI control. `Grok` and `Groq` share the spoken variant «грок», which stays unchanged when both terms are enabled. Already-spelled English text keeps its case.

A term you add to your own set or to **My words**, including a copy of a built-in set, keeps the ordinary fuzzy matching described below in addition to its reviewed variants. Add a brand such as `Codex` to a personal set, or create a replacement rule, when you always mean it.

## Create and customize

Choose **Create** in the Dictionaries header, enter a name and optionally a description, and paste terms on separate lines or separated by commas. A term can contain spaces, such as `Claude Code`. Empty lines and exact duplicates are removed when saving. A set may contain no terms, but its name cannot be empty.

New sets and copies are saved disabled; enable them with the switch in the library when needed. Editing an existing set preserves its enabled state. Changes in the editor take effect only after a successful save; closing it with the cross or Escape asks whether to discard unsaved changes.

Built-in sets are read-only. Use **Create copy** to make an independent, initially disabled copy that you can rename and edit; future application updates do not overwrite the copy. You can also copy your own sets.

Existing personal dictionary entries appear in **My words** with their previous behavior preserved. The conversion happens when configuration is read and is persisted with the next successful settings save. Words to delete rather than spell correctly live under **Cleanup**; see [Verbal tics](#verbal-tics).

## Spelling and limits

New or edited user terms may contain at most 128 characters and 8 words. Up to 1,000 distinct user terms may be active across personal entries and enabled sets; built-in sets do not count toward this limit. A save that exceeds a limit is rejected without changing the stored dictionary. Older dictionaries already above a limit remain available, and unrelated settings can still be saved; remove or disable terms to bring them within the limit before adding more.

User terms take precedence over built-in terms when they differ only in letter case. If enabled user sets contain multiple spellings of the same term after trimming surrounding whitespace and ignoring case, choose the desired spelling before saving. Phonetically similar terms are not spelling conflicts. The choice applies while that spelling is present in an enabled user set; output capitalization also follows the existing correction rules and the source text.

The dictionary corrects similar spellings after recognition. Whisper and Qwen3 additionally receive enabled terms as recognition hints; disabling local formatting stops text correction but leaves these hints active. Qwen3 includes only the leading whole terms fitting 1,024 UTF-8 bytes to preserve space for audio in its context. Other engines do not necessarily support recognition hints, and dictionary entries do not guarantee that a term will be recognized or corrected.

Reviewed recognition variants are matched literally: `Gidhap` becomes `GitHub`, `Ridmi` becomes `README`, and `постхоg` becomes `PostHog`. These variants work only while their target term is enabled, including in a personal set or a copy, and use the selected spelling of that term. They require no cloud service or LLM.

Longer variants take priority: `Wisper Flow` becomes `Wispr Flow`, while `Wisper Turbo` becomes `Whisper Turbo`. Disabling Wispr Flow does not turn its name into Whisper Flow. Correctly spelled English words retain their original case. Ambiguous ordinary words such as «ритме» and `Maine` are left unchanged. Explicit replacement rules still take priority, and code, paths and links remain protected.

Fuzzy term matching preserves words and phrases recognised by the built-in Russian spelling dictionary. Exact phonetic matches can still restore transliterated terms; use an explicit replacement rule when you intentionally want to replace an ordinary word. Matching does not join terms across sentence punctuation.

Equally close dictionary terms leave the original text unchanged. Terms or recognised fragments of four normalized characters require an exact phonetic match, except dictionary acronyms written entirely in capitals. This prevents short words such as `REST` and «буст» from becoming `Rust`, at the cost of leaving some recognition errors uncorrected.

The editor identifies terms that are too short for the local corrector after its matching normalization. They remain stored and available to Whisper hints. Reviewed aliases are an exception: `LLМ` or «ллм» can become `LLM`, «юай» can become `UI`, and `SQEL` can become `SQL`, without enabling fuzzy matching for short terms. Use the existing text preview to check post-processing; evaluating speech recognition itself requires an audio recording.

The initial text preview, suggested replacement rules and input examples follow the interface language. Once you edit or clear the preview, your text stays unchanged when switching languages or visiting another settings page during the same app session. Changing the interface language does not translate saved rules or dictionary contents, and it does not change which verbal tics are removed: those follow their own switches and the dictation language.

The library and editor operate locally and do not require an LLM or cloud service. For optional cloud processing and its data flow, see [Privacy](privacy.md).

## Local spelling and punctuation

**Processing → Text → Cleanup → Correct spelling** checks lowercase Russian words when the dictation language is **Russian**. It uses a bundled Hunspell dictionary, works offline with every speech engine, and is enabled by default. **Auto** and other languages leave spelling unchanged: Cyrillic alone does not establish that a word is Russian. Switch it off independently or disable local formatting to keep the recognised spelling.

The corrector changes an unknown word only when it finds a single valid candidate one letter edit away and the change repairs a mechanical error: a repeated letter, transposed neighbours in words of at least seven letters, a trailing filler sound «э», or a missing hyphen in an indefinite pronoun or adverb with «кое-», «-то», «-либо» or «-нибудь». Short transpositions are left unchanged because they can produce an unrelated valid word. Repeated final letters in the short function words «так», «как», «что», «это», «тут» and «там» have a separate narrow correction rule. Arbitrary letter substitutions, compound hyphens and word splits are never invented.

It preserves recognised words, capitalised names, mixed scripts, technical tokens, and terms from enabled dictionaries. Shorter words and ambiguous candidates are left alone; agreement, missing words, and meaning are not inferred. The bundled vocabulary can miss modern slang and specialist terms.

Automatic cleanup shares protection for backtick code spans and fences, email addresses, whitespace-delimited paths, links and dotted technical names such as `Cargo.toml`. It preserves their contents, including spaces and letter case. An unfinished backtick span stays protected to the end of the input, and final punctuation is not appended after code. This is a conservative text recognizer, not a parser for every programming language or markup format.

Enabled replacement rules take priority over automatic cleanup: their actual matches are protected until the replacement step, using the same word, phrase, substring or regex matching semantics as the rule itself. Disabled, paused and invalid rules do not reserve text. Explicit rules may edit code and other protected text; automatic processing does not infer such permission. Use a replacement rule for a known term alias rather than making fuzzy matching more permissive.

Dictation and file transcription retain the original text if incidental cleanup removes everything. Silence hallucinations and explicit replacement rules that delete the whole input deliberately produce an empty result. The preview shows the cleanup result itself, including an empty result from removing only filler sounds.

Comma cleanup removes duplicate commas and a leading comma; it preserves commas before conjunctions. Removing a verbal tic also removes its following comma and leaves hyphenated words intact. Space normalization repairs missing spaces after punctuation in Cyrillic prose; a missing space after a period is restored only before a capital letter, to avoid treating lowercase domain names as sentences.

Cleanup preserves «в общем и целом», comparative «короче» inside a clause, and lexical repetitions such as «чуть чуть», «еле еле» and «едва едва». An explicit custom verbal tic still requests deletion. These safeguards do not infer sentence meaning: unpunctuated sentence boundaries and deliberate repetitions can remain ambiguous.

The spelling dictionary is prepared on a background worker at startup when needed by the saved settings, or loaded on demand after those settings change. Local formatting runs on a background worker for dictation, file transcription, and text preview. Candidate searches are bounded to words of 4–24 letters and 128 distinct unknown words per pass; words beyond that limit remain unchanged. Source revision, checksums and redistribution notices are recorded in [the bundled dictionary sources](../desktop/src-tauri/resources/spelling/SOURCES.md).

Fuzzy term matching does not rewrite a single Latin word unless it is a dictionary acronym, and does not absorb a Russian function word at either edge of a phrase. Exact phonetic matches remain available. The development set includes `Hugging Face` and `drag and drop`; an ambiguous fragment is not automatically treated as one of these terms. Russian dictionary terms ending in «й», such as a custom «оверлей», keep their case endings instead of being reset to the dictionary form. Built-in verbal-tic cleanup preserves «типа» within an unpunctuated clause, such as «поле типа string»; an explicit custom deletion still takes precedence.

## Local paragraphs

**Processing → Text → Cleanup → Split text into paragraphs** is off by default and works without an LLM. The paragraph breaks are pasted into whatever field has focus, and a single-line field or a terminal handles them badly: the field may keep only the first paragraph, and a shell may run each line. It groups existing sentences in longer text, usually around four sentences or 350 characters, and avoids leaving a very short final paragraph. It preserves existing line breaks and does not change words or invent punctuation. Short messages and text without reliable sentence endings remain together.

Paragraph detection protects code, links, common Russian and English abbreviations, initials and quoted or parenthesized text. It is a readability heuristic, not semantic topic detection. The separate **Split long sentences** option remains off by default and uses conjunction-based rules; it is not needed for paragraphs. Disabling local formatting also disables paragraph grouping.

## Verbal tics

Verbal tics are removed under **Processing → Text → Cleanup**, separately from the dictionaries: a dictionary makes a word come out spelled correctly, this list deletes the word entirely. Open **List** on the **Remove verbal tics** row to see every word the step removes and how many of them you have switched off.

The built-in words come in sets, one per language, and the set for the language you dictate in is listed first. The Russian set is on. The English set ships switched off, because its commonest fillers — `like`, `well`, `right` — are ordinary words that a whole-word match cannot tell apart from padding; switch the set on to accept that trade. Within an enabled set, click any single word to stop removing that one while the rest of the set keeps working.

Your own words are added below the sets and work in any language, whichever sets are enabled. Type a word or phrase and press Enter, or separate several with commas to add them at once; each one becomes a chip with its own remove button. Everything in this dialog is saved as you change it.

Filler sounds are a separate switch, **Remove fillers**. The Russian sounds apply to every dictation, because Cyrillic cannot match text written in another alphabet. The English sounds — `uh`, `umm`, `hmm` — apply only when the dictation language is set to English, since `er` and `um` are ordinary words in German and Dutch; a dictation language of **Auto** does not enable them.

Sound cleanup recognizes capitalized hesitations such as «А-а», «Э-э» and «Хмм», and a cut-off «э-». Punctuation right after a hesitation is removed at the start of a sentence or after another comma, so «два. А-а-а. Это» becomes «два. Это»; inside a clause the comma or sentence end is preserved because it may belong to the grammar. A drawn-out «И-и-и» becomes «И», preserving the conjunction. Single «а», «о» and «и», uppercase abbreviations, initials, protected code and explicit replacement matches are preserved; disabling **Remove fillers** also disables these corrections.
