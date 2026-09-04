-- Retain the exact model used for the primary transcription.
-- This is separate from ai_processing_json.model, which is the optional
-- post-processing provider model.
ALTER TABLE history ADD COLUMN transcription_model TEXT;
