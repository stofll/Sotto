import re

from playwright.sync_api import expect


def test_preview_preserves_draft_across_navigation(app, page):
    ui = app()
    ui.nav("text")
    field = page.get_by_placeholder("Введите текст для проверки обработки")
    field.fill("Synthetic preview draft")
    ui.nav("settings")
    ui.nav("text")
    expect(field).to_have_value("Synthetic preview draft")


def test_preview_backend_error(app, page):
    ui = app()
    ui.nav("text")
    ui.queue("preview_format", {"error": "Synthetic formatting failure"})
    page.get_by_placeholder("Введите текст для проверки обработки").fill(
        "Synthetic input"
    )
    expect(page.get_by_test_id("page-text")).to_contain_text(
        "Synthetic formatting failure"
    )


def test_replacement_example_persists(app, page):
    ui = app()
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"щас.*сейчас")).click()
    page.get_by_role("button", name="Сохранить", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.state.config.replacement_rules.some(r=>r.find==='щас' && r.replace==='сейчас')"
    )
    ui.nav("settings")
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"^Замены")).click()
    expect(page.get_by_placeholder("что искать", exact=True)).to_have_value("щас")


def test_dictionary_create_read_search_delete(app, page):
    ui = app()
    ui.nav("text")
    page.get_by_role("button", name="Словари", exact=True).click()
    page.get_by_role("button", name="Создать набор", exact=True).click()
    dialog = page.get_by_role("dialog", name="Редактор набора")
    dialog.get_by_label("Название набора", exact=True).fill("Synthetic vocabulary")
    dialog.get_by_label("Термины", exact=True).fill("Playwright\nSotto")
    dialog.get_by_role("button", name="Закрыть", exact=True).click()
    expect(dialog).not_to_be_visible()
    page.get_by_role(
        "button", name=re.compile(r"^Synthetic vocabulary Пользовательский")
    ).click()
    dialog = page.get_by_role("dialog", name="Synthetic vocabulary")
    expect(dialog.get_by_text("Playwright", exact=True)).to_be_visible()
    dialog.get_by_label("Поиск по набору", exact=True).fill("absent")
    expect(dialog.get_by_text("Ничего не найдено", exact=True)).to_be_visible()
    dialog.get_by_role("button", name="Удалить", exact=True).click()
    dialog.get_by_role("alert").get_by_role(
        "button", name="Удалить", exact=True
    ).click()
    expect(dialog).not_to_be_visible()
    assert ui.state()["config"]["text_formatting"]["dictionary_sets"] == []


def test_dictionary_unsaved_changes_and_focus_restore(app, page):
    ui = app()
    ui.nav("text")
    page.get_by_role("button", name="Словари", exact=True).click()
    create = page.get_by_role("button", name="Создать набор", exact=True)
    create.click()
    dialog = page.get_by_role("dialog")
    dialog.get_by_label("Название набора", exact=True).fill("Unsaved vocabulary")
    page.keyboard.press("Escape")
    expect(dialog).to_contain_text("Закрыть без сохранения изменений?")
    dialog.get_by_role("button", name="Не сохранять", exact=True).click()
    expect(dialog).not_to_be_visible()
    expect(create).to_be_focused()
    assert ui.state()["config"]["text_formatting"]["dictionary_sets"] == []


def test_replacement_save_failure_keeps_retry_available(app, page):
    ui = app()
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"щас.*сейчас")).click()
    ui.queue("save_config", {"error": "Synthetic replacement save failure"})
    page.get_by_role("button", name="Сохранить", exact=True).click()
    expect(page.get_by_role("alert")).to_contain_text(
        "Synthetic replacement save failure"
    )
    expect(page.get_by_role("button", name="Сохранить", exact=True)).to_be_enabled()
    page.get_by_role("button", name="Сохранить", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.state.config.replacement_rules.length === 1"
    )


def test_replacement_import_export_roundtrip(app, page):
    import json

    ui = app()
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"^Замены")).click()
    rule = {
        "id": "synthetic-rule",
        "find": "sotto",
        "replace": "Sotto",
        "enabled": True,
        "match": "word",
        "case_sensitive": False,
        "preserve_case": True,
        "usage_count": 0,
    }
    with page.expect_file_chooser() as chooser:
        page.get_by_role("button", name="Импорт", exact=True).click()
    chooser.value.set_files(
        {
            "name": "rules.json",
            "mimeType": "application/json",
            "buffer": json.dumps({"replacement_rules": [rule]}).encode(),
        }
    )
    expect(page.get_by_placeholder("что искать", exact=True)).to_have_value("sotto")
    page.get_by_role("button", name="Сохранить", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.state.config.replacement_rules.length === 1"
    )
    with page.expect_download() as download:
        page.get_by_role("button", name="Экспорт", exact=True).click()
    exported = json.loads(download.value.path().read_text())
    assert exported["replacement_rules"][0]["find"] == "sotto"
    assert exported["replacement_rules"][0]["replace"] == "Sotto"


def test_invalid_replacement_import_preserves_rules(app, page):
    ui = app()
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"^Замены")).click()
    with page.expect_file_chooser() as chooser:
        page.get_by_role("button", name="Импорт", exact=True).click()
    chooser.value.set_files(
        {"name": "invalid.json", "mimeType": "application/json", "buffer": b"not json"}
    )
    expect(page.get_by_test_id("page-text")).to_contain_text("JSON")
    expect(page.get_by_text("Нет правил замены", exact=True)).to_be_visible()
    assert ui.state()["config"]["replacement_rules"] == []


def test_builtin_parasite_words_are_shown_and_can_be_switched_off(app, page):
    """The built-in list used to live only in the Rust source: a word vanished
    from the dictation and there was no way to see which one, or to stop it."""
    ui = app()
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"^Очистка")).click()
    page.get_by_role("button", name="Список: 3 слова", exact=True).click()
    dialog = page.get_by_role("dialog", name="Слова-паразиты")
    # How a chip works is said once, under the title — not per set.
    expect(dialog.get_by_text("Нажмите на слово")).to_have_count(1)
    korotche = dialog.get_by_role("button", name="короче", exact=True)
    expect(korotche).to_be_visible()
    expect(korotche).to_have_attribute("aria-pressed", "true")

    korotche.click()
    page.wait_for_function(
        "JSON.stringify(window.__sottoTest.state.config.text_formatting"
        ".disabled_parasite_words) === '[\"короче\"]'"
    )
    expect(korotche).to_have_attribute("aria-pressed", "false")
    # The rest of the list keeps working — switching one word off is not a
    # master switch.
    expect(dialog.get_by_role("button", name="типа", exact=True)).to_have_attribute(
        "aria-pressed", "true"
    )

    # The summary on the row is the only trace of the list once the dialog is
    # closed, so it has to report the change.
    dialog.get_by_role("button", name="Закрыть", exact=True).click()
    expect(dialog).not_to_be_visible()
    expect(page.get_by_role("button", name="Список: 3 слова · 1 выключено", exact=True)).to_be_visible()


def test_english_set_is_off_until_switched_on(app, page):
    """The English set ships switched off because «like» and «well» are ordinary
    vocabulary. Off, it shows its switch and nothing else — a list of words that
    cannot be removed from anything is the clutter this replaced."""
    ui = app()
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"^Очистка")).click()
    page.get_by_role("button", name="Список: 3 слова", exact=True).click()
    dialog = page.get_by_role("dialog", name="Слова-паразиты")
    expect(dialog.get_by_role("button", name="basically", exact=True)).not_to_be_visible()

    dialog.get_by_label("Английские", exact=True).click()
    page.wait_for_function(
        "JSON.stringify(window.__sottoTest.state.config.text_formatting"
        ".parasite_sets) === '[\"ru\",\"en\"]'"
    )
    expect(dialog.get_by_role("button", name="basically", exact=True)).to_be_visible()

    # Both sets now count towards the summary on the row.
    dialog.get_by_role("button", name="Закрыть", exact=True).click()
    expect(page.get_by_role("button", name="Список: 5 слов", exact=True)).to_be_visible()


def test_set_for_the_dictation_language_comes_first(app, page):
    """Dictating in English, the English set is the one worth reading, so it is
    the one at the top."""
    ui = app(config={"language": "en"})
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"^Очистка")).click()
    page.get_by_role("button", name=re.compile(r"^Список: 3")).click()
    dialog = page.get_by_role("dialog", name="Слова-паразиты")
    headings = dialog.get_by_role("heading")
    expect(headings.first).to_have_text("Английские")


def test_switching_the_last_set_off_stays_off(app, page):
    """An empty selection is a choice, not «never configured» — it must not
    resolve back to the defaults on the next render."""
    ui = app()
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"^Очистка")).click()
    page.get_by_role("button", name="Список: 3 слова", exact=True).click()
    dialog = page.get_by_role("dialog", name="Слова-паразиты")
    dialog.get_by_label("Русские", exact=True).click()
    page.wait_for_function(
        "JSON.stringify(window.__sottoTest.state.config.text_formatting"
        ".parasite_sets) === '[]'"
    )
    expect(dialog.get_by_role("button", name="короче", exact=True)).not_to_be_visible()
    expect(page.get_by_role("button", name="Список: 0 слов", exact=True)).to_be_visible()


def test_own_parasite_word_is_added_as_a_chip_and_can_be_removed(app, page):
    """Typing a word used to leave it inside a textarea that was parsed on save:
    nothing appeared, and there was nothing to take back out."""
    ui = app()
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"^Очистка")).click()
    page.get_by_role("button", name="Список: 3 слова", exact=True).click()
    dialog = page.get_by_role("dialog", name="Слова-паразиты")

    field = dialog.get_by_label("Своё слово-паразит", exact=True)
    field.fill("скажем так")
    field.press("Enter")
    page.wait_for_function(
        "JSON.stringify(window.__sottoTest.state.config.text_formatting"
        ".custom_parasite_words) === '[\"скажем так\"]'"
    )
    expect(dialog.get_by_text("скажем так", exact=True)).to_be_visible()
    expect(field).to_have_value("")

    # Several at once, and a repeat that must not become a second chip.
    field.fill("вроде, скажем так, как-то")
    field.press("Enter")
    page.wait_for_function(
        "JSON.stringify(window.__sottoTest.state.config.text_formatting"
        ".custom_parasite_words) === '[\"скажем так\",\"вроде\",\"как-то\"]'"
    )

    dialog.get_by_role("button", name="Удалить: вроде", exact=True).click()
    page.wait_for_function(
        "JSON.stringify(window.__sottoTest.state.config.text_formatting"
        ".custom_parasite_words) === '[\"скажем так\",\"как-то\"]'"
    )
    expect(dialog.get_by_text("вроде", exact=True)).not_to_be_visible()
