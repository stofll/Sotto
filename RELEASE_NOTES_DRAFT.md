Sotto now gives you control over long recordings, makes local models easier to compare, and shows a clearer picture of your dictation activity. This update also improves Windows paste reliability and moves older app data to its new location automatically.

## New and improved

- Choose a recording limit in **Settings → Advanced**: 5, 10, 15, 30, or 60 minutes, or no limit. The default is 15 minutes. When the limit is reached, Sotto stops and transcribes the recording; the pill and glow overlays show a countdown beforehand.
- The model catalog compares all 21 built-in models using CPU reference speeds for the selected language. These are comparison guides, not predicted timings on your computer. Downloads are blocked when known free disk space is insufficient, including a check of the remaining size before a model bundle starts downloading.
- **Statistics** now applies the selected period consistently to its main counters and processing breakdown. Estimated time saved subtracts speech and processing time from estimated typing time, excluding long pauses where they could be measured. LLM fallback reasons remain explicitly marked as all-time totals.
- Graphite is the default overlay color when no color was saved. On Windows, left-clicking the tray icon opens the main window on the tab you left open; right-clicking shows a native **Quit** menu.
- Redesigned installation experiences for **Windows and macOS**: Windows gets a branded setup window with animated background waves, light and dark themes, Russian and English, and installation-folder and shortcut options. macOS gets a new disk-image background and a refreshed drag-to-Applications layout. The Windows setup UI is currently a separately built candidate; standard release downloads and in-app updates still use the existing NSIS installer.
- A **What's new** dialog shows release notes after updates and can use notes cached before installation. The first launch with this feature establishes the starting version without opening the dialog.

## Fixed

- Windows paste work no longer holds up the app's main window, and successive dictations are pasted in the order their delivery is queued. History cleanup runs after delivery.
- Cloud formatting works when a provider's optional base URL is blank. Requests carrying API keys no longer follow redirects to another server or from HTTPS to HTTP, and cloud speech errors no longer put provider response text into the overlay or history.
- History retention changes take effect when saved. Cleanup waits until settings can be read. Legacy history and statistics files are imported only once, preventing deleted history from reappearing and older statistics from overwriting newer activity after a restart.
- Native status and error messages now follow the selected Russian or English interface language. The statistics activity chart remains legible across interface colors.

## Performance

- Reduced repeated settings reads and text-cleanup work, reused cloud speech connections between dictations, and removed periodic history polling. Model download progress updates are limited to ten per second to reduce UI work.

## Update notes

- Recordings now stop automatically after **15 minutes** by default. Select a longer limit or **No limit** in **Settings → Advanced** if needed.
- On first launch, Sotto automatically migrates history, statistics, logs, and diagnostic recordings from `~/.speech_to_text` to the platform's app-data location. If another copy still has the database open, migration waits for a later launch. Saved provider keys move when first used. Updates preserve your data; the Windows uninstaller removes it only if you select the delete-data option.
- Model cards no longer record timings from your dictations or model loads. Previously stored model-performance measurements are deleted on upgrade; dictation history and statistics are retained.

[Full changelog](https://github.com/stofll/Sotto/compare/v0.1.3...a202eb51d8fc8766dfa1f6c209370fb9d6a0f726)
