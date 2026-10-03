"""Opt-in synthetic screenshots for the public README and website."""

import os
from datetime import datetime, timezone
from pathlib import Path

import pytest
from playwright.sync_api import expect
from readme_models import assessments, catalog

pytestmark = pytest.mark.skipif(
    "SOTTO_README_SHOTS_DIR" not in os.environ,
    reason="set SOTTO_README_SHOTS_DIR to regenerate the README screenshots",
)


@pytest.fixture
def browser_context_args(browser_context_args):
    return {**browser_context_args, "device_scale_factor": 2}


MODELS, REVISIONS = catalog()
ASSESSMENTS = assessments(MODELS, REVISIONS)


CAPTURE_TIME = datetime(2026, 10, 3, 17, 30, tzinfo=timezone.utc)


def history_entries(locale):
    """Localized demonstration text; no transcripts from a user's history."""
    samples = {
        "ru": [
            "В пятницу выпустим новую версию. До этого проверим горячие клавиши, обновим документацию и соберём обратную связь.",
            "Идея для следующего спринта: добавить быстрый поиск по заметкам и сохранять последние использованные фильтры.",
            "Привет! Посмотрел макеты — первый вариант подходит. Давай оставим больше воздуха между блоками и сократим подписи.",
            "Купить кофе, забрать посылку и забронировать столик на субботу.",
        ],
        "en": [
            "We will release the new version on Friday. Before then, let's check the shortcuts, update the documentation, and collect feedback.",
            "An idea for the next sprint: add quick note search and remember the most recently used filters.",
            "Hi! I reviewed the mockups and the first option works. Let's give the blocks more space and shorten the labels.",
            "Buy coffee, pick up the parcel, and book a table for Saturday.",
        ],
    }
    return [
        {
            "id": index + 1,
            "timestamp": int(CAPTURE_TIME.timestamp()) - 600 - index * 3600,
            "text": text,
            "raw_text": text[0].lower() + text[1:-1],
            "length": len(text),
            "transcription_model": "Nemotron 3.5",
            "has_recording": index < 3,
        }
        for index, text in enumerate(samples[locale])
    ]


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["light", "dark"])
@pytest.mark.parametrize("screen", ["settings", "models", "history"])
def test_capture_readme_screenshot(app, page, locale, theme, screen):
    width, height = {
        "settings": (1200, 680),
        "models": (1280, 930),
        "history": (1200, 780),
    }[screen]
    page.set_viewport_size({"width": width, "height": height})
    page.clock.set_fixed_time(CAPTURE_TIME)
    ui = app(
        config={
            "ui_language": locale,
            "theme": theme,
            "model": "nemotron-streaming",
            "language": "auto",
        },
        models=MODELS,
        assessments=ASSESSMENTS,
        history=history_entries(locale) if screen == "history" else [],
        runtime={
            "loaded_model": "Nemotron 3.5",
            "model": "nemotron-streaming",
            "engine": "sherpa-onnx",
        },
    )
    if screen == "settings":
        expect(
            page.get_by_test_id("overlay-disclosure").locator("summary")
        ).to_be_visible()
    elif screen == "models":
        ui.nav("models")
        expect(
            page.get_by_test_id("model-gigaam-v3").get_by_role(
                "img", name="Скорость:" if locale == "ru" else "Speed:"
            )
        ).to_be_visible()
        cards = page.locator(".model-card2")
        expect(cards).to_have_count(len(MODELS))
        assert {card.get_attribute("data-testid") for card in cards.all()} == {
            f"model-{model['id']}" for model in MODELS
        }
        # Keep a normal window; leave the remaining catalog available by scrolling.
        page.get_by_role("button", name="Whisper", exact=True).click()
        page.locator(".win__main").evaluate("element => { element.scrollTop = 0; }")
        expect(page.get_by_role("heading", level=1)).to_be_in_viewport()
    else:
        ui.nav("history")
        for entry_id in (1, 2, 3):
            expect(page.get_by_test_id(f"history-play-{entry_id}")).to_be_visible()
        expect(page.get_by_test_id("history-entry-4")).to_be_visible()
        expect(page.get_by_test_id("history-play-4")).to_have_count(0)
    if locale == "en":
        # A string evaluated before the English strings loaded would stay Russian.
        # The interface-language toggle is the one Russian word that belongs here.
        cyrillic = page.evaluate("""() => {
          const copy = document.body.cloneNode(true);
          copy.querySelectorAll('button').forEach((button) => {
            if (button.textContent.trim() === 'Русский') button.remove();
          });
          return copy.textContent.match(/[А-Яа-яЁё]+/g) ?? [];
        }""")
        assert cyrillic == [], cyrillic
    page.evaluate("() => document.fonts.ready.then(() => true)")
    page.mouse.move(0, 0)
    output = Path(os.environ["SOTTO_README_SHOTS_DIR"])
    output.mkdir(parents=True, exist_ok=True)
    path = output / f"{screen}-{locale}-{theme}.png"
    page.screenshot(path=str(path), animations="disabled")
