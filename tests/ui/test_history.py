import pytest
from playwright.sync_api import expect

ENTRIES = [
    {
        "id": 1,
        "timestamp": 1789200000,
        "text": "Synthetic first transcript",
        "raw_text": "raw first speech",
        "length": 26,
    },
    {
        "id": 2,
        "timestamp": 1789200060,
        "text": "Синтетическая вторая запись",
        "raw_text": "исходный текст",
        "length": 27,
    },
]


def test_empty_history(app, page):
    ui = app()
    ui.nav("history")
    expect(page.get_by_text("История пуста", exact=True)).to_be_visible()
    expect(page.get_by_role("button", name="Очистить всё", exact=True)).to_be_disabled()


@pytest.mark.parametrize("query,visible", [("first", 1), ("исходный", 2)])
def test_history_search_including_raw(app, page, query, visible):
    ui = app(history=ENTRIES)
    ui.nav("history")
    page.get_by_role("searchbox", name="Поиск по истории").fill(query)
    expect(page.get_by_test_id(f"history-entry-{visible}")).to_be_visible()
    expect(page.get_by_test_id(f"history-entry-{3 - visible}")).not_to_be_visible()
    page.get_by_role("button", name="Очистить поиск", exact=True).click()
    expect(page.get_by_test_id(f"history-entry-{3 - visible}")).to_be_visible()


def test_history_no_results_and_recovery(app, page):
    ui = app(history=ENTRIES)
    ui.nav("history")
    page.get_by_role("searchbox", name="Поиск по истории").fill("absent text")
    expect(page.get_by_text("Ничего не найдено", exact=True)).to_be_visible()
    page.get_by_role("button", name="Очистить поиск", exact=True).click()
    expect(page.get_by_test_id("history-entry-1")).to_be_visible()


def test_history_delete_single(app, page):
    ui = app(history=ENTRIES)
    ui.nav("history")
    page.get_by_test_id("history-entry-1").get_by_role(
        "button", name="Действия", exact=True
    ).click()
    page.get_by_role("menuitem", name="Удалить", exact=True).click()
    page.get_by_role("alertdialog").get_by_role(
        "button", name="Удалить", exact=True
    ).click()
    expect(page.get_by_test_id("history-entry-1")).not_to_be_visible()
    expect(page.get_by_test_id("history-entry-2")).to_be_visible()
    page.reload()
    ui.nav("history")
    expect(page.get_by_test_id("history-entry-1")).not_to_be_visible()


def test_history_delete_error_preserves_entry(app, page):
    ui = app(history=ENTRIES)
    ui.nav("history")
    ui.queue("delete_history_entry", {"error": "Cannot delete entry"})
    page.get_by_test_id("history-entry-1").get_by_role(
        "button", name="Действия", exact=True
    ).click()
    page.get_by_role("menuitem", name="Удалить", exact=True).click()
    page.get_by_role("alertdialog").get_by_role(
        "button", name="Удалить", exact=True
    ).click()
    expect(page.get_by_role("alert")).to_contain_text("Cannot delete entry")
    expect(page.get_by_test_id("history-entry-1")).to_be_visible()


def test_history_view_mode_and_selection(app, page):
    ui = app(history=ENTRIES)
    ui.nav("history")
    page.get_by_role("button", name="Список", exact=True).click()
    page.get_by_test_id("history-entry-1").get_by_role("checkbox").check()
    expect(
        page.get_by_role("region", name="Действия с выбранными записями")
    ).to_be_visible()
    page.get_by_test_id("history-entry-1").get_by_role("checkbox").uncheck()
    expect(
        page.get_by_role("region", name="Действия с выбранными записями")
    ).not_to_be_visible()


def test_history_load_error(app, page):
    ui = app()
    ui.queue(
        "list_history",
        {"error": "History unavailable"},
        {"error": "History unavailable"},
    )
    ui.nav("history")
    expect(page.get_by_role("alert")).to_contain_text("History unavailable")


def test_clear_history_cancel_then_confirm(app, page):
    ui = app(history=ENTRIES)
    ui.nav("history")
    page.get_by_role("button", name="Очистить всё", exact=True).click()
    dialog = page.get_by_role("alertdialog")
    expect(dialog.get_by_role("button", name="Отмена", exact=True)).to_be_focused()
    page.keyboard.press("Escape")
    expect(page.get_by_test_id("history-entry-1")).to_be_visible()
    assert not ui.calls("clear_history")
    page.get_by_role("button", name="Очистить всё", exact=True).click()
    dialog.get_by_role("button", name="Очистить", exact=True).click()
    expect(page.get_by_text("История пуста", exact=True)).to_be_visible()


def test_history_details_display_raw_text(app, page):
    ui = app(history=ENTRIES)
    ui.nav("history")
    card = page.get_by_test_id("history-entry-1")
    card.get_by_role("button", name="Подробнее", exact=True).click()
    card.get_by_role(
        "button", name="Развернуть блок Распознавание без обработки", exact=True
    ).click()
    expect(card).to_contain_text("raw first speech")


def test_history_reprocess_preview_before_apply(app, page):
    ui = app(history=ENTRIES)
    ui.nav("history")
    card = page.get_by_test_id("history-entry-1")
    card.get_by_role("button", name="Обработать через LLM", exact=True).click()
    ui.queue(
        "preview_history_ai_processing",
        {
            "result": {
                "ok": True,
                "text": "Synthetic revised transcript",
                "provider": "openai",
                "model": "test-model",
                "profile_name": "Test",
                "elapsed_seconds": 0.1,
                "ai_json": "{}",
                "stats_json": "{}",
            }
        },
    )
    card.get_by_role("button", name="Запустить", exact=True).click()
    expect(
        card.get_by_role("button", name="Заменить текст", exact=True)
    ).to_be_visible()
    assert not ui.calls("apply_history_ai_processing")
    updated = {**ENTRIES[0], "text": "Synthetic revised transcript"}
    ui.queue(
        "apply_history_ai_processing", {"result": {"updated": True, "entry": updated}}
    )
    card.get_by_role("button", name="Заменить текст", exact=True).click()
    expect(card).to_contain_text("Synthetic revised transcript")
    expect(
        card.get_by_role("button", name="Заменить текст", exact=True)
    ).not_to_be_visible()


def test_bulk_delete_partial_failure_keeps_only_failed_rows(app, page):
    ui = app(history=ENTRIES)
    ui.nav("history")
    page.get_by_test_id("history-entry-1").get_by_role("checkbox").check()
    page.get_by_test_id("history-entry-2").get_by_role("checkbox").check()
    ui.queue("delete_history_entry", {"error": "Synthetic partial failure"})
    page.get_by_role("region", name="Действия с выбранными записями").get_by_role(
        "button", name="Удалить", exact=True
    ).click()
    page.get_by_role("alertdialog").get_by_role(
        "button", name="Удалить", exact=True
    ).click()
    expect(page.get_by_role("alert")).to_contain_text("Synthetic partial failure")
    expect(page.get_by_test_id("history-entry-1")).to_be_visible()
    expect(page.get_by_test_id("history-entry-2")).not_to_be_visible()
    assert [entry["id"] for entry in ui.state()["history"]] == [1]


def test_history_reprocess_failure_can_retry(app, page):
    ui = app(history=ENTRIES)
    ui.nav("history")
    card = page.get_by_test_id("history-entry-1")
    card.get_by_role("button", name="Обработать через LLM", exact=True).click()
    ui.queue("preview_history_ai_processing", {"error": "Synthetic preview timeout"})
    card.get_by_role("button", name="Запустить", exact=True).click()
    expect(card.get_by_role("alert")).to_contain_text("Synthetic preview timeout")
    expect(card.get_by_role("button", name="Запустить", exact=True)).to_be_enabled()
    expect(card).to_contain_text(ENTRIES[0]["text"])
    assert not ui.calls("apply_history_ai_processing")
