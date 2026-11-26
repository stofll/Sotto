import json
import re
from pathlib import Path

import pytest
from playwright.sync_api import expect


def vocabulary(words, name="Synthetic vocabulary", enabled=False):
    return {
        "id": "synthetic-vocabulary",
        "name": name,
        "description": "Synthetic test data",
        "enabled": enabled,
        "words": words,
    }


def file_payload(name, content):
    return {
        "name": name,
        "mimeType": "application/json" if name.endswith(".json") else "text/plain",
        "buffer": content.encode("utf-8") if isinstance(content, str) else content,
    }


def test_import_previews_5000_terms_and_saves_disabled_without_losing_commas(app, page):
    ui = app()
    ui.nav("text")
    words = ["Синтетический термин, вариант 1"] + [
        f"Synthetic term {index}" for index in range(1, 5000)
    ]
    # Import is reachable while the dictionary block is collapsed.
    with page.expect_file_chooser() as chooser:
        page.get_by_role("button", name="Импорт", exact=True).click()
    chooser.value.set_files(
        file_payload("Medical.txt", "\ufeff" + "\r\n".join([*words, words[0], ""]))
    )
    dialog = page.get_by_role("dialog")
    expect(dialog).to_have_accessible_name("Редактор набора")
    expect(dialog.get_by_label("Название набора", exact=True)).to_have_value("Medical")
    expect(dialog.get_by_label("Термины", exact=True)).to_have_value("\n".join(words))
    expect(dialog).to_contain_text(
        "Прочитано терминов: 5001. Добавлено: 5000. Пропущено повторов: 1."
    )
    assert ui.state()["config"]["text_formatting"]["dictionary_sets"] == []
    # An imported draft is unsaved even before the user edits its fields.
    page.keyboard.press("Escape")
    expect(dialog).to_contain_text("Закрыть без сохранения изменений?")
    dialog.get_by_role("button", name="Вернуться", exact=True).click()
    dialog.get_by_role("button", name="Сохранить", exact=True).click()
    expect(dialog).not_to_be_visible()
    saved = ui.state()["config"]["text_formatting"]["dictionary_sets"][0]
    assert saved["words"] == words
    assert saved["enabled"] is False


def test_failed_import_and_save_preserve_existing_draft_and_activation(app, page):
    original = vocabulary(["Original term"], enabled=True)
    ui = app(config={"text_formatting": {"dictionary_sets": [original]}})
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"^Словари")).click()
    page.locator(".dictionary-open").first.click()
    dialog = page.get_by_role("dialog")
    dialog.get_by_role("button", name="Редактировать", exact=True).click()
    terms = dialog.get_by_label("Термины", exact=True)
    terms.fill("Original term\nUnsaved, phrase")
    file_input = dialog.get_by_label("Добавить термины из файла", exact=True)
    file_input.set_input_files(file_payload("terms.json", "not JSON"))
    expect(dialog.get_by_role("alert")).to_contain_text("Некорректный JSON")
    expect(terms).to_have_value("Original term\nUnsaved, phrase")
    incoming = {
        "version": 1,
        "name": "Another name",
        "description": "Another description",
        "words": ["Original term", "New term", "New term"],
    }
    file_input.set_input_files(file_payload("terms.json", json.dumps(incoming)))
    expect(dialog.get_by_role("alert")).to_have_count(0)
    expect(terms).to_have_value("Original term\nUnsaved, phrase\nNew term")
    expect(dialog.get_by_label("Название набора", exact=True)).to_have_value(
        original["name"]
    )
    expect(dialog).to_contain_text(
        "Прочитано терминов: 3. Добавлено: 1. Пропущено повторов: 2."
    )
    ui.queue("save_config", {"error": "Synthetic write failure"})
    save = dialog.get_by_role("button", name="Сохранить", exact=True)
    save.click()
    expect(dialog.get_by_role("alert")).to_contain_text("Synthetic write failure")
    assert ui.state()["config"]["text_formatting"]["dictionary_sets"] == [original]
    save.click()
    expect(dialog).not_to_be_visible()
    saved = ui.state()["config"]["text_formatting"]["dictionary_sets"][0]
    assert saved["words"] == ["Original term", "Unsaved, phrase", "New term"]
    assert saved["enabled"] is True


def test_invalid_encoding_is_reported_and_retry_remains_available(app, page):
    ui = app()
    ui.nav("text")
    file_input = page.get_by_label("Файл словаря", exact=True)
    file_input.set_input_files(file_payload("terms.txt", b"\xff\xfe"))
    expect(page.get_by_role("alert")).to_contain_text("UTF-8")
    expect(page.get_by_role("dialog")).to_have_count(0)
    assert ui.state()["config"]["text_formatting"]["dictionary_sets"] == []
    file_input.set_input_files(file_payload("terms.txt", "Valid term"))
    dialog = page.get_by_role("dialog", name="Редактор набора")
    expect(dialog.get_by_label("Термины", exact=True)).to_have_value("Valid term")
    expect(page.get_by_role("alert")).to_have_count(0)


def test_late_file_read_cannot_replace_newer_import(app, page):
    ui = app()
    ui.nav("text")
    page.evaluate("""() => {
      const read = File.prototype.arrayBuffer;
      File.prototype.arrayBuffer = function () {
        if (this.name !== 'held.txt') return read.call(this);
        return new Promise((resolve, reject) => {
          window.finishDictionaryRead = () => read.call(this).then(resolve, reject);
        });
      };
    }""")
    file_input = page.get_by_label("Файл словаря", exact=True)
    file_input.set_input_files(file_payload("held.txt", "Old term"))
    page.wait_for_function("typeof window.finishDictionaryRead === 'function'")
    file_input.set_input_files(file_payload("new.txt", "New term"))
    dialog = page.get_by_role("dialog", name="Редактор набора")
    expect(dialog.get_by_label("Термины", exact=True)).to_have_value("New term")
    page.evaluate("() => window.finishDictionaryRead()")
    expect(dialog.get_by_label("Название набора", exact=True)).to_have_value("new")
    expect(dialog.get_by_label("Термины", exact=True)).to_have_value("New term")
    assert ui.state()["config"]["text_formatting"]["dictionary_sets"] == []


def test_dictionary_change_during_save_validation_keeps_import_as_draft(app, page):
    ui = app()
    ui.nav("text")
    page.get_by_label("Файл словаря", exact=True).set_input_files(
        file_payload("draft.txt", "Imported term")
    )
    dialog = page.get_by_role("dialog", name="Редактор набора")
    save = dialog.get_by_role("button", name="Сохранить", exact=True)
    expect(save).to_be_enabled()
    writes_before = len(ui.calls("save_config"))
    ui.queue("analyze_dictionary", {"hold": True})
    save.click()
    page.wait_for_function("window.__sottoTest.pending('analyze_dictionary')")
    changed = ui.state()["config"]
    changed["text_formatting"]["dictionary_sets"] = [
        vocabulary(["Term from another window"])
    ]
    page.evaluate("config => window.__sottoTest.state.config = config", changed)
    ui.emit("config-updated", changed)
    expect(dialog.get_by_role("alert")).to_contain_text("Словари изменились")
    ui.settle(
        "analyze_dictionary",
        result={"effective_count": 0, "conflicts": [], "unsupported_words": []},
    )
    expect(save).to_be_disabled()
    expect(dialog.get_by_label("Термины", exact=True)).to_have_value("Imported term")
    assert len(ui.calls("save_config")) == writes_before
    assert (
        ui.state()["config"]["text_formatting"]["dictionary_sets"]
        == changed["text_formatting"]["dictionary_sets"]
    )


def test_export_cancel_failure_and_retry_do_not_change_the_set(app, page):
    original = vocabulary(["First, phrase", "Second term"], enabled=True)
    ui = app(config={"text_formatting": {"dictionary_sets": [original]}})
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"^Словари")).click()
    page.locator(".dictionary-open").first.click()
    dialog = page.get_by_role("dialog")
    export = dialog.get_by_role("button", name="Экспорт", exact=True)
    ui.queue(
        "export_dictionary_set",
        {"hold": True},
        {"error": "Synthetic export failure"},
        {"result": True},
    )
    export.click()
    expect(export).to_be_disabled()
    ui.settle("export_dictionary_set", result=False)
    expect(export).to_be_enabled()
    expect(dialog.get_by_text("Набор экспортирован.", exact=True)).to_have_count(0)
    expect(dialog.get_by_role("alert")).to_have_count(0)
    export.click()
    expect(dialog.get_by_role("alert")).to_contain_text("Не удалось экспортировать")
    export.click()
    expect(dialog.get_by_role("alert")).to_have_count(0)
    expect(dialog.get_by_text("Набор экспортирован.", exact=True)).to_be_visible()
    content = json.loads(ui.calls("export_dictionary_set")[-1]["args"]["content"])
    assert content == {
        "version": 1,
        "name": original["name"],
        "description": original["description"],
        "words": original["words"],
    }
    assert ui.state()["config"]["text_formatting"]["dictionary_sets"] == [original]


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["light", "dark"])
def test_large_dictionary_search_paging_and_layout(
    app, page, locale, theme, output_path
):
    ru = locale == "ru"
    page.set_viewport_size({"width": 1000, "height": 710})
    words = [f"Synthetic term {index:04}" for index in range(5000)]
    ui = app(
        config={
            "ui_language": locale,
            "theme": theme,
            "text_formatting": {"dictionary_sets": [vocabulary(words)]},
        }
    )
    ui.nav("text")
    page.get_by_role(
        "button", name=re.compile(r"^Словари" if ru else r"^Dictionaries")
    ).click()
    page.locator(".dictionary-open").first.click()
    dialog = page.get_by_role("dialog")
    terms = dialog.locator(".dictionary-term-list .pill")
    expect(terms).to_have_count(100)
    dialog.get_by_role("button", name="Далее" if ru else "Next", exact=True).click()
    expect(dialog.get_by_text("Synthetic term 0100", exact=True)).to_be_visible()
    expect(terms).to_have_count(100)
    # The second textbox searches terms; the first checks recognition variants.
    search = dialog.get_by_role("textbox").nth(1)
    search.fill("4999")
    expect(terms).to_have_count(1)
    expect(dialog.get_by_text("Synthetic term 4999", exact=True)).to_be_visible()
    search.fill("absent")
    expect(terms).to_have_count(0)
    search.fill("")
    expect(dialog.get_by_text("Synthetic term 0000", exact=True)).to_be_visible()
    expect(terms).to_have_count(100)
    expect(
        dialog.get_by_role("button", name="Экспорт" if ru else "Export", exact=True)
    ).to_be_in_viewport()
    assert dialog.evaluate("e => e.scrollWidth <= e.clientWidth + 1")
    page.screenshot(
        path=str(Path(output_path) / f"dictionary-5000-{locale}-{theme}.png"),
        animations="disabled",
    )
