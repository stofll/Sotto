# Local speech evaluation

The dataset mode of `model_benchmark` evaluates prepared audio and fixed text independently, using Sotto's local recognizers and formatter. It starts no application, opens no microphone, sends no cloud request, and does not paste or write history. Use a private results directory outside the frozen dataset; output files are created exclusively and never overwritten.

The public repository contains evaluator code, synthetic test fixtures and this guide, not personal evaluation data. Keep recordings, reference transcripts, annotation files, manifests and generated reports outside the checkout or under the Git-ignored `tools/speech-eval/` directory. Pass their paths to the runner; publishing the tooling does not require publishing the dataset. Model weights and local database copies stay outside commits too.

Prepare the frontend and verified native runtime as described in [Development](development.md). Build the CPU runner from `desktop/src-tauri` with `cargo build --release --locked --example model_benchmark`; keep its native libraries available when running the executable directly on Windows. Model preparation is separate: evaluation requires existing catalog weights and verifies their hashes without downloading them.

## Input and commands

A UTF-8 `manifest.jsonl` contains one record per line with `id`, `source`, `split` (`development` or `holdout`), `language`, `audio`, `observed` and `reference`. For audio, provide a dataset-relative `path`, `sha256` and `duration_seconds`; WAV must be PCM16 mono at 16 kHz. Text mode uses `observed.raw_text`, falling back to `reference.asr_text` for public examples. Keep group assignments and the original manifest unchanged.

The configuration file contains only formatting settings, language and replacement rules, never provider credentials. Text runs use each record's language with those fixed settings. ASR runs are independent of formatting, with explicit RU/EN and CPU defaults (up to eight threads). The default `--hints empty` preserves a vocabulary-free baseline; `--hints config` uses the application's effective dictionary for Whisper and Qwen3, including when formatting is disabled. Other engines, including Parakeet Ultra, receive no vocabulary prompt; metadata records support and the effective prompt. The Sherpa path uses the same segmentation methods and catalog fragment limits as file transcription; Whisper uses the application greedy decoding parameters. This measures prepared audio inference, not microphone capture, VAD trimming, live preview or hotkey-to-paste behavior.

Example PowerShell commands, run from the repository root after building:

```powershell
$dataset = 'D:\evaluation\dataset-v1'
$results = 'D:\evaluation\cycle-1'
$models = 'D:\evaluation\verified-models'
$runner = '.\desktop\src-tauri\target\release\examples\model_benchmark.exe'
# Create $results and place a safe formatter-config.json there first.
& $runner --eval --mode stt --dataset $dataset --split development --source google/fleurs --language ru --model gigaam-v3 --models-dir $models --config "$results\formatter-config.json" --output "$results\asr-ru.jsonl"
& $runner --eval --mode format --dataset $dataset --split development --config "$results\formatter-config.json" --output "$results\format.jsonl"
& $runner --eval --mode regressions --dataset $dataset --input desktop/src-tauri/tests/fixtures/formatter/cases.json --split development --config "$results\formatter-config.json" --output "$results\regressions.jsonl"
python scripts/speech_eval.py --dataset $dataset --results "$results\asr-ru.jsonl" --output "$results\asr-ru-report.json"
```

Omit `--language` to evaluate all supported record languages, or use `--source sotto-private-history` for private observations. `--limit` selects the first N matching records for a smoke run; its result is not the full dataset. `--input` accepts a separate JSONL manifest of prepared records, so a second text run can use a frozen ASR result as its raw input without changing the dataset. Only `--mode regressions` accepts the existing formatter fixture JSON array; it checks delivery and preview expectations independently.

For an isolated Parakeet Ultra STT probe, `--fragment-seconds N` overrides its fragment limit in the runner, using the application's existing pause-based splitting and preserving every sample. Values must be positive and no larger than the model's catalog limit (currently 120 seconds); other models and non-STT modes reject this option. Omitting it uses the catalog default. The override is recorded in run metadata and does not change application settings or model weights; compare it with a fresh default run using the same executable and reviewed inputs before drawing quality conclusions.

For a decoding experiment, `--beam-size N` selects beam search for Whisper or modified beam search for Parakeet Ultra, with a width of 1–8. Omission preserves greedy decoding; a beam width of one still selects the beam algorithm. Other models and non-STT modes reject the option before I/O. The override is recorded in metadata and reuses the normal model verification and segmentation paths. It does not enable vocabulary hints for Ultra or change application defaults. Compare the same executable, audio, references and configuration, recording both errors and latency; a wider search is not necessarily more accurate. Start with a fixed development pilot and reject a worse candidate before expanding the run.

Each JSONL run records source fingerprints, Git revision, model weight revision, full safe configuration, selection, hardware, CPU/thread settings, audio duration and per-record timing. The load line includes weight verification; the first inference is marked separately, and the first audio is repeated with the same loaded state. Subsequent records are warm. Retain the executable hash, a source snapshot or diff and the lockfile with a cycle; the source fingerprint describes files present at run time, so rebuild after edits and never substitute a stale binary.

Results are flushed after every record, followed by a completion marker. The scorer rejects incomplete or duplicate runs. After interruption, keep the checkpoint; select unfinished IDs into a new input manifest and run into a new output file with the identical binary, weights and configuration. Do not splice results from different contexts. Completed files can be scored again into new reports without repeating inference.

## Interpretation

The scorer uses publisher or reviewed ASR references only. It reports languages, sources, model runs and duration buckets separately, with absolute substitutions, deletions and insertions alongside micro WER/CER. Normalization uses Unicode NFC and case folding, maps curly apostrophes to ASCII, replaces other punctuation with spaces and collapses whitespace. It preserves `ё`, numeric tokens, negations and apostrophes; it does not equate digit and spoken number forms. Such formatting differences need separate inspection, not silent normalization.

Raw WER compares whitespace tokens against `reference.asr_text`. Raw CER compares exact characters against `reference.punctuated_text` when present, otherwise the ASR reference; it is a written-prompt proxy, not an approved spontaneous punctuation target. Normalized CER includes spaces. Reports retain original input, output, references and diffs, so casing, punctuation, numbers and terms remain inspectable. A fixed-reference text run measures lexical damage caused by formatting; it is not an ASR accuracy result or a cleanup gold score.

Reports retain removal counters and also flag changed normalized spans containing known negation markers, digits or a finite list of Russian/English cardinal number words. These `negation_changes` and `numeric_changes` include the before/after phrase, nearby context and token diff; additions and relocations can therefore trigger review even when token counts are unchanged. Spelled numbers in other languages, unlisted inflections and punctuation-only numeric signs are outside this lexical check.

Flags are review prompts, not proof of errors; explicit replacements and approved cleanup can also trigger them, and historical Sotto or LLM output is never a gold transcript. Private records without references still show before/after changes, lost Latin terms, empty output and substantial word loss. Review suspected repetition and deletions against audio, especially at fragment boundaries; read speech and short clips do not establish quality for long, spontaneous or mixed-language dictation.

Copy the annotation queue into the experiment directory. Keep three separate targets: verbatim speech with disfluencies, the same words with punctuation, and a faithful cleaned result. Mark a record reviewed only after audio verification, with reviewer, method and uncertainty notes. Exclude unresolved speech from exact scoring; keep proposed STT/LLM texts labelled as proposals.

To score reviewed private speech without editing the frozen manifest, pass `--annotations reviewed.jsonl` to the Python scorer. Each reviewed row needs `id`, `audio_sha256`, `reviewer`, `method` and the approved target text. Use `status=asr_reviewed`, `punctuation_reviewed` or `clean_reviewed` for one target, or `status=reviewed` with an explicit `reviewed_targets` list for several. Pending/proposed rows are ignored; audio hashes and provenance are checked before references are overlaid in memory. An ASR review alone does not approve punctuation or cleaned text, and a small reviewed subset must be reported with its own reference denominator.

Select hypotheses and counterexamples on development, add synthetic regression cases, then freeze the chosen code/configuration before opening holdout. Evaluate holdout once; reject a candidate that regresses rather than adjusting it against the same holdout. Record rejected experiments and native checks that were not performed. Keep personal audio, texts and reports out of Git, preserve source attribution and assess each dataset's own terms independently of Sotto's MIT license.

## Quality and progress

Evaluate shared text processing on fixed outputs from several recognizers, including Parakeet Ultra (`parakeet-ultra`), GigaAM Multilingual (`gigaam-multilingual`) and Whisper Turbo (`turbo`). For each model, compare formatter versions with identical raw inputs and settings; distinguish Russian GigaAM v3 from the multilingual model. Each metric has its own coverage count: an absent judgement produces `null`, never a claimed zero errors or an implicit acceptance. Publisher ASR text establishes recognition accuracy; it does not establish a cleaned-text or usability target.

| Metric | Evidence | Reported value |
| --- | --- | --- |
| Recognition | Publisher or audio-reviewed verbatim words | Raw and normalized WER/CER, absolute S/D/I and word/character denominators |
| Required lexical edits | Reviewed `clean_text` | Minimum word edits per 100 reference words; a proxy rather than measured keyboard actions |
| Ready without mandatory edits | Output-specific human judgement | Accepted / reviewed outputs |
| Editing effort | Measured editing time for that output | Seconds per audio minute, with timed-record coverage |
| Terms | Explicit term spans in reviewed verbatim text | Correct whole mentions / reference mentions, extra output mentions |
| Meaning and formatting damage | Output-specific human adjudication | Affected records and confirmed errors by kind |
| Preservation of correct input | Verbatim-correct fixed input through the formatter | Minimum lexical deviation from verbatim or approved clean text; unchanged speech and approved cleanup are both allowed |
| Punctuation and paragraphs | Human review | Punctuation edit counts and keep/split/merge/mixed judgements |
| Latency | Same model context and hardware | Warm median/p95, cold first inference, first-clip repeat, model load and RTF |

Term annotations use `terms: [{"text":"API", "accepted":["API"]}]` alongside an approved ASR target. Each term must occur in that target. Matching uses normalized complete token sequences, counts repeated mentions and avoids double counting overlapping accepted spellings of the same term. Extra mentions are flags; false substitutions and semantic correctness require separate review.

Pass `--reviews output-reviews.jsonl` for human output judgements. Each reviewed row needs `id`, `status=reviewed`, `reviewer`, `method`, `run_sha256` (the complete raw JSONL run's SHA-256) and `output_sha256` (SHA-256 of that output's exact UTF-8 text). This prevents an assessment from silently applying to another model or later output. Optional independent fields are `ready_without_edits` (boolean), `editing_seconds`, `mandatory_word_edits`, `punctuation_edits`, `false_term_replacements` (nonnegative counts), `meaning_errors` and `postprocess_damage` (arrays of `kind`/`note` entries), and `paragraph_judgement`. An empty adjudicated error array means reviewed with no errors; an omitted array means unreviewed. Keep these files private.

Compare reports with the same manifest, annotation overlay, normalization, mode, split, record IDs, references and timing hardware:

```powershell
python scripts/speech_eval.py --dataset $dataset --results "$results\baseline.jsonl" --annotations "$results\reviewed.jsonl" --reviews "$results\baseline-reviews.jsonl" --output "$results\baseline-report.json"
python scripts/speech_eval.py --dataset $dataset --results "$results\candidate.jsonl" --annotations "$results\reviewed.jsonl" --reviews "$results\candidate-reviews.jsonl" --output "$results\candidate-report.json"
python scripts/compare_speech_eval.py --baseline "$results\baseline-report.json" --candidate "$results\candidate-report.json" --output "$results\paired-progress.json"
```

The comparison reports per-language/source/duration deltas and improved/regressed/unchanged records. Formatter comparisons also require exactly the same input text for every paired record; correcting the candidate's input would confound the formatter change with an earlier recognition or editing change. Separate input files may have different hashes when their selected texts are identical.

Human metrics use only records with both judgements for that metric, so newly annotated easy records cannot fabricate progress. WER deltas include a deterministic sentence-group bootstrap interval and cluster count; small samples and training overlap remain limitations. When new labels arrive, re-score both saved runs against the same overlay without repeating inference.

Set cycle acceptance targets before running candidates. Practical starting targets are 20% fewer required edits and editing time, 10 percentage points more ready outputs, zero newly confirmed meaning failures, and at most 10% higher fast-mode p95. These are goals, not measured gains: evaluate them only where paired coverage exists. Retain a stable monitoring set and use fresh, previously unseen sentence groups for the next acceptance control after a prior holdout has been inspected.

Automate runs, model disagreement checks, annotation drafts and prioritization. Multi-model agreement can suggest an easy record but never promotes it to reviewed ground truth; shared errors and orthographic variants remain possible. Ask the user for small batches of disputed clips, preserving verbatim wording before proposing punctuation or cleanup.

For a fragment-only review, use `status=fragments_reviewed`, an empty `reviewed_targets` list and `reviewed_fragments` entries with a unique `id` and `asr_text`. An entry can also have a separately approved `clean_text` and `clean_exclusions` (phrases forbidden in that edited target). Raw STT checks the spoken fragment; format runs use the clean fragment when present. Reports count complete normalized phrase matches and reviewed exclusions, with their own fragment denominator. This scoped match metric is not whole-utterance WER or an output usability judgement, and it does not approve the surrounding draft.

Retain actual spoken restarts in ASR references even when the user requests their removal in cleaned text. Promote a reconstructed whole utterance to reviewed ASR only after the remaining context is confirmed; punctuation and timed editing stay independent. The next draft queue skips records with approved fragments so the same questions are not sent again.

`python scripts/speech_eval_annotation.py --dataset $dataset --run "$results\ultra.jsonl" --run "$results\challenger.jsonl" --annotations "$results\reviewed.jsonl" --output "$results\drafts.jsonl"` validates completed raw runs and prepares private proposals with Ultra as the primary transcript. Drafts include source run hashes, alternative texts and disagreement flags, skip reviewed ASR targets or previously reviewed fragments and count hint variants of one model as one model. `unanimous_normalized` describes agreement, never confidence or approval; all generated rows retain `status=proposed`.

## Local audio review page

The annotation page shows a playable recording, the full Ultra transcript with disputed spans, selectable alternatives and free corrections. Keep batches small. Previously selected chat answers may be loaded as drafts; they do not approve the surrounding words. The reviewer saves each complete recording after listening, or saves only resolved fragments when some speech remains unclear. Verbatim repeats stay in the ASR target; a later clean-text review can approve removing them.

Build the separate page with the repository's Node version and already installed desktop dependencies, from `desktop/`:

```powershell
pnpm exec tsc -p ../scripts/speech_eval_web/tsconfig.json
pnpm exec vite build --config ../scripts/speech_eval_web/vite.config.mjs --configLoader native
Push-Location ../scripts/speech_eval_web
node ../../desktop/node_modules/eslint/bin/eslint.js --config eslint.config.mjs src.tsx --max-warnings 0
Pop-Location
```

Prepare a private catalog outside Git from validated Ultra development results. Bind it to the frozen manifest SHA-256 and retain each source run hash. Its batches contain record IDs, complete `ultra_text`, transcript `parts` (literal text or a `question_id`) and questions with a source phrase, options and a default answer. Each question occupies exactly one span; together the parts must cover the entire recording text. Use `verb`, `first_term` or `second_term` for translated question captions, or provide another readable caption. Include canonical `terms` only when those spellings have been explicitly agreed. Use a new review directory when changing the catalog or its seed.

Start the server from the repository root; the seed is an existing reviewed annotation overlay, or an empty UTF-8 file:

```powershell
python scripts/speech_eval_web.py --dataset $dataset --catalog "$results\annotation-catalog.json" --seed "$results\reviewed.jsonl" --reviews "$results\web-reviews" --assets tools/speech-eval/annotation-site/dist --port 8765
```

Open `http://127.0.0.1:8765` on the same computer. The server binds only loopback, serves only catalog audio and built assets, verifies audio hashes before serving, supports seekable WAV playback and rejects cross-origin writes. Audio, transcripts, drafts and answers stay local; this page is not a hosted Site and must not be deployed with personal data. Closing the page leaves the server running; stop the foreground server with Ctrl+C when finished.

Saving writes an atomic revision history in `reviews.json` and rebuilds `annotations-web.jsonl`, preserving reviewed seed records outside the current batch. A stale tab cannot overwrite a later revision. A complete save approves only the ASR word sequence, while a partial save approves only resolved spans. Free full-text corrections take precedence over fragment choices; punctuation, cleanup, editing time and output usability are not inferred. Browser drafts survive navigation and reload when local storage is available, but disk review history is the source of approved references.

Re-score both saved runs with the same `--annotations "$results\web-reviews\annotations-web.jsonl"` file, then use the paired comparison command above. Copy that overlay to a new immutable cycle snapshot when fixing the evaluation cohort; the live export changes as reviews arrive. Run `pnpm i18n:check` from `desktop/` and the Python tool tests below when changing this page or its saving behavior.

## Tool checks

Run `python -m unittest discover -s scripts -p '*speech_eval*.py'` and `cargo test --locked --example model_benchmark` after modifying the runner or scorer. The existing [formatter corpus checks](testing.md#formatter-corpus-tests) remain the behavior gate; the evaluator extends them and the public-fixture benchmark rather than introducing another formatter or speech implementation.
