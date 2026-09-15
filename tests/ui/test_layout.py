"""Layout smoke checks and reviewable screenshots, not pixel-diff baselines."""

from pathlib import Path

import pytest
from playwright.sync_api import expect
from test_navigation import TABS


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_minimum_window_layout(app, page, locale, theme, output_path):
    page.set_viewport_size({"width": 1000, "height": 710})
    ui = app(config={"ui_language": locale, "theme": theme})
    shots = Path(output_path)
    shots.mkdir(parents=True, exist_ok=True)
    for tab in TABS:
        region = ui.nav(tab)
        expect(region.get_by_role("heading", level=1)).to_be_visible()
        page.evaluate("() => document.fonts.ready.then(() => true)")
        # Font metrics differ between a developer machine and the CI image, so
        # report the page and the overflow: a bare boolean makes a CI-only
        # failure look like a mystery instead of a layout to widen.
        overflow = page.get_by_test_id("main-content").evaluate(
            "e => e.scrollWidth - e.clientWidth"
        )
        assert overflow <= 1, f"{tab} overflows by {overflow}px ({locale}, {theme})"
        page.screenshot(path=str(shots / f"{tab}.png"), animations="disabled")


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_overlay_error_layout(app, page, locale, output_path):
    page.set_viewport_size({"width": 600, "height": 170})
    ui = app("overlay", config={"ui_language": locale})
    ui.emit("whisper-failed", {"message": "Synthetic error: retry the recording"})
    overlay = page.get_by_test_id("overlay")
    expect(overlay).to_contain_text("Synthetic error")
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(path=str(Path(output_path) / "overlay.png"), animations="disabled")


def test_overlay_native_geometry_and_preview(app, page, output_path):
    page.set_viewport_size({"width": 308, "height": 64})
    ui = app("overlay", config={"ui_language": "ru"})
    ui.emit("recording-started", 1)
    ui.emit("audio-level", {"level": 0.8})
    shots = Path(output_path)
    shots.mkdir(parents=True, exist_ok=True)
    shell = page.locator(".overlay-shell")
    expect(shell).to_have_css("height", "56px")
    assert shell.bounding_box()["width"] <= 308
    close = page.get_by_role("button")
    close.focus()
    expect(close).to_be_focused()
    page.screenshot(path=str(shots / "recording.png"), animations="disabled")
    page.set_viewport_size({"width": 600, "height": 150})
    ui.emit("live-preview-armed", {"session_id": 1, "armed": True})
    text = "Synthetic long draft. " * 80 + "LATEST WORDS"
    ui.emit("transcription-delta", {"session_id": 1, "text": text})
    expect(shell).to_have_css("height", "142px")
    preview = page.locator(".overlay-preview")
    expect(preview).to_contain_text("LATEST WORDS")
    assert (
        preview.evaluate("e => Math.abs(e.scrollHeight - e.clientHeight - e.scrollTop)")
        <= 1
    )
    page.screenshot(path=str(shots / "streaming.png"), animations="disabled")


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("form", ["pill", "bead"])
@pytest.mark.parametrize(
    "state", ["loading", "recording", "processing", "done", "pasted", "error"]
)
def test_overlay_forms_and_states(app, page, locale, form, state, output_path):
    bead = form == "bead" and state != "error"
    page.set_viewport_size({"width": 72 if bead else 308, "height": 72 if bead else 64})
    ui = app(
        "overlay",
        config={
            "ui_language": locale,
            "overlay": {"form": form, "palette": "graphite"},
        },
    )
    ui.emit("recording-started", 1)
    ui.emit("overlay-state", state)
    overlay = page.get_by_test_id("overlay")
    expect(overlay).to_have_attribute("data-layout", "bead" if bead else "compact")
    shell = page.locator(".overlay-shell").bounding_box()
    assert shell["width"] <= page.viewport_size["width"]
    assert shell["height"] <= page.viewport_size["height"]
    page.mouse.move(0, 0)
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(path=str(Path(output_path) / "overlay.png"), animations="disabled")


@pytest.mark.parametrize(
    "size,window,shell", [("s", 64, 56), ("m", 72, 64), ("l", 80, 72)]
)
def test_bead_sizes_hover_and_focus(app, page, size, window, shell, output_path):
    page.set_viewport_size({"width": window, "height": window})
    ui = app("overlay", config={"overlay": {"form": "bead", "size": size}})
    ui.emit("recording-started", 1)
    expect(page.locator(".overlay-shell")).to_have_css("width", f"{shell}px")
    button = page.get_by_role("button")
    button.focus()
    expect(button).to_be_focused()
    expect(button).to_have_css("opacity", "1")
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(
        path=str(Path(output_path) / "bead-focus.png"), animations="disabled"
    )


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("streaming", [False, True])
@pytest.mark.parametrize("size", ["s", "m", "l"])
def test_pill_sizes_and_long_content(app, page, locale, streaming, size, output_path):
    sizes = {
        "s": (520, 138) if streaming else (280, 60),
        "m": (600, 150) if streaming else (308, 64),
        "l": (680, 174) if streaming else (360, 72),
    }
    width, height = sizes[size]
    page.set_viewport_size({"width": width, "height": height})
    ui = app("overlay", config={"ui_language": locale, "overlay": {"size": size}})
    ui.emit("recording-started", 1)
    if streaming:
        ui.emit("live-preview-armed", {"session_id": 1, "armed": True})
        ui.emit(
            "transcription-delta",
            {"session_id": 1, "text": "Long synthetic draft. " * 80},
        )
        expect(page.locator(".overlay-preview")).to_be_visible()
    else:
        ui.emit(
            "paste-done",
            {
                "session_id": 1,
                "length": 128,
                "ai_processing": {
                    "fallback": True,
                    "skipped_reason": "provider_timeout",
                },
            },
        )
        expect(page.locator(".overlay-result")).to_be_visible()
        warning = page.locator(".overlay-result > div").last
        assert warning.evaluate("e => e.scrollHeight <= e.clientHeight + 1")
    button = page.get_by_role("button").bounding_box()
    assert button["x"] >= 0 and button["x"] + button["width"] <= width
    assert button["y"] >= 0 and button["y"] + button["height"] <= height
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(path=str(Path(output_path) / "content.png"), animations="disabled")
