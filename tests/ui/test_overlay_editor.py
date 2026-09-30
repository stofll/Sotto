import re
import time
from pathlib import Path

import pytest
from playwright.sync_api import expect

RECIPE = {"shell": "pill", "slots": {"center": "level"}, "draw": {"level": "wave"}}


def wait_until_still(locator, quiet_ms=200, timeout_ms=7000):
    """Wait until a box stops moving, covering size transitions and entrance motion."""
    deadline = time.monotonic() + timeout_ms / 1000
    box, since = locator.bounding_box(), time.monotonic()
    while time.monotonic() - since < quiet_ms / 1000:
        assert time.monotonic() < deadline, f"{locator} kept moving: {box}"
        locator.page.wait_for_timeout(20)
        current = locator.bounding_box()
        if current != box:
            box, since = current, time.monotonic()
    return box


def open_editor(page):
    page.get_by_test_id("overlay-disclosure").locator("summary").click()
    page.get_by_role("button", name="Открыть конструктор", exact=True).click()
    return page.get_by_test_id("overlay-editor")


def test_recipe_save_failure_rolls_back_and_same_choice_retries(app, page):
    ui = app(config={"overlay": {"recipe": RECIPE}})
    editor = open_editor(page)
    mini = editor.locator(".ove-shell").filter(has_text="Мини")
    ui.queue("save_config", {"error": "Synthetic save failure"})
    mini.click()
    expect(editor.get_by_role("alert")).to_be_visible()
    expect(editor.locator(".ove-stage .ovs")).to_have_attribute("data-shell", "pill")
    assert ui.state()["config"]["overlay"]["recipe"]["shell"] == "pill"
    mini.click()
    expect(editor.get_by_role("alert")).to_have_count(0)
    expect(editor.locator(".ove-stage .ovs")).to_have_attribute("data-shell", "mini")
    page.reload()
    editor = open_editor(page)
    expect(editor.locator(".ove-stage .ovs")).to_have_attribute("data-shell", "mini")


def test_later_recipe_edit_survives_an_earlier_save_failure(app, page):
    ui = app(config={"overlay": {"recipe": RECIPE}})
    editor = open_editor(page)
    ui.queue("save_config", {"hold": True})
    editor.locator(".ove-shell").filter(has_text="Мини").click()
    editor.locator(".ove-shell").filter(has_text="Карточка").click()
    ui.settle("save_config", error="Synthetic save failure")
    page.wait_for_function(
        "window.__sottoTest.state.config.overlay.recipe.shell === 'card'"
    )
    expect(editor.locator(".ove-stage .ovs")).to_have_attribute("data-shell", "card")
    expect(editor.get_by_role("alert")).to_have_count(0)


def test_failed_edit_restores_last_successful_recipe(app, page):
    ui = app(config={"overlay": {"recipe": RECIPE}})
    editor = open_editor(page)
    editor.locator(".ove-shell").filter(has_text="Мини").click()
    page.wait_for_function(
        "window.__sottoTest.state.config.overlay.recipe.shell === 'mini'"
    )
    ui.queue("save_config", {"error": "Synthetic save failure"})
    editor.locator(".ove-shell").filter(has_text="Карточка").click()
    expect(editor.get_by_role("alert")).to_be_visible()
    expect(editor.locator(".ove-stage .ovs")).to_have_attribute("data-shell", "mini")


@pytest.mark.parametrize("slider", [False, True])
def test_editor_colour_failure_rolls_back_and_retries(app, page, slider):
    ui = app(
        config={"overlay": {"recipe": RECIPE, "palette": "custom", "palette_hue": 120}}
    )
    editor = open_editor(page)
    editor.get_by_role("button", name="Стиль", exact=True).click()
    hue = editor.locator(".ove-tone__range--hue")
    violet = editor.get_by_role("button", name="Фиолетовый", exact=True)
    ui.queue("save_config", {"error": "Synthetic save failure"})

    def change():
        if slider:
            hue.focus()
            hue.press("ArrowRight")
        else:
            violet.click()

    change()
    expect(editor.get_by_role("alert")).to_be_visible()
    expect(hue).to_have_value("120")
    expect(violet).to_have_attribute("aria-pressed", "false")
    change()
    expect(editor.get_by_role("alert")).to_have_count(0)
    if slider:
        expect(hue).to_have_value("121")
        assert ui.state()["config"]["overlay"]["palette_hue"] == 121
    else:
        expect(violet).to_have_attribute("aria-pressed", "true")
        assert ui.state()["config"]["overlay"]["palette"] == "violet"


def test_native_pointer_tests_the_caption_chip_not_the_transparent_window(app, page):
    page.set_viewport_size({"width": 520, "height": 138})
    page.mouse.move(0, 0)
    ui = app(
        "overlay",
        config={
            "overlay": {
                "size": "s",
                "recipe": {"shell": "caps", "slots": {"c1": "timer"}},
            }
        },
    )
    ui.emit("recording-started", 42)
    chip = page.locator(".ovs-chip")
    expect(chip).to_be_visible()
    expect(page.locator(".ovs")).to_have_attribute("data-shown", "1")
    # Read pointer coordinates only after the entrance and resizing have settled.
    expect(page.locator(".ovs")).to_have_css("transform", "none")
    bounds = wait_until_still(chip)
    ui.emit(
        "overlay-pointer",
        {
            "x": bounds["x"] + bounds["width"] / 2,
            "y": bounds["y"] + bounds["height"] / 2,
        },
    )
    close = page.get_by_role("button", name="Отменить запись", exact=True)
    expect(close).to_have_css("opacity", "1")
    ui.emit("overlay-pointer", {"x": 1, "y": 1})
    expect(close).to_have_css("opacity", "0")
    close.focus()
    expect(close).to_have_css("opacity", "1")
    page.keyboard.press("Enter")
    expect(page.get_by_test_id("overlay")).not_to_be_visible()


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["light", "dark"])
def test_captions_long_error_keeps_close_inside_window(
    app, page, locale, theme, output_path
):
    page.set_viewport_size({"width": 520, "height": 138})
    ui = app(
        "overlay",
        config={
            "ui_language": locale,
            "theme": theme,
            "overlay": {"size": "s", "recipe": {"shell": "caps"}},
        },
    )
    ui.emit("recording-started", 42)
    message = (
        "Не удалось распознать речь. Откройте «Настройки → Модели» и убедитесь, что модель скачана."
        if locale == "ru"
        else "Speech recognition failed. Open Settings → Models and make sure the selected model has been downloaded."
    )
    ui.emit("whisper-failed", {"session_id": 42, "message": message})
    expect(page.locator(".ovs")).to_have_attribute("data-phase", "error")
    expect(page.locator(".ovs")).to_have_attribute("data-shown", "1")
    # Assert the visible end state without enumerating unrelated animations;
    # awaiting their finished promises has crashed WebKit in this scenario.
    expect(page.locator(".ovs")).to_have_css("transform", "none")
    expect(page.locator(".ovs-stl")).to_have_css("opacity", "1")
    expect(page.locator(".ovs-stl")).to_have_css(
        "transform", re.compile(r"^(none|matrix\(1, 0, 0, 1, 0, 0\))$")
    )
    close = page.get_by_role(
        "button", name="Закрыть" if locale == "ru" else "Close", exact=True
    )
    close.focus()
    expect(close).to_have_css("opacity", "1")
    # The shell resize for the error outlasts the text fade-in.
    bounds = wait_until_still(close)
    assert bounds and bounds["x"] >= 0 and bounds["x"] + bounds["width"] <= 520
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(
        path=str(Path(output_path) / "captions-error.png"), animations="disabled"
    )
    hides = len(ui.calls("hide"))
    close.click()
    page.wait_for_function(
        "count => window.__sottoTest.calls.filter(call => call.command === 'hide').length > count",
        arg=hides,
    )
    ui.emit("overlay-reset")
    expect(page.get_by_test_id("overlay")).not_to_be_visible()


@pytest.mark.parametrize(
    "module,recipe",
    [
        ("OverlayScene", {"shell": "mini"}),
        (
            "OverlayGlow",
            {"shell": "card", "slots": {"edge": "level"}, "draw": {"level": "beam"}},
        ),
        (
            "OverlayMatrix",
            {"shell": "bead", "slots": {"core": "level"}, "draw": {"level": "matrix"}},
        ),
    ],
)
def test_recipe_module_failure_keeps_cancel_available(
    app, page, pytestconfig, module, recipe
):
    production = pytestconfig.getoption("--ui-mode") == "production"
    pattern = (
        f"**/assets/{module}-*.js" if production else f"**/src/overlay/{module}.tsx*"
    )
    page.set_viewport_size({"width": 148, "height": 44})
    ui = app("overlay", config={"overlay": {"recipe": recipe}})
    ui.allow_asset_failure(pattern)
    page.route(pattern, lambda route: route.abort())
    with page.expect_event(
        "requestfailed", predicate=lambda request: f"/{module}" in request.url
    ):
        ui.emit("recording-started", 42)
    # Wait for the failed request to be handled, rather than clicking before loading starts.
    page.wait_for_function(
        "document.querySelector('.overlay-scene-fallback[aria-busy=false]') !== null"
        if module == "OverlayScene"
        else "document.querySelector('.ovs') !== null"
    )
    close = page.get_by_role("button", name="Отменить запись", exact=True)
    close.focus()
    page.keyboard.press("Enter")
    expect(page.get_by_test_id("overlay")).not_to_be_visible()
    assert ui.calls("cancel_recording")[-1]["args"]["sessionId"] == 42
    ui.emit("recording-started", 43)
    expect(close).to_be_attached()


def test_template_save_failure_can_be_retried(app, page):
    ui = app(config={"overlay": {"recipe": RECIPE}})
    editor = open_editor(page)
    save = editor.get_by_role("button", name="Сохранить как шаблон", exact=True)
    ui.queue("save_config", {"error": "Synthetic save failure"})
    save.click()
    page.get_by_role("dialog").get_by_role(
        "button", name="Сохранить", exact=True
    ).click()
    expect(editor.get_by_role("alert")).to_be_visible()
    expect(save).to_be_enabled()
    expect(editor).not_to_contain_text("Шаблон «Мой оверлей 1» сохранён")
    save.click()
    page.get_by_role("dialog").get_by_role(
        "button", name="Сохранить", exact=True
    ).click()
    expect(editor).to_contain_text("Шаблон «Мой оверлей 1» сохранён")
    expect(save).to_be_disabled()
    assert len(ui.state()["config"]["overlay"]["templates"]) == 1


def test_header_save_respects_template_limit(app, page):
    templates = [
        {"id": str(i), "name": f"Template {i}", "recipe": {"shell": "bead"}}
        for i in range(8)
    ]
    app(config={"overlay": {"recipe": RECIPE, "templates": templates}})
    editor = open_editor(page)
    expect(
        editor.get_by_role("button", name="Сохранить как шаблон", exact=True)
    ).to_be_disabled()


def test_existing_template_update_failure_can_be_retried(app, page):
    template = {"id": "saved", "name": "Saved", "recipe": RECIPE}
    ui = app(config={"overlay": {"recipe": RECIPE, "templates": [template]}})
    page.get_by_test_id("overlay-disclosure").locator("summary").click()
    page.get_by_role("button", name="Действия с шаблоном «Saved»").click()
    page.get_by_role("menuitem", name="Изменить в конструкторе").click()
    editor = page.get_by_test_id("overlay-editor")
    editor.get_by_role("button", name="Сбросить", exact=True).click()
    save = editor.get_by_role("button", name="Сохранить в «Saved»", exact=True)
    expect(save).to_be_enabled()
    ui.queue("save_config", {"error": "Synthetic update failure"})
    save.click()
    expect(editor.get_by_role("alert")).to_be_visible()
    expect(save).to_be_enabled()
    save.click()
    expect(
        editor.get_by_role("button", name="Шаблон сохранён", exact=True)
    ).to_be_disabled()
    assert (
        ui.state()["config"]["overlay"]["templates"][0]["recipe"]["draw"]["level"]
        == "bars"
    )


@pytest.mark.parametrize("locale,theme", [("ru", "dark"), ("en", "light")])
@pytest.mark.parametrize(
    "shell,width,height",
    [
        ("pill", 280, 60),
        ("bead", 64, 64),
        ("stack", 72, 112),
        ("card", 360, 100),
        ("caps", 520, 138),
    ],
)
def test_timerless_limit_stays_inside_window(
    app, page, shell, width, height, locale, theme, output_path
):
    page.set_viewport_size({"width": width, "height": height})
    ui = app(
        "overlay",
        config={
            "ui_language": locale,
            "theme": theme,
            "overlay": {"size": "s", "recipe": {"shell": shell}},
        },
    )
    ui.emit("recording-started", 42)
    ui.emit("recording-limit", {"session_id": 42, "remaining_seconds": 60})
    badge = page.locator(".ovs-limitb")
    expect(badge).to_be_visible()
    bounds = badge.bounding_box()
    assert bounds and bounds["x"] >= 0 and bounds["y"] >= 0
    assert bounds["x"] + bounds["width"] <= width
    assert bounds["y"] + bounds["height"] <= height
    page.screenshot(path=str(Path(output_path) / f"limit-{shell}-{locale}-{theme}.png"))
    page.get_by_test_id("overlay").hover()
    close = page.get_by_role(
        "button", name="Отменить запись" if locale == "ru" else "Cancel recording"
    )
    close.focus()
    page.keyboard.press("Enter")
    expect(page.get_by_test_id("overlay")).not_to_be_visible()
    assert ui.calls("cancel_recording")[-1]["args"]["sessionId"] == 42
