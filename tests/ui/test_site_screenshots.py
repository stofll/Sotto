"""Opt-in website captures for the feature tour: the dark 2× models and history screens."""

import os
from datetime import UTC, datetime
from pathlib import Path

import pytest
from playwright.sync_api import expect
from test_readme_screenshots import ASSESSMENTS, MODELS

pytestmark = pytest.mark.skipif(
    "SOTTO_SITE_SHOTS_DIR" not in os.environ,
    reason="set SOTTO_SITE_SHOTS_DIR to regenerate the website screenshots",
)


@pytest.fixture
def browser_context_args(browser_context_args):
    # The tour zooms into regions, so the site ships only 2x originals.
    return {**browser_context_args, "device_scale_factor": 2}


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_capture_site_screenshots(app, page, locale):
    page.set_viewport_size({"width": 1088, "height": 736})
    captured_at = datetime(2026, 9, 30, 17, 30, tzinfo=UTC)
    page.clock.set_fixed_time(captured_at)
    texts = {
        "ru": [
            "Встреча переносится на четверг, 15:00. Пришлю повестку утром.",
            "Идея для заметки: собрать все вопросы к следующей встрече.",
            "Купить кофе, забрать посылку и позвонить маме.",
        ],
        "en": [
            "The meeting moves to Thursday, 3 pm. I'll send the agenda in the morning.",
            "Note to self: collect the questions for our next meeting.",
            "Buy coffee, pick up the parcel and call mom.",
        ],
    }
    ui = app(
        config={
            "ui_language": locale,
            "theme": "dark",
            "model": "nemotron-streaming",
            "language": "auto",
        },
        models=MODELS,
        assessments=ASSESSMENTS,
        runtime={
            "loaded_model": "Nemotron 3.5",
            "model": "nemotron-streaming",
            "engine": "sherpa-onnx",
        },
        history=[
            {
                "id": index + 1,
                "timestamp": int(captured_at.timestamp()) - 600 - index * 3600,
                "transcription_model": "Nemotron 3.5",
                "text": text,
                "raw_text": text,
                "length": len(text),
            }
            for index, text in enumerate(texts[locale])
        ],
    )
    page.locator(".sidebar-brand__toggle").click()
    # Open the same navigation groups before taking any of the captures.
    for group in ["core", "processing", "integrations", "data", "help"]:
        button = page.get_by_test_id(f"nav-group-{group}")
        if button.get_attribute("aria-expanded") == "false":
            button.click()

    output = Path(os.environ["SOTTO_SITE_SHOTS_DIR"])
    output.mkdir(parents=True, exist_ok=True)
    for screen in ["models", "history"]:
        ui.nav(screen)
        if screen == "models":
            page.get_by_role("button", name="Whisper", exact=True).click()
            expect(page.get_by_test_id("model-gigaam-v3")).to_be_visible()
        else:
            expect(page.get_by_test_id("history-entry-1")).to_be_visible()
        page.evaluate("() => document.fonts.ready.then(() => true)")
        page.get_by_test_id("main-content").evaluate(
            "el => { el.scrollTop = 0; for (const child of el.querySelectorAll('*')) child.scrollTop = 0; }"
        )
        page.mouse.move(0, 0)
        page.screenshot(
            path=str(output / f"{screen}-{locale}-dark@2x.png"),
            animations="disabled",
        )
