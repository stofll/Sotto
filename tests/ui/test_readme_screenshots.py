"""Opt-in synthetic screenshots for the public README and website."""

import os
from pathlib import Path

import pytest
from playwright.sync_api import expect

pytestmark = pytest.mark.skipif(
    "SOTTO_README_SHOTS_DIR" not in os.environ,
    reason="set SOTTO_README_SHOTS_DIR to regenerate the README screenshots",
)


@pytest.fixture
def browser_context_args(browser_context_args):
    return {**browser_context_args, "device_scale_factor": 2}


MODELS = [
    {
        "id": "tiny",
        "label": "Whisper Tiny",
        "size": "75 MB",
        "ram": "~0.4 GB",
        "downloaded": False,
        "selected": False,
        "family": "Whisper",
        "engine": "whisper.cpp",
    },
    {
        "id": "gigaam-v3",
        "label": "GigaAM v3",
        "size": "214 MB",
        "ram": "~0.5 GB",
        "downloaded": True,
        "selected": False,
        "family": "GigaAM",
        "engine": "sherpa-onnx",
        "languages": ["ru"],
        "cpu_only": True,
        "quantization": "int8",
    },
    {
        "id": "nemotron-streaming",
        "label": "Nemotron 3.5",
        "size": "651 MB",
        "ram": "~1.4 GB",
        "downloaded": True,
        "selected": True,
        "loaded": True,
        "family": "Nemotron",
        "engine": "sherpa-onnx",
        "streaming": True,
        "cpu_only": True,
        "quantization": "int8",
    },
    {
        "id": "parakeet-tdt-v3",
        "label": "Parakeet TDT v3",
        "size": "639 MB",
        "ram": "~1.4 GB",
        "downloaded": False,
        "selected": False,
        "family": "Parakeet",
        "engine": "sherpa-onnx",
        "cpu_only": True,
        "quantization": "int8",
    },
    {
        "id": "parakeet-streaming-en",
        "label": "Parakeet unified",
        "size": "632 MB",
        "ram": "~1.4 GB",
        "downloaded": False,
        "selected": False,
        "family": "Parakeet",
        "engine": "sherpa-onnx",
        "languages": ["en"],
        "streaming": True,
        "cpu_only": True,
        "quantization": "int8",
    },
    {
        "id": "parakeet-tdt-v2-en",
        "label": "Parakeet TDT v2",
        "size": "631 MB",
        "ram": "~1.4 GB",
        "downloaded": False,
        "selected": False,
        "family": "Parakeet",
        "engine": "sherpa-onnx",
        "languages": ["en"],
        "cpu_only": True,
        "quantization": "int8",
    },
]


def assessment(model_id, score):
    return {
        "id": model_id,
        "compute": "cpu",
        "speed": {"score": score, "source": "reference"},
        "memory": {
            "score": 0.8,
            "status": "enough",
            "required_bytes": 1024**3,
            "available_bytes": 8 * 1024**3,
        },
    }


ASSESSMENTS = [
    assessment("gigaam-v3", 0.82),
    assessment("nemotron-streaming", 0.7),
    assessment("parakeet-tdt-v3", 0.88),
    assessment("parakeet-streaming-en", 0.58),
    assessment("parakeet-tdt-v2-en", 0.8),
]


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["light", "dark"])
@pytest.mark.parametrize("screen", ["settings", "models"])
def test_capture_readme_screenshot(app, page, locale, theme, screen):
    width, height = (1200, 680) if screen == "settings" else (1280, 930)
    page.set_viewport_size({"width": width, "height": height})
    ui = app(
        config={
            "ui_language": locale,
            "theme": theme,
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
    )
    if screen == "settings":
        expect(
            page.get_by_test_id("overlay-disclosure").locator("summary")
        ).to_be_visible()
    else:
        ui.nav("models")
        page.get_by_role("button", name="Whisper", exact=True).click()
        expect(
            page.get_by_test_id("model-gigaam-v3").get_by_role(
                "img", name="Скорость:" if locale == "ru" else "Speed:"
            )
        ).to_be_visible()
    page.evaluate("() => document.fonts.ready.then(() => true)")
    page.mouse.move(0, 0)
    output = Path(os.environ["SOTTO_README_SHOTS_DIR"])
    output.mkdir(parents=True, exist_ok=True)
    path = output / f"{screen}-{locale}-{theme}.png"
    page.screenshot(path=str(path), animations="disabled")
