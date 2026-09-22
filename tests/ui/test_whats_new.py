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


def test_whats_new_renders_a_list_written_under_its_heading(app, page):
    # GitHub's own generated notes put the bullets directly under the heading,
    # with no blank line and with CRLF endings. Treating that whole block as
    # prose printed the leading "* " as text instead of building a list.
    app(
        whats_new={
            **RELEASE,
            "notes": "## What's Changed\r\n"
            "* Faster startup by @dev in #1\r\n"
            "* Fixed pasting\r\n"
            "\r\n"
            "See the full changelog.",
        }
    )
    dialog = page.get_by_role("dialog")
    expect(
        dialog.get_by_role("heading", name="What's Changed", exact=True)
    ).to_be_visible()
    expect(dialog.get_by_role("listitem")).to_have_count(2)
    expect(dialog.get_by_role("listitem").first).to_have_text(
        "Faster startup by @dev in #1"
    )
    expect(dialog).to_contain_text("See the full changelog.")
    expect(dialog).not_to_contain_text("* Faster")


def test_whats_new_remote_markup_is_inert(app, page):
    app(
        whats_new={
            **RELEASE,
            "notes": '<img src="https://invalid.example/pixel">\n\n'
            "![Image](https://invalid.example/pixel)\n\n"
            "[Unsafe](javascript:alert%281%29)\n\n[Local](file:///private)",
        }
    )
    dialog = page.get_by_role("dialog")
    expect(dialog).to_contain_text('<img src="https://invalid.example/pixel">')
    expect(dialog.locator("img, iframe, script")).to_have_count(0)
    expect(dialog.get_by_role("link")).to_have_count(0)


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_whats_new_rich_notes_and_safe_links(app, page, locale, theme, output_path):
    ui = app(
        config={"ui_language": locale, "theme": theme},
        whats_new={
            **RELEASE,
            "notes": "## Changes\n"
            "- **Faster** dictation\n"
            "  - Nested *detail*\n\n"
            "1. First step\n2. Second step\n\n"
            "**Full Changelog**: https://github.com/stofll/Sotto/compare/v0.1.0...v0.2.0\n\n"
            "[Release](https://github.com/stofll/Sotto/releases)",
        },
    )
    dialog = page.get_by_role("dialog")
    expect(dialog.locator("strong").first).to_have_text("Faster")
    expect(dialog.locator("ul ul li")).to_have_text("Nested detail")
    expect(dialog.locator("ol > li")).to_have_count(2)
    expect(dialog.get_by_role("link")).to_have_count(2)
    expect(dialog.locator("img, iframe, script")).to_have_count(0)
    link = dialog.get_by_role("link", name="Release", exact=True)
    link.focus()
    expect(link).to_be_focused()
    dialog.screenshot(path=str(Path(output_path) / f"rich-notes-{locale}-{theme}.png"))
    ui.queue("open_url", {"error": "Synthetic browser failure"})
    page.keyboard.press("Enter")
    expect(dialog.get_by_role("alert")).to_be_visible()
    link.click()
    expect(dialog.get_by_role("alert")).not_to_be_visible()
    assert (
        ui.calls("open_url")[-1]["args"]["url"]
        == "https://github.com/stofll/Sotto/releases"
    )


def test_whats_new_internal_failure_is_logged_without_blocking_startup(app, page):
    errors = []
    page.on(
        "console",
        lambda message: (
            errors.append(message.text) if message.type == "error" else None
        ),
    )
    ui = app(responses={"get_whats_new": [{"error": "Synthetic database failure"}]})
    ui.nav("info")
    expect(page.get_by_role("dialog")).not_to_be_visible()
    assert any(
        "Could not load release notes:" in error
        and "Synthetic database failure" in error
        for error in errors
    )


def test_whats_new_renderer_failure_keeps_notes_and_controls(app, page, pytestconfig):
    module_url = (
        "**/assets/ReleaseNotes-*.js"
        if pytestconfig.getoption("--ui-mode") == "production"
        else "**/src/components/ReleaseNotes.tsx*"
    )
    ui = app(responses={"get_whats_new": [{"hold": True}]})
    ui.allow_asset_failure(module_url)
    page.route(module_url, lambda route: route.abort())
    ui.settle("get_whats_new", result=RELEASE)
    dialog = page.get_by_role("dialog")
    expect(dialog).to_contain_text(RELEASE["notes"])
    dialog.get_by_role("button", name="Релиз на GitHub").click()
    assert ui.calls("open_url")[-1]["args"]["url"] == RELEASE["url"]
    dialog.get_by_role("button", name="Закрыть", exact=True).last.click()
    expect(dialog).not_to_be_visible()


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_whats_new_markdown_table(app, page, locale, theme, output_path):
    page.set_viewport_size({"width": 1000, "height": 710})
    ui = app(
        config={"ui_language": locale, "theme": theme},
        whats_new={
            **RELEASE,
            "notes": "## Изменения\n\n"
            "| Компонент | Исправление | Версия |\n"
            "| :--- | :---: | ---: |\n"
            "| **Диктовка** | Быстрее остановка | `0.1.3` |\n"
            "| [Статистика](https://github.com/stofll/Sotto/releases) | Уточнены подсказки | 0.1.3 |\n"
            "| A \\| B | ОченьДлинноеСловоБезПробеловДляПроверкиПереносаВнутриЯчейки | 0.1.3 |",
        },
    )
    dialog = page.get_by_role("dialog")
    table = dialog.get_by_role("table")
    expect(table.get_by_role("columnheader")).to_have_count(3)
    expect(table.get_by_role("cell")).to_have_count(9)
    expect(table.locator("strong")).to_have_text("Диктовка")
    expect(table.locator("code")).to_have_text("0.1.3")
    expect(table.get_by_role("cell", name="A | B", exact=True)).to_be_visible()
    expect(dialog).not_to_contain_text("| :--- |")
    for short_text in [table.locator("th").first, table.locator("code")]:
        assert (
            short_text.evaluate("""element => {
            const range = document.createRange();
            range.selectNodeContents(element);
            return range.getClientRects().length;
        }""")
            == 1
        )
    assert (
        table.locator("th").nth(1).evaluate("e => getComputedStyle(e).textAlign")
        == "center"
    )
    assert (
        table.locator("td").nth(2).evaluate("e => getComputedStyle(e).textAlign")
        == "right"
    )
    assert (
        dialog.locator(".modal__body").evaluate("e => e.scrollWidth - e.clientWidth")
        <= 1
    )
    link = table.get_by_role("link", name="Статистика")
    link.focus()
    expect(link).to_be_focused()
    dialog.screenshot(path=str(Path(output_path) / f"table-{locale}-{theme}.png"))
    page.keyboard.press("Enter")
    assert (
        ui.calls("open_url")[-1]["args"]["url"]
        == "https://github.com/stofll/Sotto/releases"
    )


def test_whats_new_wide_table_stays_inside_dialog(app, page):
    columns = " | ".join(f"Column {i}" for i in range(10))
    app(
        whats_new={
            **RELEASE,
            "notes": f"| {columns} |\n| "
            + " | ".join(["---"] * 10)
            + f" |\n| {columns} |",
        }
    )
    dialog = page.get_by_role("dialog")
    table_region = dialog.get_by_role("region", name="Таблица изменений")
    expect(table_region.get_by_role("columnheader")).to_have_count(10)
    assert table_region.evaluate("e => e.scrollWidth > e.clientWidth")
    assert (
        dialog.locator(".modal__body").evaluate("e => e.scrollWidth - e.clientWidth")
        <= 1
    )
    dialog.get_by_role("button").first.focus()
    page.keyboard.press("Tab")
    expect(table_region).to_be_focused()


def test_whats_new_table_remote_content_stays_inert(app, page):
    app(
        whats_new={
            **RELEASE,
            "notes": "| Content |\n| --- |\n"
            "| [Unsafe](javascript:alert%281%29) |\n"
            '| <img src="https://invalid.example/pixel"> |\n'
            "| ![Image](https://invalid.example/pixel) |",
        }
    )
    table = page.get_by_role("table")
    expect(table.get_by_role("cell")).to_have_count(3)
    expect(table).to_contain_text("Image")
    expect(table.locator("a, img, script, iframe")).to_have_count(0)
