from pathlib import Path

import pytest
from playwright.sync_api import expect

RELEASE = {
    "version": "0.2.0",
    "notes": "## New and improved\n\n- Smoother charts\n- Speech timing\n\n## Fixed\n\n- History layout",
    "url": "https://github.com/stofll/Sotto/releases/tag/v0.2.0",
}


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_whats_new_dismiss_persists(app, page, locale, theme, output_path):
    ui = app(config={"ui_language": locale, "theme": theme}, whats_new=RELEASE)
    dialog = page.get_by_role(
        "dialog", name="Что нового" if locale == "ru" else "What's new"
    )
    expect(dialog).to_be_visible()
    expect(dialog).to_contain_text("Sotto 0.2.0")
    expect(dialog.get_by_role("heading", name="Fixed", exact=True)).to_be_visible()
    expect(dialog.get_by_role("listitem")).to_have_count(3)
    page.keyboard.press("Tab")
    expect(dialog.get_by_role("button").first).to_be_focused()
    page.keyboard.press("Shift+Tab")
    expect(dialog.get_by_role("button").last).to_be_focused()
    dialog.screenshot(path=str(Path(output_path) / f"whats-new-{locale}-{theme}.png"))
    page.keyboard.press("Escape")
    expect(dialog).not_to_be_visible()
    assert ui.calls("dismiss_whats_new")[-1]["args"] == {"version": "0.2.0"}
    page.reload()
    expect(page.get_by_test_id("page-settings")).to_be_visible()
    expect(dialog).not_to_be_visible()


def test_whats_new_failed_dismiss_and_link_retry(app, page):
    ui = app(whats_new=RELEASE)
    dialog = page.get_by_role("dialog")
    ui.queue("open_url", {"error": "Synthetic browser failure"})
    dialog.get_by_role("button", name="Релиз на GitHub").click()
    expect(dialog.get_by_role("alert")).to_contain_text("Не удалось открыть браузер")
    dialog.get_by_role("button", name="Релиз на GitHub").click()
    assert ui.calls("open_url")[-1]["args"]["url"] == RELEASE["url"]
    ui.queue("dismiss_whats_new", {"error": "Synthetic disk failure"})
    page.keyboard.press("Escape")
    expect(dialog.get_by_role("alert")).to_contain_text("Не удалось сохранить отметку")
    expect(dialog).to_be_visible()
    dialog.get_by_role("button", name="Закрыть", exact=True).last.click()
    expect(dialog).not_to_be_visible()


def test_whats_new_waits_for_recording(app, page):
    ui = app(responses={"get_whats_new": [{"hold": True}]})
    ui.emit("recording-started", 1)
    ui.settle("get_whats_new", result=RELEASE)
    expect(page.get_by_role("dialog")).not_to_be_visible()
    ui.emit("whisper-empty", 1)
    expect(page.get_by_role("dialog")).to_be_visible()
    assert not ui.calls("dismiss_whats_new")


@pytest.mark.parametrize("response", [{"result": None}, {"error": "Offline"}])
def test_whats_new_empty_or_offline_is_quiet(app, page, response):
    ui = app(responses={"get_whats_new": [response]})
    expect(page.get_by_role("dialog")).not_to_be_visible()
    ui.nav("info")
    expect(
        page.get_by_text("Установлена последняя версия.", exact=True)
    ).to_be_visible()
    assert not ui.calls("dismiss_whats_new")


def test_whats_new_remote_markup_is_inert(app, page):
    app(
        whats_new={
            **RELEASE,
            "notes": '<img src="https://invalid.example/pixel">\n\n![Image](https://invalid.example/pixel)',
        }
    )
    dialog = page.get_by_role("dialog")
    expect(dialog).to_contain_text('<img src="https://invalid.example/pixel">')
    expect(dialog.locator("img, iframe, script")).to_have_count(0)
