# Numbers, dimensions and punctuation

Open **Processing → Text** and expand **Cleanup** to choose local formatting. **Russian numbers as digits** and **Format dimensions** are independent, off-by-default options; existing settings keep their previous behavior. They operate on recognized text in Rust, without an LLM or an additional network request. Dictation, file transcription and the local text preview use the same formatter.

For a first Russian measurement test, select Russian as the dictation language, enable **Cleanup**, **Russian numbers as digits** and **Format dimensions**, then choose the punctuation mode you want. Test in the text preview before using the settings in another application's input field. The number parser also accepts **Auto**; an explicit different language leaves its input unchanged.

## Russian numbers as digits

This option converts complete cardinal expressions, signed values and explicit decimal phrases. It abbreviates a recognized measurement unit following the value without changing the unit or rescaling the number.

| Recognized text | Local result before sentence styling |
| --- | --- |
| `тридцать пять миллиметров` | `35 мм` |
| `одна тысяча двести тридцать четыре` | `1234` |
| `восемь целых пять десятых миллиметра` | `8,5 мм` |
| `ноль запятая ноль пять миллиграмма` | `0,05 мг` |
| `минус ноль запятая пять` | `-0,5` |
| `8,8 мм` | `8,8 мм` |

Supported cardinal scales are thousands, millions and billions. Spoken decimal denominators cover tenths, hundredths and thousandths; an explicit `запятая` can introduce fractional digits, preserving leading zeroes. Recognized units include millimetres, centimetres, metres, micrometres, milligrams, micrograms, grams, kilograms, millilitres, litres and percentages.

This is a strict grammar, not a general language model. Unsupported case forms, ordinals, incomplete fractions, mixed or malformed numeric phrases, and expressions such as `два с половиной` stay as recognized. It does not infer a date, rewrite a telephone number, convert units or repair a value that the recognizer omitted. For example, `двадцать третьего` remains words instead of becoming an incorrect partial number.

## Dimensions

**Format dimensions** handles two or three non-negative axes separated by an explicit `на`, `х`, `x` or `×`, and writes the separator as Cyrillic `х`. Values, separators and units must be separate tokens; compact identifiers such as `8x8мм` stay unchanged. A length unit at the end, or an immediately preceding cue such as `размерами`, is required. This option parses the numbers inside a dimension even when general number conversion is off.

| Recognized text | Local result before sentence styling |
| --- | --- |
| `размерами тридцать пять на тридцать пять миллиметров` | `размерами 35 х 35 мм` |
| `восемь на восемь на три миллиметра` | `8 х 8 х 3 мм` |
| `8,8 на 3,5 мм` | `8,8 х 3,5 мм` |
| `размеры восемь на восемь` | `размеры 8 х 8` |
| `8,8 мм` | `8,8 мм` |
| `умножить восемь на восемь` with only dimensions enabled | unchanged |

Four axes, mixed units, a missing final unit after an intermediate unit, negative dimensions and unsupported numeric phrases are not combined. If general number conversion is also enabled, independently valid numerical values can still become digits while their original separators and units remain.

`8,8 мм` is a genuine decimal notation. If speech recognition produced it after the speaker said “восемь на восемь”, formatting cannot determine whether `на` was lost. Sotto therefore preserves the decimal. Inspect the raw recognition text in history when investigating this case; changing punctuation or adding a regex replacement does not recover the missing evidence.

## Punctuation policy

The **Punctuation** selector is separate from sentence capitalization. It removes punctuation from recognized text locally; it does not switch off punctuation inside the speech model.

| Mode | Behavior |
| --- | --- |
| **Automatic** | Keeps the existing cleanup, sentence style and optional sentence/paragraph splitting. |
| **No periods, ? or !** | Removes ordinary sentence periods, question marks, exclamation marks and ellipses. Keeps commas, colons and semicolons. |
| **No punctuation** | Also removes prose commas, colons, semicolons, quotation marks and detached prose dashes. |

Both removal modes preserve decimal separators, times and ratios, numeric ranges and signs, common abbreviation periods and initials, hyphens within words, apostrophes within names or contractions, and brackets that may carry mathematical grouping. Backtick code, links, email addresses, paths and dotted technical names stay protected. These protections are conservative; the setting is not a parser for every notation or programming language.

Existing line breaks stay in place. Automatic sentence splitting, automatic paragraph grouping and the final-period step do not run in a removal mode, so they cannot put sentence breaks back. The casing control becomes **Aa / aa**, affecting the beginning of the text. It does not lowercase every capital inside the recognition result: a capital after a removed pause can also be a proper name. Returning to **Automatic** restores the previously selected final-period preference.

An input made entirely of removable punctuation deliberately becomes empty, including in dictation and file transcription. Switching **Cleanup** off disables these new automatic options, while dictionary correction and explicitly enabled replacements retain their existing independent behavior.

## Replacements and optional LLM processing

An enabled replacement rule protects its actual input matches from local number and punctuation processing. Replacement output is applied after these automatic steps, so a local rule can explicitly supply punctuation. Paused, disabled or invalid rules do not protect matches. See [Dictionaries and replacements](dictionaries.md) for importing and managing those rules.

When optional LLM processing is enabled for dictation, files or history retries, accepted answers pass through the chosen punctuation-removal policy as a final step. A delayed answer uses the settings captured when its request began. This final policy can also remove punctuation that a replacement supplied earlier. The separate AI prompt-testing tools continue to show the AI's result rather than applying the local dictation policy.

The LLM fidelity check separately compares recognized numeric measurements and explicit dimension shapes. It rejects changes such as `8,8 мм → 8 х 8 мм`, a lost negative sign or a changed recognized unit, while allowing equivalent decimal separators and supported unit spellings. A rejected answer falls back to the local text and remains visible as a rejected variant in history. This is a bounded text check, not a guarantee that every numerical or semantic error can be detected.

## Testing a new configuration

1. Use the local preview with the examples above, including both `8,8 мм` and `восемь на восемь миллиметров`. Confirm the two meanings remain distinct.
2. Test each number switch separately, then together. Verify that disabling **Cleanup** restores the recognized text apart from active dictionaries and explicit replacements.
3. Try all punctuation modes with a decimal, a time, a URL, an abbreviation and backtick code. Check the final-period and casing choices after switching back to **Automatic**.
4. Dictate the same short examples on the target operating system and inspect raw, locally formatted and final text in history. Recognition accuracy, clipboard delivery and native UI behavior require this real application check.
5. If an LLM is configured, check accepted output and history retry. An answer that changes the shape of a measurement should keep the local text with a number-change fallback reason.
