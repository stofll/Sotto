# Dictionaries

Open **Processing → Text → Dictionaries** to manage names, brands and specialist terms. Open a set by its name to inspect every term before enabling it; its switch controls whether it participates in processing.

Several sets can be enabled together. Sotto combines their terms without repeating identical entries; disabling or deleting one set does not remove a term that remains in another enabled set.

A dictionary says how a term is spelled, and Sotto decides which recognised fragments it fixes. A replacement rule under **Replacements** changes exactly the text you specify, including ordinary words, and always takes priority. Use a dictionary for names and terms that recognition distorts in unpredictable ways; use a replacement for a fixed conversion or for an ordinary word that a dictionary deliberately leaves alone.

**Word** mode can match a term with punctuation, such as `C++`, when it stands on its own. A letter, digit or underscore immediately outside the match makes it part of a larger word. With **Case** enabled, an uppercase match such as `ALPHA` produces `BETA`, `Alpha` produces `Beta`, and `alpha` uses the spelling you entered for the replacement.

In **Processing → Text → Replacements**, open the **⋯** menu and choose **Export** to select a folder and file name in the system save dialog. Sotto suggests `replacement-rules.json`. To load rules from a JSON file, choose **Import** from the same menu, then click **Save** in the Replacements section to apply them.

The set view groups its terms by how they fire: **Exact spellings** change only the listed reviewed variants, **By similar sound** terms also correct fragments that sound almost the same, and **Model hint only** terms never change the text. **Check a word** runs that set alone, enabled or not, on a spelling you type and shows the result and which route produced it. The text preview lists dictionary corrections separately from the replacement rules that fired.

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

The dictionary corrects similar spellings after recognition. Whisper and Qwen3 additionally receive enabled terms as recognition hints. Dictionaries and replacement rules do not depend on the **Cleanup** switch: with it off they still correct the text, and only the cleanup steps stop. Qwen3 includes only the leading whole terms fitting 1,024 UTF-8 bytes to preserve space for audio in its context. Other engines do not necessarily support recognition hints, and dictionary entries do not guarantee that a term will be recognized or corrected.

Reviewed recognition variants are matched literally: `Gidhap` becomes `GitHub`, `Ridmi` becomes `README`, and `постхоg` becomes `PostHog`. Enabled terms also restore «клаудкод», «клаудкода», `cloud code` and `cloudcode` to `Claude Code`, «ворк три» to `worktree`, and «чат GPT» to `ChatGPT`. An explicitly added literal term such as `Cloud Code` takes precedence over that alias; code, links and filenames remain protected.

With the corresponding personal terms enabled, «мцп» and «мсип» become `MCP`, and «харнаш» becomes `harness`; the ambiguous abbreviation «МСП» stays unchanged. Adding `Z.ai` enables the literal variant `ZAI` despite its short length. These variants work only while their target term is enabled, including in a personal set or a copy, and use the selected spelling of that term with the usual capitalization rules. They require no cloud service or LLM.

Longer variants take priority: `Wisper Flow` becomes `Wispr Flow`, while `Wisper Turbo` becomes `Whisper Turbo`. Disabling Wispr Flow does not turn its name into Whisper Flow. Correctly spelled English words retain their original case. Ambiguous ordinary words such as «ритме» and `Maine` are left unchanged. Explicit replacement rules still take priority, and code, paths and links remain protected.

Fuzzy term matching preserves words and phrases recognised by the built-in Russian spelling dictionary. Latin words and multiword Latin phrases also require an exact match or a reviewed recognition variant for non-acronym terms, so ordinary `back down` does not become `Markdown`. Exact phonetic matches can still restore transliterated terms; use an explicit replacement rule when you intentionally want to replace an ordinary word. Matching does not join terms across sentence punctuation. The development set includes `Hugging Face` and `drag and drop`; an ambiguous fragment is not automatically treated as one of these terms.

A Cyrillic term does not automatically transliterate Latin prose: adding «скролл» can correct «скрулл» but leaves English `scroll` unchanged. Russian terms ending in «й» or another consonant retain common inflections: «оверлея», «скроллы» and «скроллом» are not reduced to the dictionary form. Joined phonetic matching never absorbs a Russian function word at either edge of a phrase, even on an exact sound match: «скилл с созданием» stays unchanged when `skills` is enabled. Reviewed aliases and explicit replacement rules remain available for intentional conversions; dictionary matching does not infer missing words or repair sentence meaning.

The development set includes exact recognition variants for `issue` («issu») and `sidebar` («side bar», «сайдбар», «сайтбар»). Inflected «сайтбар…» forms become «сайдбар…» with their endings retained. These rules require the corresponding enabled term and do not alter identifiers, code, links or longer words such as `issues` and `issuance`.

The built-in `staging` and `Redis` terms use reviewed spellings («стейджинг» and «рэдис») rather than approximate matches, so unrelated recognition output such as «стоппинг» or «редит» is preserved. Personal copies retain their existing fuzzy matching policy.

Latin terms also retain a Russian ending when removing that ending would make the stem a closer dictionary match: «комиты», «пул реквестов» and «endpointми» stay as spoken rather than losing their number or case. Reviewed aliases and explicit replacements still take priority. Reviewed technical forms are an exception: with `commit` enabled, «комиты» becomes «коммиты» and common case endings are retained; with `pull request` enabled, forms such as «пул реквестов» become «пул-реквестов». Base forms still use the existing English aliases. These finite mappings do not guess arbitrary inflections, and a literal personal term or explicit replacement takes priority. Other forms keep their source spelling.

Equally close dictionary terms leave the original text unchanged. Terms or recognised fragments of four normalized characters require an exact phonetic match, except dictionary acronyms written entirely in capitals. This prevents short words such as `REST` and «буст» from becoming `Rust`, at the cost of leaving some recognition errors uncorrected.

The editor identifies terms that are too short for the local corrector after its matching normalization. They remain stored and available to Whisper hints. Reviewed aliases are an exception: `LLМ` or «ллм» can become `LLM`, «юай» can become `UI`, and `SQEL` can become `SQL`, without enabling fuzzy matching for short terms. Use the existing text preview to check post-processing; evaluating speech recognition itself requires an audio recording.

The initial text preview, suggested replacement rules and input examples follow the interface language. Once you edit or clear the preview, your text stays unchanged when switching languages or visiting another settings page during the same app session. Changing the interface language does not translate saved rules or dictionary contents, and it does not change which verbal tics are removed: those follow their own switches and the dictation language.

The library and editor operate locally and do not require an LLM or cloud service. For optional cloud processing and its data flow, see [Privacy](privacy.md).

## Local spelling and punctuation

**Processing → Text → Cleanup → Sentences → aa** lowercases an ordinary capital at the start of each new transcription. For example, «И завтра продолжим.» becomes «и завтра продолжим». Acronyms, mixed-case terms such as `GitHub`, and protected technical text keep their case. A name written with only an initial capital can be lowercased unless an enabled dictionary restores its spelling. Later sentence starts keep their case.

Both **Aa** and **aa** remove an ordinary period at the end of the local result, including a period immediately before a closing quotation mark. This runs after pause cleanup and replacement rules. These styles preserve question marks, exclamation marks, ellipses left by cleanup, and periods in recognised abbreviations and code. The rule applies while **Cleanup** is on.

**Processing → Text → Cleanup → Correct spelling** checks lowercase Russian words and words with an initial capital at the start of text, a line or a sentence when the dictation language is **Russian**. It restores the initial capital after correction. Known abbreviations and initials do not start a new sentence for this check. It uses a bundled Hunspell dictionary, works offline with every speech engine, and is enabled by default. **Auto** and other languages leave spelling unchanged: Cyrillic alone does not establish that a word is Russian. Switch it off on its own, or switch **Cleanup** off, to keep the recognised spelling.

The corrector changes an unknown word only when it finds a single valid candidate one letter edit away and the change repairs a mechanical error: a repeated letter, transposed neighbours in words of at least seven letters, a trailing filler sound «э», or a missing hyphen in an indefinite pronoun or adverb with «кое-», «-то», «-либо» or «-нибудь». Short transpositions are left unchanged because they can produce an unrelated valid word. Repeated final letters in the short function words «так», «как», «что», «это», «тут» and «там» have a separate narrow correction rule. The reviewed glued spellings «вобщем» and «какбудто» become «в общем» and «как будто», including at sentence starts. Arbitrary letter substitutions, compound hyphens and other word splits are not invented.

Reviewed word-level exceptions correct «ширене → ширине» and «атамарным → атомарным». «Жаля» becomes «жаль» only with a regret cue (for example, «мне жаля», «жаля, что» or «клёво, жаля только»), or as an isolated word. Its participial use, such as «оса защищалась, жаля противника», stays unchanged. Contextual choices are not cached between occurrences; dictionary terms and explicit replacement matches stay protected.

It preserves recognised words, mid-sentence capitals, all-uppercase words, mixed scripts, technical tokens, and terms from enabled dictionaries. Sentence-initial capitals use the same conservative candidate checks as lowercase words; an unknown name cannot always be distinguished from an ordinary typo. Shorter words and ambiguous candidates are left alone; agreement, missing words, and meaning are not inferred. The bundled vocabulary can miss modern slang and specialist terms.

Automatic cleanup shares protection for backtick code spans and fences, email addresses, whitespace-delimited paths, links and dotted technical names such as `Cargo.toml`. It preserves their contents, including spaces and letter case. An unfinished backtick span stays protected to the end of the input, and final punctuation is not appended after code. This is a conservative text recognizer, not a parser for every programming language or markup format.

Enabled replacement rules take priority over automatic cleanup: their actual matches are protected until the replacement step, using the same word, phrase, substring or regex matching semantics as the rule itself. Disabled, paused and invalid rules do not reserve text. Explicit rules may edit code and other protected text; automatic processing does not infer such permission. Use a replacement rule for a known term alias rather than making fuzzy matching more permissive.

Dictation and file transcription retain the original text if incidental cleanup removes everything. Silence hallucinations and explicit replacement rules that delete the whole input deliberately produce an empty result. The preview shows the cleanup result itself, including an empty result from removing only filler sounds.

Comma and space cleanup always run while **Cleanup** is on. Comma cleanup collapses repeated commas and removes a leading comma. After an explicit deletion, it also removes a comma left before a sentence ending or at the end of the text. Removing a verbal tic removes its following comma and preserves hyphenated words. Space normalization repairs missing spaces after punctuation in Cyrillic prose. After a period it restores a missing space only before a capital letter, to avoid treating lowercase domain names as sentences.

Sentence capitalization treats dots between digits as part of the number or address, as in `3.14` and `192.168.0.1`. When **Split long sentences** inserts a sentence boundary, it replaces a comma at the split point with a period and respects an existing sentence ending.

**Processing → Text → Cleanup → Advanced → Remove pause ellipses** optionally replaces `...` and `…` with a space inside a phrase and a period at its end. A capitalized word in the same alphabet after an ellipsis starts a new sentence, so «Я подумал... Может, позже» becomes «Я подумал. Может, позже.»; acronyms and a Latin term after Russian text stay inside the phrase. A capitalized name after a hesitation is indistinguishable from a new sentence and also receives a period. Leading ellipses disappear; adjacent punctuation and existing line breaks are preserved. A result containing only ellipses becomes empty, including in dictation and file delivery. The option is off by default because ellipses can be intentional. It interprets punctuation in text, not audio silence, and does not reconstruct missing words or change the following word's case.

Pause cleanup preserves backtick code, links, paths, email addresses, dotted filenames and numeric ranges such as `1...5`. Explicit replacement matches keep priority.

Cleanup preserves comparative «короче» inside a clause and lexical repetitions such as «чуть чуть», «еле еле» and «едва едва». Phrase-loop removal keeps a negative phrase followed by its positive restart, such as «не с первого раза, с первого раза…» or “not after the first restart, after the first restart…”. This protection recognizes a short list of Russian and English negators and one intervening modifier; sentence punctuation and paragraph breaks end that preceding scope. Complete repeated negative clauses can still be collapsed.

Built-in verbal-tic cleanup removes «типа» as padding, also mid-clause in unpunctuated dictation, but keeps it in «типа того», after «что-то» or «какой-нибудь», and in a type name such as «поле типа string» or «объект типа записи»; an explicit custom deletion still takes precedence.

Duplicate cleanup also preserves the predicate in «то есть есть ответ» and the repeated conjunction in «потому что что первый, что второй». These safeguards do not infer sentence meaning, and other deliberate repetitions can remain ambiguous.

The spelling dictionary is prepared on a background worker at startup when needed by the saved settings, or loaded on demand after those settings change. Local formatting runs on a background worker for dictation, file transcription, and text preview. Candidate searches are bounded to words of 4–24 letters and 128 distinct unknown words per pass; words beyond that limit remain unchanged. Source revision, checksums and redistribution notices are recorded in [the bundled dictionary sources](../desktop/src-tauri/resources/spelling/SOURCES.md).

## Local paragraphs

**Processing → Text → Cleanup → Split text into paragraphs** is off by default and works without an LLM. The paragraph breaks are pasted into whatever field has focus, and a single-line field or a terminal handles them badly: the field may keep only the first paragraph, and a shell may run each line. It groups existing sentences in longer text, after at least two sentences and roughly 250 characters, or four sentences and 180 characters, while leaving at least 60 characters for the final paragraph. It preserves existing line breaks and does not change words or invent punctuation. Short messages and text without reliable sentence endings remain together.

Paragraph detection protects code, links, common Russian and English abbreviations, initials and quoted or parenthesized text. It is a readability heuristic, not semantic topic detection. The separate **Split long sentences** option remains off by default and uses conjunction-based rules; it is not needed for paragraphs. Switching **Cleanup** off also disables paragraph grouping.

## Verbal tics

Verbal tics are removed under **Processing → Text → Cleanup**, separately from the dictionaries: a dictionary makes a word come out spelled correctly, this list deletes the word entirely. Open **List** on the **Remove verbal tics** row to see every word the step removes and how many of them you have switched off.

The built-in words come in sets, one per language, and the set for the language you dictate in is listed first. The Russian set is on, but excludes «как бы», «в общем», «понимаешь», «понимаете», «чё» and «че»: these words can carry modality, qualifiers or the sentence's predicate. Add them as custom verbal tics if you explicitly want them deleted. The English set ships switched off, because its commonest fillers — `like`, `well`, `right` — are ordinary words that a whole-word match cannot tell apart from padding; switch the set on to accept that trade. Within an enabled set, click any single word to stop removing that one while the rest of the set keeps working. For example, switching off «ну» keeps conversational «ну» in every position, while sound cleanup and duplicate removal continue to work.

Your own words are added below the sets and work in any language, whichever sets are enabled. Type a word or phrase and press Enter, or separate several with commas to add them at once; each one becomes a chip with its own remove button. Everything in this dialog is saved as you change it.

Filler sounds share a separate switch with repeated words, **Remove hesitations and repeats**. The Russian sounds apply to every dictation, because Cyrillic cannot match text written in another alphabet. The English sounds — `uh`, `umm`, `hmm` — apply only when the dictation language is set to English, since `er` and `um` are ordinary words in German and Dutch; a dictation language of **Auto** does not enable them.

Sound cleanup recognizes capitalized hesitations such as «А-а», «Э-э» and «Хмм», and a cut-off «э-». Punctuation right after a hesitation is removed at the start of a sentence or after another comma, so «два. А-а-а. Это» becomes «два. Это»; inside a clause the comma or sentence end is preserved because it may belong to the grammar. A drawn-out «И-и-и» becomes «И», preserving the conjunction.

Single «а», «о», «и» and «м», uppercase abbreviations, initials, protected code and explicit replacement matches are preserved. Numeric millimetres such as «35 мм» and era abbreviations such as «н. э.» remain intact through automatic cleanup, including decimals, ranges and nonbreaking spaces; explicit replacement rules can still change them. Held or repeated «ммм» and «м-м-м» count as hesitations. Disabling **Remove hesitations and repeats** also disables these sound corrections.
