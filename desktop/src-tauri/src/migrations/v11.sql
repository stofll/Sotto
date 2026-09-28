-- The saved recording of a dictation, as a file name inside the
-- recordings folder. NULL when recordings were off or the save failed.
ALTER TABLE history ADD COLUMN recording_file TEXT;
