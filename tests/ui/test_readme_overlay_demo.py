"""Opt-in recording of the real overlay constructor with synthetic app state."""

import os
import re
from pathlib import Path

import pytest
from playwright.sync_api import expect

pytestmark = pytest.mark.skipif(
    "SOTTO_OVERLAY_DEMO_DIR" not in os.environ,
    reason="set SOTTO_OVERLAY_DEMO_DIR to record the README overlay demo",
)


@pytest.fixture(scope="session")
def browser_context_args(browser_context_args):
    return {
        **browser_context_args,
        "viewport": {"width": 1280, "height": 820},
        "record_video_size": {"width": 1280, "height": 820},
        "record_video_dir": os.environ["SOTTO_OVERLAY_DEMO_DIR"],
    }


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_record_overlay_constructor(app, page, locale):
    output = Path(os.environ["SOTTO_OVERLAY_DEMO_DIR"])
    output.mkdir(parents=True, exist_ok=True)
    ui = app(
        config={
            "ui_language": locale,
            "theme": "dark",
            "overlay": {"palette": "graphite"},
        }
    )

    def name(ru, en):
        return ru if locale == "ru" else en

    def click(locator, pause=1100):
        locator.click()
        page.mouse.move(1260, 800)
        # Deliberate reading time in a public demo, not synchronization.
        page.wait_for_timeout(pause)

    click(page.get_by_test_id("overlay-disclosure").locator("summary"), 500)
    click(
        page.get_by_role(
            "button",
            name=name("Открыть конструктор", "Open the constructor"),
            exact=True,
        )
    )
    editor = page.get_by_test_id("overlay-editor")
    expect(editor).to_be_visible()
    page.evaluate("() => document.fonts.ready.then(() => true)")
    page.screenshot(path=str(output / f"overlay-{locale}-dark.png"))

    click(editor.locator(".ove-shell").filter(has_text=name("Бусина", "Bead")))
    click(
        editor.get_by_role(
            "button", name=re.compile(name(r"^Уровень: Матрица\.", r"^Level: Matrix\."))
        ),
        1800,
    )
    click(editor.get_by_role("button", name=name("Стиль", "Style"), exact=True), 500)
    click(
        editor.get_by_role("button", name=name("Графит", "Graphite"), exact=True),
        1800,
    )
    click(
        editor.get_by_role(
            "button", name=name("Сохранить как шаблон", "Save as template"), exact=True
        ),
        500,
    )
    dialog = page.get_by_role("dialog")
    dialog.get_by_role("textbox").fill(name("Моя матрица", "My matrix"))
    page.wait_for_timeout(700)
    click(dialog.get_by_role("button", name=name("Сохранить", "Save"), exact=True))
    assert len(ui.state()["config"]["overlay"]["templates"]) == 1

    click(editor.get_by_role("button", name=name("Сборка", "Build"), exact=True), 500)
    click(editor.locator(".ove-shell").filter(has_text=name("Пилюля", "Pill")))
    click(
        editor.get_by_role(
            "button", name=re.compile(name(r"^Уровень: Пиксели\.", r"^Level: Pixels\."))
        )
    )
    click(
        editor.get_by_role(
            "button",
            name=re.compile(name(r"^Черновик: Ровный текст\.", r"^Draft: Even text\.")),
        )
    )
    assert ui.state()["config"]["overlay"]["recipe"]["slots"]["below"] == "draft"
    click(
        editor.locator(".ove-phases").get_by_role(
            "button", name=name("Стриминг", "Streaming"), exact=True
        ),
        2600,
    )
    click(editor.get_by_role("button", name=name("Стиль", "Style"), exact=True), 500)
    click(editor.get_by_role("button", name=name("Графит", "Graphite"), exact=True))
    click(editor.get_by_role("button", name=name("Светлая", "Light"), exact=True), 1800)
    page.screenshot(path=str(output / f"overlay-{locale}-card.png"))
    click(
        editor.locator(".ove-phases").get_by_role(
            "button", name=name("Обработка", "Processing"), exact=True
        ),
        1800,
    )
    click(
        editor.locator(".ove-phases").get_by_role(
            "button", name=name("Вставка", "Inserted"), exact=True
        ),
        1800,
    )
    click(
        editor.get_by_role(
            "button", name=name("Сохранить как шаблон", "Save as template"), exact=True
        ),
        500,
    )
    dialog.get_by_role("textbox").fill(name("Мои субтитры", "My captions"))
    click(dialog.get_by_role("button", name=name("Сохранить", "Save"), exact=True))
    assert len(ui.state()["config"]["overlay"]["templates"]) == 2
    click(editor.get_by_role("button", name=name("Готово", "Ready"), exact=True), 2000)
    expect(editor).not_to_be_visible()
    click(page.get_by_test_id("overlay-disclosure").locator("summary"), 500)
    mine = page.locator(".ovt-mine")
    mine.scroll_into_view_if_needed()
    click(
        mine.get_by_role("button", name=name("Моя матрица", "My matrix"), exact=True),
        1800,
    )
    assert ui.state()["config"]["overlay"]["recipe"]["shell"] == "bead"
    click(
        mine.get_by_role(
            "button", name=name("Мои субтитры", "My captions"), exact=True
        ),
        1800,
    )
    assert ui.state()["config"]["overlay"]["recipe"]["slots"]["below"] == "draft"
    page.screenshot(path=str(output / f"overlay-{locale}-templates.png"))
    expect(page.locator("html")).to_have_attribute("data-theme", "dark")
    (output / f"overlay-{locale}-video.txt").write_text(str(page.video.path()))
