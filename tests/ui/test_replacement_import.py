import json
import re
from pathlib import Path

import pytest
from playwright.sync_api import expect


def rule(find, replace, **extra):
    return {
        "id": f"saved-{find}",
        "find": find,
        "replace": replace,
        "match": "word",
        "enabled": True,
        "case_sensitive": False,
        "preserve_case": False,
        "usage_count": 0,
        **extra,
    }


def open_replacements(ui, page):
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"^(Замены|Replacements)")).click()


def replacement_saves(ui):
    # Startup may save UI preferences; watch both persisted rule representations.
    return [
        call
        for call in ui.calls("save_config")
        if {"replacement_rules", "replacements"}.intersection(call["args"]["patch"])
    ]


def upload(page, value, *, button="Импорт", raw=False):
    with page.expect_file_chooser() as chooser:
        page.locator(".fold__body").get_by_role(
            "button", name=button, exact=True
        ).click()
    chooser.value.set_files(
        {
            "name": "replacement-rules.json",
            "mimeType": "application/json",
            "buffer": value if raw else json.dumps(value, ensure_ascii=False).encode(),
        }
    )
    return page.get_by_role("dialog")


def test_replacement_import_reviews_conflicts_and_saves_only_after_apply(app, page):
    original = [rule("first", "First"), rule("same", "Old", usage_count=7)]
    ui = app(config={"replacement_rules": original})
    open_replacements(ui, page)
    dialog = upload(
        page,
        {
            "replacement_rules": [
                rule("first", "First"),
                rule("same", "New"),
                rule("third", "Third"),
                rule("third", "Last"),
            ]
        },
    )
    expect(dialog).to_contain_text("повторов: 1; конфликтов: 2; ошибок: 0")
    expect(dialog).to_contain_text("Добавится: 1; заменится: 0")
    dialog.get_by_role("button", name="Заменять совпадения", exact=True).click()
    expect(dialog).to_contain_text("Добавится: 1; заменится: 1")
    expect(dialog).to_contain_text("Совпадение с записью 3 из файла")
    assert ui.state()["config"]["replacement_rules"] == original
    assert replacement_saves(ui) == []

    dialog.get_by_role("button", name="Добавить в список", exact=True).click()
    expect(page.get_by_label("что искать", exact=True)).to_have_count(3)
    assert ui.state()["config"]["replacement_rules"] == original
    assert replacement_saves(ui) == []
    page.get_by_role("button", name="Сохранить", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.state.config.replacement_rules.length === 3"
    )
    saved = ui.state()["config"]["replacement_rules"]
    assert [(item["find"], item["replace"]) for item in saved] == [
        ("first", "First"),
        ("same", "New"),
        ("third", "Last"),
    ]
    assert saved[1]["id"] == original[1]["id"]
    assert saved[1]["usage_count"] == 7
    assert saved[2]["id"] != "saved-third"


def test_import_reports_schema_and_native_regex_errors_without_partial_draft(app, page):
    original = [rule("keep", "Keep")]
    ui = app(config={"replacement_rules": original})
    open_replacements(ui, page)
    ui.queue(
        "validate_replacement_rules",
        {"result": [{"index": 1, "message": "Synthetic Rust regex error"}]},
    )
    dialog = upload(
        page,
        [
            {"find": "good", "replace": "Good"},
            {"find": "bad type", "replace": False},
            {"find": "(?=unsupported)", "replace": "value", "match": "regex"},
        ],
    )
    expect(dialog).to_contain_text("ошибок: 2")
    expect(dialog.get_by_role("button", name="Добавить в список")).to_be_disabled()
    rows = dialog.get_by_role("listitem")
    expect(rows.nth(1)).to_contain_text("Запись 2")
    expect(rows.nth(1)).to_contain_text("Поле replace должно быть строкой")
    expect(rows.nth(2)).to_contain_text("Запись 3")
    expect(rows.nth(2)).to_contain_text("Synthetic Rust regex error")
    assert len(ui.calls("validate_replacement_rules")[0]["args"]["rules"]) == 2
    dialog.get_by_role("button", name="Отмена", exact=True).click()
    expect(page.get_by_label("что искать", exact=True)).to_have_value("keep")
    expect(page.get_by_role("button", name="Сохранить", exact=True)).to_have_count(0)
    assert ui.state()["config"]["replacement_rules"] == original


def test_native_validation_failure_can_retry_with_the_same_ids(app, page):
    ui = app()
    open_replacements(ui, page)
    ui.queue(
        "validate_replacement_rules",
        {"error": "Synthetic native check failure"},
        {"result": []},
    )
    dialog = upload(page, {"old": "new"})
    expect(dialog.get_by_role("alert")).to_contain_text(
        "Synthetic native check failure"
    )
    expect(dialog.get_by_role("button", name="Добавить в список")).to_be_disabled()
    dialog.get_by_role("button", name="Повторить проверку", exact=True).click()
    expect(dialog.get_by_role("button", name="Добавить в список")).to_be_enabled()
    calls = ui.calls("validate_replacement_rules")
    assert len(calls) == 2
    assert calls[0]["args"]["rules"] == calls[1]["args"]["rules"]
    dialog.get_by_role("button", name="Добавить в список").click()
    assert ui.state()["config"]["replacement_rules"] == []
    expect(page.get_by_label("что искать", exact=True)).to_have_value("old")


def test_import_module_failure_keeps_dirty_rules_available_for_export_and_save(
    app, page, pytestconfig
):
    original = [rule("original", "Original")]
    ui = app(config={"replacement_rules": original})
    open_replacements(ui, page)
    page.get_by_label("на что заменить", exact=True).fill("Unsaved replacement")
    production = pytestconfig.getoption("--ui-mode") == "production"
    module_url = (
        "**/assets/ReplacementImportDialog-*.js"
        if production
        else "**/src/pages/ReplacementImportDialog.tsx*"
    )
    ui.allow_asset_failure(module_url)
    page.route(
        module_url,
        lambda route: route.fulfill(
            status=503,
            body="Synthetic import module unavailable",
            headers={"Cache-Control": "no-store"},
        ),
    )
    dialog = upload(page, [{"find": "imported", "replace": "Imported"}])
    expect(dialog.get_by_role("alert")).to_contain_text(
        "Не удалось открыть импорт правил"
    )
    expect(page.get_by_label("на что заменить", exact=True)).to_have_value(
        "Unsaved replacement"
    )
    assert ui.state()["config"]["replacement_rules"] == original
    assert replacement_saves(ui) == []

    dialog.get_by_role("button", name="Отмена", exact=True).click()
    expect(dialog).to_have_count(0)
    page.unroute(module_url)
    expect(page.get_by_role("button", name="Сохранить", exact=True)).to_be_enabled()
    expect(page.get_by_label("что искать", exact=True)).to_have_value("original")
    expect(page.get_by_label("на что заменить", exact=True)).to_have_value(
        "Unsaved replacement"
    )
    ui.queue("export_replacement_rules", {"result": None})
    page.get_by_role("button", name="Экспорт и импорт правил", exact=True).click()
    page.get_by_role("menuitem", name="Экспорт", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.calls.some(call => call.command === 'export_replacement_rules')"
    )
    exported = json.loads(ui.calls("export_replacement_rules")[-1]["args"]["content"])
    assert exported["replacement_rules"] == [
        {**original[0], "replace": "Unsaved replacement"}
    ]
    assert replacement_saves(ui) == []
    page.get_by_role("button", name="Сохранить", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.state.config.replacement_rules[0].replace === 'Unsaved replacement'"
    )
    assert ui.state()["config"]["replacement_rules"] == exported["replacement_rules"]


def test_cancelling_pending_validation_ignores_its_late_result(app, page):
    ui = app()
    open_replacements(ui, page)
    ui.queue("validate_replacement_rules", {"hold": True})
    dialog = upload(page, [{"find": "pending", "replace": "Pending"}])
    page.wait_for_function(
        "window.__sottoTest.calls.some(call => call.command === 'validate_replacement_rules')"
    )
    expect(dialog.get_by_role("button", name="Добавить в список")).to_be_disabled()
    dialog.get_by_role("button", name="Отмена", exact=True).click()
    ui.settle("validate_replacement_rules", result=[])
    expect(page.get_by_role("dialog")).to_have_count(0)
    expect(page.get_by_text("Нет правил замены", exact=True)).to_be_visible()
    assert ui.state()["config"]["replacement_rules"] == []
    assert replacement_saves(ui) == []


def test_import_uses_current_rules_and_preserves_its_draft_on_config_refresh(app, page):
    ui = app(config={"replacement_rules": [rule("original", "Original")]})
    open_replacements(ui, page)
    ui.queue("validate_replacement_rules", {"hold": True})
    dialog = upload(page, [{"find": "imported", "replace": "Imported"}])
    page.wait_for_function("window.__sottoTest.pending('validate_replacement_rules')")

    updated = ui.state()["config"]
    updated["replacement_rules"].append(rule("external", "External"))
    page.evaluate("config => window.__sottoTest.state.config = config", updated)
    ui.emit("config-updated", updated)
    expect(page.get_by_label("что искать", exact=True)).to_have_count(2)
    ui.settle("validate_replacement_rules", result=[])
    expect(dialog.get_by_role("button", name="Добавить в список")).to_be_enabled()
    dialog.get_by_role("button", name="Добавить в список").click()
    expect(page.get_by_label("что искать", exact=True)).to_have_count(3)
    expect(page.get_by_label("что искать", exact=True).last).to_have_value("imported")

    # Another saved-config notification must not replace an unsaved draft.
    updated["replacement_rules"][0]["replace"] = "Externally updated"
    page.evaluate("config => window.__sottoTest.state.config = config", updated)
    ui.emit("config-updated", updated)
    expect(page.get_by_label("что искать", exact=True)).to_have_count(3)
    expect(page.get_by_label("что искать", exact=True).last).to_have_value("imported")
    expect(page.get_by_role("alert")).to_contain_text(
        "Правила замен изменились в другом окне"
    )
    save = page.get_by_role("button", name="Сохранить", exact=True)
    expect(save).to_be_disabled()
    # Even an attempted activation cannot write the stale array over newer rules.
    save.evaluate("button => button.click()")
    assert ui.state()["config"]["replacement_rules"] == updated["replacement_rules"]
    assert replacement_saves(ui) == []

    # Export remains available to preserve this unsaved draft before reopening.
    ui.queue("export_replacement_rules", {"result": None})
    page.get_by_role("button", name="Экспорт и импорт правил", exact=True).click()
    page.get_by_role("menuitem", name="Экспорт", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.calls.some(call => call.command === 'export_replacement_rules')"
    )
    exported = json.loads(ui.calls("export_replacement_rules")[-1]["args"]["content"])
    assert [
        (item["find"], item["replace"]) for item in exported["replacement_rules"]
    ] == [
        ("original", "Original"),
        ("external", "External"),
        ("imported", "Imported"),
    ]
    ui.nav("settings")
    ui.nav("text")
    expect(page.get_by_label("что искать", exact=True)).to_have_count(2)
    expect(page.get_by_label("на что заменить", exact=True).first).to_have_value(
        "Externally updated"
    )
    expect(page.get_by_role("button", name="Сохранить", exact=True)).to_have_count(0)
    expect(page.get_by_role("alert")).to_have_count(0)
    assert replacement_saves(ui) == []


@pytest.mark.parametrize(
    "content,error",
    [
        (b" " * (2 * 1024 * 1024 + 1), "не должен превышать 2 МиБ"),
        (b'{"word":"\xff"}', "в кодировке UTF-8"),
    ],
    ids=["oversize", "invalid-utf8"],
)
def test_replacement_import_rejects_oversize_and_non_utf8_files(
    app, page, content, error
):
    ui = app()
    open_replacements(ui, page)
    dialog = upload(page, content, raw=True)
    expect(dialog.get_by_role("alert")).to_contain_text(error)
    expect(dialog.get_by_role("button", name="Добавить в список")).to_be_disabled()
    assert ui.calls("validate_replacement_rules") == []
    assert replacement_saves(ui) == []


def test_two_thousand_rules_are_reviewed_and_edited_in_bounded_pages(app, page):
    ui = app()
    open_replacements(ui, page)
    contents = [
        {"find": f"term-{index}", "replace": f"Термин-{index}"} for index in range(2000)
    ]
    dialog = upload(page, contents)
    expect(dialog).to_contain_text("Корректных: 2000")
    expect(dialog.get_by_role("listitem")).to_have_count(50)
    pager = dialog.get_by_role("navigation", name="Страницы импорта")
    expect(pager).to_contain_text("1–50 из 2000")
    pager.get_by_role("button", name="Следующая страница").click()
    expect(dialog.get_by_role("listitem").first).to_contain_text("Запись 51")
    dialog.get_by_role("button", name="Добавить в список").click()
    expect(page.get_by_label("что искать", exact=True)).to_have_count(50)
    page.get_by_placeholder("Найти правило", exact=True).fill("term-1999")
    expect(page.get_by_label("что искать", exact=True)).to_have_count(1)
    expect(page.get_by_label("что искать", exact=True)).to_have_value("term-1999")
    page.get_by_label("на что заменить", exact=True).fill("Отредактировано")
    page.get_by_role("button", name="Сохранить", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.state.config.replacement_rules.length === 2000"
    )
    saved = ui.state()["config"]["replacement_rules"]
    assert saved[-1]["replace"] == "Отредактировано"
    assert len({item["id"] for item in saved}) == 2000


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["light", "dark"])
def test_import_review_layout_and_keyboard_cancel(
    app, page, output_path, locale, theme
):
    page.set_viewport_size({"width": 900, "height": 650})
    ui = app(
        config={
            "ui_language": locale,
            "theme": theme,
            "replacement_rules": [rule("old", "Old")],
        }
    )
    open_replacements(ui, page)
    dialog = upload(
        page,
        [
            {"find": "old", "replace": "New"},
            {"find": "длинный медицинский термин", "replace": "Корректное написание"},
        ],
        button="Импорт" if locale == "ru" else "Import",
    )
    apply = dialog.locator(".btn--primary")
    expect(apply).to_be_enabled()
    expect(dialog).to_be_in_viewport()
    assert dialog.evaluate("element => element.scrollWidth <= element.clientWidth + 1")
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(
        path=str(Path(output_path) / f"replacement-import-{locale}-{theme}.png"),
        animations="disabled",
    )
    page.keyboard.press("Tab")
    assert dialog.evaluate("element => element.contains(document.activeElement)")
    page.keyboard.press("Escape")
    expect(dialog).to_have_count(0)
    assert len(ui.state()["config"]["replacement_rules"]) == 1
