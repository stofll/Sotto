"""History pages cross IPC in bounded batches; filters still cover all rows."""

import pytest
from playwright.sync_api import expect


def history_entries(count=120):
    return [
        {
            "id": index,
            "timestamp": 1789200000 + index * 60,
            "text": f"Synthetic entry {index}",
            "raw_text": f"raw speech {index}",
            "length": 18,
        }
        for index in range(count, 0, -1)
    ]


@pytest.mark.parametrize(
    "field,stored,query",
    [
        ("text", "ЯКОРЬ_% точный текст", "якорь_%"),
        ("raw_text", "ЯКОРЬ исходной речи", "якорь исходной"),
        ("formatted_text", "ЯКОРЬ форматирования", "якорь форматирования"),
        ("system_prompt", "ЯКОРЬ системного промпта", "якорь системного"),
        ("transcription_model", "ЯКОРЬ-model", "якорь-model"),
    ],
)
def test_search_reaches_unloaded_pages(app, page, field, stored, query):
    entries = history_entries()
    entries[-7][field] = stored
    ui = app(history=entries)
    page.evaluate("""() => {
      const invoke = window.__TAURI_INTERNALS__.invoke;
      window.historyReplySizes = [];
      window.__TAURI_INTERNALS__.invoke = async (command, args) => {
        const result = await invoke(command, args);
        if (command === 'list_history') window.historyReplySizes.push(result.entries.length);
        return result;
      };
    }""")
    ui.nav("history")
    expect(page.get_by_test_id("history-entry-120")).to_be_visible()
    expect(page.get_by_test_id("history-entry-7")).to_have_count(0)
    page.get_by_role("searchbox", name="Поиск по истории").fill(query)
    expect(page.get_by_test_id("history-entry-7")).to_be_visible()
    expect(page.get_by_test_id("history-entry-120")).to_have_count(0)
    expect(page.get_by_text("Показано 1 из 120", exact=False)).to_be_visible()
    assert all(size <= 50 for size in page.evaluate("window.historyReplySizes"))
    assert all(
        call["args"]["query"]["page_size"] == 50 for call in ui.calls("list_history")
    )


def test_status_and_date_filters_cover_unloaded_rows(app, page):
    now = page.evaluate("Date.now() / 1000")
    entries = history_entries()
    for index, entry in enumerate(entries):
        entry["timestamp"] = now - index * 3600
    entries[-7]["ai_processing"] = {"attempted": True, "used": True}
    entries[-8]["ai_processing"] = {"attempted": True, "fallback": True}
    ui = app(history=entries)
    ui.nav("history")
    page.get_by_role("button", name="Обработано", exact=True).click()
    expect(page.get_by_test_id("history-entry-7")).to_be_visible()
    expect(page.get_by_test_id("history-entry-8")).to_have_count(0)
    page.get_by_role("button", name="Сегодня", exact=True).click()
    expect(page.get_by_text("Ничего не найдено", exact=True)).to_be_visible()
    page.get_by_role("button", name="Неделя", exact=True).click()
    expect(page.get_by_test_id("history-entry-7")).to_be_visible()
    page.get_by_role("button", name="Fallback", exact=True).click()
    expect(page.get_by_test_id("history-entry-8")).to_be_visible()
    expect(page.get_by_test_id("history-entry-7")).to_have_count(0)


def test_late_search_reply_cannot_replace_current_results(app, page):
    ui = app(history=history_entries())
    ui.nav("history")
    expect(page.get_by_test_id("history-entry-120")).to_be_visible()
    page.evaluate("""() => {
      const invoke = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = async (command, args) => {
        const result = await invoke(command, args);
        if (command === 'list_history' && args.query.query === 'entry 1') {
          return new Promise(resolve => { window.finishOldSearch = () => resolve(result); });
        }
        return result;
      };
    }""")
    search = page.get_by_role("searchbox", name="Поиск по истории")
    search.fill("entry 1")
    page.wait_for_function("typeof window.finishOldSearch === 'function'")
    search.fill("entry 7")
    expect(page.get_by_test_id("history-entry-7")).to_be_visible()
    expect(page.get_by_test_id("history-entry-119")).to_have_count(0)
    page.evaluate("""async () => {
      window.finishOldSearch();
      await new Promise(resolve => setTimeout(resolve, 0));
    }""")
    expect(page.get_by_test_id("history-entry-7")).to_be_visible()
    expect(page.get_by_test_id("history-entry-119")).to_have_count(0)
    expect(page.get_by_text("Показано 11 из 120", exact=False)).to_be_visible()


def test_history_initial_error_has_retry_without_false_empty_state(app, page):
    ui = app(history=history_entries())
    page.evaluate("""() => {
      const invoke = window.__TAURI_INTERNALS__.invoke;
      window.failHistory = true;
      window.__TAURI_INTERNALS__.invoke = (command, args) =>
        command === 'list_history' && window.failHistory
          ? Promise.reject(new Error('Synthetic history load failure')) : invoke(command, args);
    }""")
    ui.nav("history")
    expect(page.get_by_role("alert")).to_contain_text("Synthetic history load failure")
    expect(page.get_by_text("История пуста", exact=True)).to_have_count(0)
    page.evaluate("window.failHistory = false")
    page.get_by_role("button", name="Повторить попытку", exact=True).click()
    expect(page.get_by_test_id("history-entry-120")).to_be_visible()
    expect(page.get_by_role("alert")).to_have_count(0)


def test_page_error_keeps_previous_rows_and_retries_requested_page(app, page):
    ui = app(history=history_entries())
    ui.nav("history")
    pager = page.get_by_role("navigation", name="Страницы истории")
    ui.queue("list_history", {"error": "Synthetic page unavailable"})
    pager.get_by_role("button", name="Страница 2", exact=True).click()
    expect(page.get_by_role("alert")).to_contain_text("Synthetic page unavailable")
    expect(page.get_by_test_id("history-entry-120")).to_be_visible()
    expect(
        pager.get_by_role("button", name="Страница 1", exact=True)
    ).to_have_attribute("aria-current", "page")
    page.get_by_role("button", name="Повторить попытку", exact=True).click()
    expect(page.get_by_test_id("history-entry-70")).to_be_visible()
    expect(page.get_by_test_id("history-entry-120")).to_have_count(0)
    expect(
        pager.get_by_role("button", name="Страница 2", exact=True)
    ).to_have_attribute("aria-current", "page")


def test_deleting_last_page_clamps_to_remaining_page(app, page):
    ui = app(history=history_entries(101))
    ui.nav("history")
    pager = page.get_by_role("navigation", name="Страницы истории")
    pager.get_by_role("button", name="Страница 3", exact=True).click()
    card = page.get_by_test_id("history-entry-1")
    card.get_by_role("button", name="Действия", exact=True).click()
    page.get_by_role("menuitem", name="Удалить", exact=True).click()
    page.get_by_role("alertdialog").get_by_role(
        "button", name="Удалить", exact=True
    ).click()
    expect(page.get_by_test_id("history-entry-1")).to_have_count(0)
    expect(page.get_by_test_id("history-entry-51")).to_be_visible()
    expect(
        pager.get_by_role("button", name="Страница 2", exact=True)
    ).to_have_attribute("aria-current", "page")
    expect(pager.get_by_role("button", name="Страница 3", exact=True)).to_have_count(0)


def test_bulk_copy_fetches_actual_retained_rows_across_pages(app, page):
    ui = app(history=history_entries())
    ui.nav("history")
    page.get_by_test_id("history-entry-120").get_by_role("checkbox").check()
    page.get_by_role("button", name="Страница 3", exact=True).click()
    page.get_by_test_id("history-entry-1").get_by_role("checkbox").check()
    bulk = page.get_by_role("region", name="Действия с выбранными записями")
    expect(bulk).to_contain_text("Выбрано: 2")
    page.evaluate("""() => {
      Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
        writeText: async text => { window.copiedHistory = text; },
      }});
      const state = window.__sottoTest.state;
      state.history.find(entry => entry.id === 120).text = 'Updated off-page transcript';
      state.history = state.history.filter(entry => entry.id !== 1);
    }""")
    bulk.get_by_role("button", name="Скопировать", exact=True).click()
    expect(page.get_by_role("status")).to_contain_text("Скопировано записей: 1")
    assert page.evaluate("window.copiedHistory") == "Updated off-page transcript"
    expect(bulk).to_contain_text("Выбрано: 1")
    copy_requests = [
        call for call in ui.calls("list_history") if call["args"]["query"].get("ids")
    ]
    assert len(copy_requests) == 1
    assert copy_requests[0]["args"]["query"] == {
        "ids": [120, 1],
        "page": 0,
        "page_size": 100,
    }


def test_off_page_selection_survives_filter_and_selects_visible_rows(app, page):
    ui = app(history=history_entries())
    ui.nav("history")
    page.get_by_test_id("history-entry-120").get_by_role("checkbox").check()
    bulk = page.get_by_role("region", name="Действия с выбранными записями")
    bulk.get_by_role("button", name="Выделить видимые (50)", exact=True).click()
    page.get_by_role("button", name="Страница 3", exact=True).click()
    expect(bulk).to_contain_text("Выбрано: 50")
    bulk.get_by_role("button", name="Выделить видимые (20)", exact=True).click()
    expect(bulk).to_contain_text("Выбрано: 70")
    page.get_by_role("searchbox", name="Поиск по истории").fill("entry 1")
    expect(page.get_by_text("Показано 32 из 120", exact=False)).to_be_visible()
    expect(page.get_by_test_id("history-entry-1")).to_be_visible()
    expect(page.get_by_test_id("history-entry-80")).to_have_count(0)
    expect(bulk).to_contain_text("Выбрано: 70")


def test_large_bulk_copy_is_complete_and_ordered_with_bounded_replies(app, page):
    ui = app(history=history_entries(101))
    ui.nav("history")
    page.get_by_test_id("history-entry-101").get_by_role("checkbox").check()
    bulk = page.get_by_role("region", name="Действия с выбранными записями")
    bulk.get_by_role("button", name="Выделить видимые (50)", exact=True).click()
    page.get_by_role("button", name="Страница 2", exact=True).click()
    bulk.get_by_role("button", name="Выделить видимые (50)", exact=True).click()
    page.get_by_role("button", name="Страница 3", exact=True).click()
    bulk.get_by_role("button", name="Выделить видимые (1)", exact=True).click()
    expect(bulk).to_contain_text("Выбрано: 101")
    page.evaluate("""() => {
      Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
        writeText: async text => { window.copiedHistory = text; },
      }});
      const invoke = window.__TAURI_INTERNALS__.invoke;
      window.copyReplySizes = [];
      window.__TAURI_INTERNALS__.invoke = async (command, args) => {
        const result = await invoke(command, args);
        if (command === 'list_history' && args.query.ids) window.copyReplySizes.push(result.entries.length);
        return result;
      };
    }""")
    bulk.get_by_role("button", name="Скопировать", exact=True).click()
    expect(page.get_by_role("status")).to_contain_text("Скопировано записей: 101")
    assert page.evaluate("window.copiedHistory") == "\n\n---\n\n".join(
        f"Synthetic entry {index}" for index in range(101, 0, -1)
    )
    assert page.evaluate("window.copyReplySizes") == [100, 1]


def test_focus_jump_finds_older_page_and_reopens_existing_row(app, page):
    ui = app(history=history_entries())
    ui.nav("history")
    page.get_by_role("searchbox", name="Поиск по истории").fill("absent entry")
    expect(page.get_by_text("Ничего не найдено", exact=True)).to_be_visible()
    ui.emit("open-history-entry", 7)
    card = page.get_by_test_id("history-entry-7")
    hide = card.get_by_role("button", name="Скрыть детали", exact=True)
    expect(hide).to_be_visible()
    expect(page.get_by_role("searchbox", name="Поиск по истории")).to_have_value("")
    expect(page.get_by_role("button", name="Страница 3", exact=True)).to_have_attribute(
        "aria-current", "page"
    )
    hide.click()
    ui.emit("open-history-entry", 7)
    expect(hide).to_be_visible()
    expect(page.get_by_role("button", name="Страница 3", exact=True)).to_have_attribute(
        "aria-current", "page"
    )
