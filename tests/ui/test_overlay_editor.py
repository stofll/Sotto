from pathlib import Path

import pytest
from playwright.sync_api import expect

RECIPE = {"shell": "pill", "slots": {"center": "level"}, "draw": {"level": "wave"}}


def open_editor(page):
    page.get_by_test_id("overlay-disclosure").locator("summary").click()
    page.get_by_role("button", name="Открыть конструктор", exact=True).click()
    return page.get_by_test_id("overlay-editor")


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
