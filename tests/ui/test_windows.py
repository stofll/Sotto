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


def test_tray_uses_saved_interface_color_and_live_updates(app, page):
    ui = app("tray", config={"ui_accent": "#102040", "theme": "dark"})
    root = page.locator("html")
    expect(root).to_have_css("--accent", "#102040")
    ui.emit("config-updated", {"ui_accent": "#3dc97c", "theme": "light"})
    expect(root).to_have_css("--accent", "#3dc97c")
    expect(root).to_have_attribute("data-theme", "light")
    ui.emit("config-updated", {"theme": "dark"})
    expect(root).to_have_css("--accent", "#e68a3d")
    expect(root).to_have_attribute("data-theme", "dark")


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


def test_overlay_audio_lifetime_and_reduced_motion(app, page):
    page.emulate_media(reduced_motion="reduce")
    ui = app("overlay")
    ui.emit("recording-started", 1)
    ui.emit("audio-level", {"level": 1})
    bars = page.locator(".overlay-waveform > span")
    expect(bars).to_have_count(24)
    expect(bars.last).to_have_css("height", "26px")
    expect(bars.last).to_have_css("transition-duration", "0s")
    ui.emit("audio-level", {"level": -1})
    expect(bars.last).to_have_css("height", "5px")
    ui.emit("recording-stopped", 1)
    page.wait_for_function("window.__sottoTest.subscriptions['audio-level'] === 0")
    assert (
        page.locator(".overlay-shell").evaluate(
            "e => getComputedStyle(e, '::before').animationName"
        )
        == "none"
    )
    ui.emit("overlay-reset")
    ui.emit("recording-started", 2)
    expect(bars).to_have_count(24)
    ui.emit("audio-level", {"level": 0.25})
    expect(bars.last).to_have_css("height", "16px")


def test_overlay_timer_stops_after_recording_and_reset(app, page):
    page.clock.install()
    ui = app("overlay")
    ui.emit("recording-started", 1)
    page.clock.run_for(2000)
    expect(page.locator(".overlay-timer")).to_have_text("00:02")
    ui.emit("recording-stopped", 1)
    page.clock.run_for(5000)
    expect(page.locator(".overlay-timer")).to_have_count(0)
    ui.emit("overlay-reset")
    expect(page.get_by_test_id("overlay")).not_to_be_visible()
    page.clock.run_for(5000)
    ui.emit("recording-started", 2)
    expect(page.locator(".overlay-timer")).to_have_text("00:00")


def test_glow_keeps_live_text_and_errors(app, page):
    page.set_viewport_size({"width": 400, "height": 112})
    ui = app("overlay", config={"overlay": {"form": "glow"}})
    overlay = page.get_by_test_id("overlay")
    ui.emit("recording-started", 1)
    ui.emit("live-preview-armed", {"session_id": 1, "armed": True})
    ui.emit("transcription-delta", {"session_id": 1, "text": "Visible live draft"})
    expect(overlay).to_have_attribute("data-layout", "glow")
    expect(page.locator(".overlay-preview")).to_contain_text("Visible live draft")
    expect(page.locator(".overlay-beam")).to_have_count(1)
    expect(page.locator(".overlay-timer")).to_be_visible()
    ui.emit("whisper-failed", {"session_id": 1, "message": "Synthetic glow error"})
    expect(overlay).to_have_attribute("data-layout", "glow")
    expect(overlay).to_contain_text("Synthetic glow error")


def test_bead_suppresses_live_text_and_recovers_after_warning(app, page):
    ui = app("overlay", config={"overlay": {"form": "bead"}})
    overlay = page.get_by_test_id("overlay")
    ui.emit("recording-started", 1)
    ui.emit("live-preview-armed", {"session_id": 1, "armed": True})
    ui.emit("transcription-delta", {"session_id": 1, "text": "Hidden live draft"})
    expect(overlay).to_have_attribute("data-layout", "bead")
    expect(page.locator(".overlay-preview")).to_have_count(0)
    expect(page.locator(".overlay-timer")).to_have_count(0)
    ui.emit("recording-stopped", 1)
    expect(overlay).to_have_attribute("data-layout", "bead")
    ui.emit(
        "paste-done",
        {"session_id": 1, "length": 12, "ai_processing": {"fallback": True}},
    )
    expect(overlay).to_have_attribute("data-layout", "compact")
    expect(overlay).to_contain_text("Ошибка LLM")
    ui.emit("overlay-reset")
    ui.emit("recording-started", 2)
    expect(overlay).to_have_attribute("data-layout", "bead")
    expect(overlay).not_to_contain_text("Ошибка LLM")


@pytest.mark.parametrize("state", ["recording", "loading", "processing", "done"])
def test_bead_hover_cancels_active_session(app, page, state):
    page.set_viewport_size({"width": 72, "height": 72})
    ui = app("overlay", config={"overlay": {"form": "bead"}})
    ui.emit("recording-started", 21)
    ui.emit("overlay-state", state)
    button = page.get_by_role("button", name="Отменить запись", exact=True)
    page.get_by_test_id("overlay").hover()
    expect(button).to_have_css("opacity", "1")
    button.click()
    expect(page.get_by_test_id("overlay")).not_to_be_visible()
    assert ui.calls("cancel_recording")[-1]["args"]["sessionId"] == 21


@pytest.mark.parametrize(
    "answer", [{"result": False}, {"error": "Synthetic cancel failure"}]
)
def test_bead_failed_cancel_releases_button_and_allows_next_session(app, page, answer):
    ui = app("overlay", config={"overlay": {"form": "bead"}})
    ui.emit("recording-started", 1)
    ui.queue("cancel_recording", answer)
    button = page.get_by_role("button", name="Отменить запись", exact=True)
    page.get_by_test_id("overlay").hover()
    button.click()
    expect(button).to_be_enabled()
    page.wait_for_function(
        "window.__sottoTest.calls.filter(c => c.command === 'hide').length >= 2"
    )
    ui.emit("overlay-reset")
    ui.emit("recording-started", 2)
    page.get_by_test_id("overlay").hover()
    button.click()
    expect(page.get_by_test_id("overlay")).not_to_be_visible()


def test_overlay_config_event_wins_over_slow_initial_read(app, page):
    ui = app("overlay", responses={"get_config": [{"hold": True}, {"hold": True}]})
    ui.emit(
        "config-updated",
        {"ui_language": "en", "overlay": {"form": "bead", "size": "l"}},
    )
    ui.settle("get_config", result={"ui_language": "ru", "overlay": {"form": "pill"}})
    ui.emit("recording-started", 1)
    overlay = page.get_by_test_id("overlay")
    expect(overlay).to_have_attribute("data-layout", "bead")
    expect(overlay).to_have_attribute("data-size", "l")
    expect(
        page.get_by_role("button", name="Cancel recording", exact=True)
    ).to_have_count(1)


def test_processing_label_stays_while_only_counter_is_delayed(app, page):
    page.clock.install()
    ui = app("overlay")
    ui.emit("recording-started", 1)
    ui.emit("recording-stopped", 1)
    label = page.locator(".overlay-progress > span").first
    expect(label).to_have_text("Обрабатываю")
    label.evaluate("e => e.dataset.identity = 'preserved'")
    ui.emit("whisper-done", {"session_id": 1})
    page.clock.run_for(200)
    expect(label).to_have_attribute("data-identity", "preserved")
    expect(page.locator(".overlay-counter")).to_have_count(0)
    page.clock.run_for(2000)
    expect(page.locator(".overlay-counter")).to_contain_text("2")
