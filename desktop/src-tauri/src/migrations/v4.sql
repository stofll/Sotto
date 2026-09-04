-- v4: repair history rows written by the old "Повторить LLM" path.
--
-- Until this migration, `retry_history_ai_processing` wrote the two JSON
-- columns the other way round from the live dispatcher: `{"text": …}` went
-- into `ai_processing_json` and the `AiStatus` into `processing_stats_json`,
-- while the `text` column was never updated at all. The frontend reads
-- `attempted` / `used` / `provider_error` off `ai_processing_json`, so those
-- rows rendered as if the LLM had never run — and the cleaned text the
-- provider actually returned sat unused in a column nobody reads.
--
-- The discriminator is exact: `AiStatus` has no `text` field, and the old
-- retry blob had nothing else, so a `$.text` string in `ai_processing_json`
-- next to an `$.attempted` in `processing_stats_json` can only be a row this
-- bug produced.
--
-- Not recoverable: the per-stage timings (`audio_seconds`, `whisper_seconds`)
-- were overwritten by the status blob and are gone. `llm_seconds` is
-- reconstructed from the status's own `elapsed_seconds`, which makes it equal
-- to the total — honest, if incomplete.
UPDATE history
SET text = json_extract(ai_processing_json, '$.text'),
    -- SQLite's length() counts characters on TEXT, matching the
    -- chars().count() the Rust writers use.
    length = length(json_extract(ai_processing_json, '$.text')),
    ai_processing_json = processing_stats_json,
    processing_stats_json = json_object(
        'llm_seconds', json_extract(processing_stats_json, '$.elapsed_seconds'),
        'total_seconds', json_extract(processing_stats_json, '$.elapsed_seconds')
    )
WHERE json_valid(ai_processing_json)
  AND json_valid(processing_stats_json)
  AND json_type(ai_processing_json, '$.text') = 'text'
  AND json_type(processing_stats_json, '$.attempted') IS NOT NULL;
