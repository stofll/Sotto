# First launch

A new installation opens a four-step introduction in the main window: how dictation works, a local speech model, the shortcut and microphone, then optional preferences. The model shortlist uses the current catalogue and speech language; Linux offers Whisper only.

**Download and continue** starts the existing verified model download in the background. You can configure the shortcut and microphone while it runs. After the introduction, a dismissible card shows download progress or explains how to dictate into another application's text field.

**Skip step** advances without changing that step's settings or starting a model download. **Skip introduction** finishes the whole flow; if no model is available and no download is running, it opens Models. Closing the window hides it in the tray and preserves the current step. A download already started continues when the window is hidden or the introduction is skipped.

Existing installations with saved settings or a downloaded model go straight to the regular interface after an update. An unfinished introduction resumes at its saved step. Use **Help → Repeat introduction** to open it again. Automatic release notes wait until the next launch when the introduction is shown.

Local recognition remains the default. Cloud recognition and LLM processing require separate configuration.

Repeating the introduction preserves an existing cloud route when skipped; successfully activating a chosen local model switches recognition back to local while keeping provider settings. A pending or failed download leaves the previous route available.

Autostart is omitted in portable mode; macOS shows the existing Accessibility check alongside microphone setup.

Turning off telemetry opens an explanation shared with **Settings → Advanced**. **Turn off**, the close button, Escape and clicking outside the dialog save the opt-out; **Keep enabled** preserves the previous setting. Skipping the introduction does not change telemetry. See [Privacy](privacy.md) for storage and network details.
