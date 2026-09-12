import pytest
from playwright.sync_api import expect


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_tray_record_stop_complete(app, page, locale):
    ui = app("tray", config={"ui_language": locale})
    button = page.get_by_test_id("tray-record")
    start, stop = (
        ("Начать запись", "Остановить запись")
        if locale == "ru"
        else ("Start recording", "Stop recording")
    )
    expect(button).to_contain_text(start)
    button.click()
    expect(button).to_contain_text(stop)
    button.click()
    expect(
        page.get_by_text("Распознаю" if locale == "ru" else "Transcribing", exact=False)
    ).to_be_visible()
    ui.emit("paste-done", {"session_id": 1, "length": 12})
    expect(button).to_contain_text(start)
    assert len(ui.calls("start_recording")) == 1
    assert len(ui.calls("stop_recording")) == 1


def test_tray_start_failure_can_retry(app, page):
    app("tray", responses={"start_recording": [{"error": "Microphone unavailable"}]})
    page.get_by_test_id("tray-record").click()
    expect(page.get_by_role("alert")).to_have_text("Microphone unavailable")
    page.get_by_test_id("tray-record").click()
    expect(page.get_by_test_id("tray-record")).to_contain_text("Остановить запись")
    expect(page.get_by_role("alert")).not_to_be_visible()


def test_tray_pause_replacements(app, page):
    ui = app("tray")
    page.get_by_role("menuitem", name="Пауза замен", exact=True).click()
    expect(
        page.get_by_role("menuitem", name="Возобновить замены", exact=True)
    ).to_be_visible()
    ui.saved("replacements_paused", True)
    page.get_by_role("menuitem", name="Возобновить замены", exact=True).click()
    ui.saved("replacements_paused", False)


@pytest.mark.parametrize(
    "label,tab",
    [("Настройки", "settings"), ("Статистика", "stats"), ("Справка", "info")],
)
def test_tray_navigation(app, page, label, tab):
    ui = app("tray")
    page.get_by_role("menuitem", name=label).click()
    page.wait_for_function(
        "window.__sottoTest.calls.some(x => x.command === 'focus_main_window')"
    )
    assert ui.calls("focus_main_window")[-1]["args"]["tab"] == tab


def test_overlay_full_lifecycle_and_stale_events(app, page):
    ui = app("overlay")
    overlay = page.get_by_test_id("overlay")
    expect(overlay).not_to_be_visible()
    ui.emit("recording-started", 10)
    expect(overlay).to_have_attribute("data-state", "recording")
    ui.emit("paste-done", {"session_id": 9, "length": 999})
    expect(overlay).to_have_attribute("data-state", "recording")
    ui.emit("recording-stopped", 10)
    expect(overlay).to_have_attribute("data-state", "processing")
    ui.emit("whisper-done", {"session_id": 10, "text": "Synthetic speech"})
    expect(overlay).to_have_attribute("data-state", "done")
    ui.emit("paste-done", {"session_id": 10, "length": 16})
    expect(overlay).to_have_attribute("data-state", "pasted")
    expect(overlay).to_contain_text("16")
    ui.emit("overlay-reset")
    expect(overlay).not_to_be_visible()
    ui.emit("recording-started", 11)
    expect(overlay).to_have_attribute("data-state", "recording")
    expect(overlay).not_to_contain_text("999")


@pytest.mark.parametrize("event", ["whisper-empty", "whisper-cancelled"])
def test_overlay_terminal_event_allows_next_recording(app, page, event):
    ui = app("overlay")
    ui.emit("recording-started", 1)
    ui.emit(event, 1)
    expect(page.get_by_test_id("overlay")).not_to_be_visible()
    ui.emit("recording-started", 2)
    expect(page.get_by_test_id("overlay")).to_have_attribute("data-state", "recording")


@pytest.mark.parametrize(
    "event", ["whisper-failed", "paste-failed", "whisper-load-failed"]
)
def test_overlay_error_and_recovery(app, page, event):
    ui = app("overlay")
    ui.emit("recording-started", 1)
    ui.emit(event, {"session_id": 1, "message": "Synthetic pipeline failure"})
    expect(page.get_by_test_id("overlay")).to_have_attribute("data-state", "error")
    expect(page.get_by_test_id("overlay")).to_contain_text("Synthetic pipeline failure")
    ui.emit("recording-started", 2)
    expect(page.get_by_test_id("overlay")).to_have_attribute("data-state", "recording")
    expect(page.get_by_test_id("overlay")).not_to_contain_text(
        "Synthetic pipeline failure"
    )


def test_overlay_escape_cancels_active_session(app, page):
    ui = app("overlay")
    ui.emit("recording-started", 42)
    page.keyboard.press("Escape")
    expect(page.get_by_test_id("overlay")).not_to_be_visible()
    assert ui.calls("cancel_recording")[-1]["args"]["sessionId"] == 42


def test_overlay_streaming_preview(app, page):
    ui = app("overlay")
    ui.emit("recording-started", 1)
    ui.emit("live-preview-armed", {"session_id": 1, "armed": True})
    ui.emit("transcription-delta", {"session_id": 1, "text": "Synthetic live preview"})
    expect(page.get_by_test_id("overlay")).to_contain_text("Synthetic live preview")
    ui.emit("recording-stopped", 1)
    expect(page.get_by_test_id("overlay")).not_to_contain_text("Synthetic live preview")


def test_tray_updates_locale_from_settings_event(app, page):
    ui = app("tray")
    expect(page.get_by_test_id("tray-record")).to_contain_text("Начать запись")
    ui.emit(
        "config-updated",
        {**ui.state()["config"], "ui_language": "en", "replacements_paused": True},
    )
    expect(page.get_by_test_id("tray-record")).to_contain_text("Start recording")
    expect(
        page.get_by_role("menuitem", name="Resume replacements", exact=True)
    ).to_be_visible()


def test_overlay_late_mount_reads_current_state(app, page):
    app("overlay", overlay_state="processing")
    expect(page.get_by_test_id("overlay")).to_have_attribute("data-state", "processing")
