from pathlib import Path

import pytest
from playwright.sync_api import expect

# Bar plus gap in overlay.css, the pitch OverlayWaveform.tsx counts bars by.
BAR_PITCH = 6


def ring_mark(page):
    """Where the tallest ring bar sits, relative to the ring's centre."""
    return page.locator(".overlay-waveform--circular").evaluate(
        "el => { const box = el.getBoundingClientRect();"
        " const tallest = [...el.children].reduce((a, b) =>"
        "   (parseFloat(b.style.height) > parseFloat(a.style.height) ? b : a));"
        " const mark = tallest.getBoundingClientRect();"
        " return { dx: mark.x + mark.width / 2 - box.x - box.width / 2,"
        "          dy: mark.y + mark.height / 2 - box.y - box.height / 2 }; }"
    )


def settled_bar_count(page):
    """Wait for the width-derived bar count and return it."""
    return page.wait_for_function(
        "pitch => { const wave = document.querySelector('.overlay-waveform');"
        " if (!wave) return null;"
        " const fit = Math.max(12, Math.floor((wave.clientWidth + pitch / 2) / pitch));"
        " return wave.children.length === fit ? fit : null; }",
        arg=BAR_PITCH,
    ).json_value()


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_tray_has_no_recording_controls(app, page, locale):
    ui = app("tray", config={"ui_language": locale})
    expect(page.get_by_role("menu")).to_be_visible()
    expect(page.get_by_test_id("tray-record")).to_have_count(0)
    ui.emit("recording-started", 1)
    expect(page.get_by_test_id("tray-record")).to_have_count(0)
    assert not ui.calls("start_recording")
    assert not ui.calls("stop_recording")


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


@pytest.mark.parametrize("os", ["windows", "macos", "linux"])
def test_tray_navigation_dismisses_popup_only_on_windows(app, page, os):
    ui = app("tray", runtime={"os": os})
    hide = page.get_by_role("button", name="Скрыть меню", exact=True)
    if os == "windows":
        expect(hide).to_be_visible()
    else:
        expect(hide).to_have_count(0)
    page.get_by_role("menuitem", name="Настройки").click()
    page.wait_for_function(
        "window.__sottoTest.calls.some(x => x.command === 'focus_main_window')"
    )
    commands = page.evaluate(
        "window.__sottoTest.calls.map(x => x.command).filter("
        "x => ['hide_tray_popup', 'focus_main_window'].includes(x))"
    )
    assert commands == (
        ["hide_tray_popup", "focus_main_window"]
        if os == "windows"
        else ["focus_main_window"]
    )
    assert ui.calls("focus_main_window")[-1]["args"]["tab"] == "settings"


def test_tray_can_dismiss_before_runtime_status_arrives(app, page):
    ui = app("tray", responses={"get_runtime_status": [{"hold": True}]})
    page.get_by_role("button", name="Скрыть меню", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.calls.some(x => x.command === 'hide_tray_popup')"
    )
    assert len(ui.calls("hide_tray_popup")) == 1


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
    expect(page.get_by_test_id("overlay")).to_have_attribute("data-state", "recording")
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
    expect(page.get_by_role("menuitem", name="Пауза замен")).to_be_visible()
    ui.emit(
        "config-updated",
        {**ui.state()["config"], "ui_language": "en", "replacements_paused": True},
    )
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
    expect(bars).to_have_count(settled_bar_count(page))
    expect(bars.last).to_have_css("transition-duration", "0s")
    # The newest reading takes the last place, so the trace fills at the
    # right edge of the pill and runs left from there.
    expect(bars.last).to_have_css("height", "26px")
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
    expect(bars).to_have_count(settled_bar_count(page))
    ui.emit("audio-level", {"level": 0.25})
    expect(bars.last).to_have_css("height", "16px")


def test_bead_traces_the_voice_clockwise_from_the_top(app, page):
    ui = app("overlay", config={"overlay": {"form": "bead"}})
    ui.emit("recording-started", 1)
    ui.emit("audio-level", {"level": 1})
    # The newest reading is drawn at twelve o'clock.
    newest = ring_mark(page)
    assert abs(newest["dx"]) < 2
    assert newest["dy"] < -8
    # The next one takes its place, so that reading moves on clockwise.
    ui.emit("audio-level", {"level": 0})
    moved = ring_mark(page)
    assert moved["dx"] > 1
    assert moved["dy"] < 0


def test_overlay_hides_timer_when_disabled(app, page):
    page.set_viewport_size({"width": 308, "height": 64})
    ui = app("overlay", config={"overlay": {"show_timer": False}})
    ui.emit("recording-started", 1)
    overlay = page.get_by_test_id("overlay")
    expect(overlay).to_have_attribute("data-timer", "off")
    expect(page.locator(".overlay-timer")).to_have_count(0)


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


def test_glow_hides_placeholder_without_live_preview(app, page):
    page.set_viewport_size({"width": 400, "height": 112})
    ui = app("overlay", config={"overlay": {"form": "glow"}})
    ui.emit("recording-started", 1)
    overlay = page.get_by_test_id("overlay")
    expect(overlay).to_have_attribute("data-layout", "glow")
    expect(page.locator(".overlay-preview")).to_have_count(0)
    expect(overlay).not_to_contain_text("Говорите")
    expect(page.locator(".overlay-beam")).to_have_count(1)


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


@pytest.mark.parametrize(
    "form,viewport",
    [
        ("pill", {"width": 308, "height": 64}),
        ("glow", {"width": 400, "height": 112}),
        ("bead", {"width": 72, "height": 72}),
    ],
)
def test_overlay_close_hidden_until_hover(app, page, form, viewport):
    # Pad the page so (0, 0) sits outside `.overlay-shell`. The overlay root
    # fills the viewport, and a synthetic pointerleave is immediately followed
    # by a real pointerenter while the cursor is still over it.
    page.set_viewport_size(
        {"width": viewport["width"] + 80, "height": viewport["height"] + 80}
    )
    page.mouse.move(0, 0)
    ui = app("overlay", config={"overlay": {"form": form}})
    ui.emit("recording-started", 1)
    button = page.get_by_role("button", name="Отменить запись", exact=True)
    overlay = page.get_by_test_id("overlay")
    if form == "glow":
        expect(page.locator(".overlay-beam")).to_have_count(1)
    page.mouse.move(0, 0)
    expect(overlay).to_have_attribute("data-hovered", "false")
    expect(button).to_have_css("opacity", "0")
    page.locator(".overlay-shell").hover()
    expect(overlay).to_have_attribute("data-hovered", "true")
    expect(button).to_have_css("opacity", "1")
    button.click()
    expect(overlay).not_to_be_visible()
    assert ui.calls("cancel_recording")[-1]["args"]["sessionId"] == 1


def test_overlay_close_shows_on_native_pointer_event(app, page):
    page.set_viewport_size({"width": 388, "height": 144})
    page.mouse.move(0, 0)
    ui = app("overlay")
    ui.emit("recording-started", 1)
    overlay = page.get_by_test_id("overlay")
    button = page.get_by_role("button", name="Отменить запись", exact=True)
    page.mouse.move(0, 0)
    expect(overlay).to_have_attribute("data-hovered", "false")
    expect(button).to_have_css("opacity", "0")
    ui.emit("overlay-pointer", True)
    expect(overlay).to_have_attribute("data-hovered", "true")
    expect(button).to_have_css("opacity", "1")


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


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["light", "dark"])
def test_pill_processing_is_centred(app, page, locale, theme):
    page.set_viewport_size({"width": 308, "height": 64})
    ui = app("overlay", config={"ui_language": locale, "theme": theme})
    ui.emit("recording-started", 8)
    ui.emit("recording-stopped", 8)
    ui.emit("whisper-done", {"session_id": 8, "text": "Synthetic speech"})
    expect(page.get_by_test_id("overlay")).to_have_attribute("data-state", "done")
    progress = page.locator(".overlay-progress")
    expect(progress).to_be_visible()
    page.wait_for_timeout(250)  # Wait for the grid's 200 ms size transition.
    surface = page.locator(".overlay-surface").bounding_box()
    bounds = progress.bounding_box()
    assert (
        abs(bounds["x"] + bounds["width"] / 2 - surface["x"] - surface["width"] / 2) < 1
    )


def test_native_pointer_leave_overrides_stale_css_hover(app, page):
    page.set_viewport_size({"width": 388, "height": 144})
    ui = app("overlay")
    ui.emit("recording-started", 1)
    shell = page.locator(".overlay-shell")
    shell.hover()
    button = page.get_by_role("button", name="Отменить запись", exact=True)
    expect(button).to_have_css("opacity", "1")
    # WKWebView can keep :hover after the native window has moved/hidden.
    ui.emit("overlay-pointer", False)
    expect(button).to_have_css("opacity", "0")
    ui.emit("overlay-reset")
    page.mouse.move(0, 0)
    ui.emit("recording-started", 2)
    expect(button).to_have_css("opacity", "0")
    ui.emit("overlay-pointer", True)
    expect(button).to_have_css("opacity", "1")


@pytest.mark.parametrize(
    "size,width,height", [("s", 280, 60), ("m", 308, 64), ("l", 360, 72)]
)
@pytest.mark.parametrize("timer", [False, True])
def test_pill_waveform_fills_space_and_makes_room_for_cancel(
    app, page, size, width, height, timer, output_path
):
    page.set_viewport_size({"width": width + 80, "height": height + 80})
    page.mouse.move(0, 0)
    ui = app(
        "overlay",
        config={"overlay": {"form": "pill", "size": size, "show_timer": timer}},
    )
    page.add_style_tag(content=f".overlay-shell {{ max-width: {width}px; }}")
    ui.emit("recording-started", 1)
    overlay = page.get_by_test_id("overlay")
    expect(overlay).to_have_attribute("data-hovered", "false")
    page.wait_for_timeout(250)
    wave = page.locator(".overlay-waveform")
    bars = wave.locator("span")
    bounds = wave.bounding_box()
    first, last = bars.first.bounding_box(), bars.last.bounding_box()
    assert abs(first["x"] - bounds["x"]) < 1
    assert abs(last["x"] + last["width"] - bounds["x"] - bounds["width"]) < 1
    # The count follows the width, so the bars keep their spacing instead of
    # being stretched apart over whatever room the row has.
    assert bars.count() == settled_bar_count(page)
    widest_pitch = wave.evaluate(
        "el => { const xs = [...el.children].map(b => b.getBoundingClientRect().x);"
        " return Math.max(...xs.slice(1).map((x, i) => x - xs[i])); }"
    )
    assert widest_pitch <= BAR_PITCH + 1
    if not timer:
        row = page.locator(".overlay-row").bounding_box()
        assert abs(bounds["width"] - row["width"]) < 1
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(path=str(Path(output_path) / "waveform.png"))
    page.locator(".overlay-shell").hover()
    cancel = page.get_by_role("button", name="Отменить запись", exact=True)
    expect(cancel).to_have_css("opacity", "1")
    page.wait_for_timeout(250)
    hovered, button = wave.bounding_box(), cancel.bounding_box()
    assert hovered["width"] < bounds["width"] - 30
    assert hovered["x"] + hovered["width"] <= button["x"]
    cancel.click()
    expect(overlay).not_to_be_visible()
    assert ui.calls("cancel_recording")[-1]["args"]["sessionId"] == 1
