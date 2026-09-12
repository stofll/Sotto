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
