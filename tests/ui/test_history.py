from pathlib import Path

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


def test_history_refreshes_when_shown_instead_of_polling(app, page):
    page.clock.install()
    ui = app()
    ui.nav("history")
    expect(page.get_by_text("История пуста", exact=True)).to_be_visible()
    loaded = len(ui.calls("list_history"))
    page.clock.run_for(120_000)
    assert len(ui.calls("list_history")) == loaded
    page.evaluate("""() => {
        Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
        document.dispatchEvent(new Event('visibilitychange'));
    }""")
    page.wait_for_function(
        "n => window.__sottoTest.calls.filter(x => x.command === 'list_history').length > n",
        arg=loaded,
    )


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


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_history_view_mode_and_selection(app, page, locale, theme, output_path):
    page.set_viewport_size({"width": 1000, "height": 710})
    ui = app(history=ENTRIES, config={"ui_language": locale, "theme": theme})
    ui.nav("history")
    entry = page.get_by_test_id("history-entry-1")
    count = "· 26 симв." if locale == "ru" else "· 26 chars"
    expect(entry.get_by_text(count, exact=True)).to_be_visible()
    header = entry.get_by_text(count, exact=True).locator("..")
    assert header.inner_text().index(count) < header.inner_text().index("STT:")
    entry.get_by_text(ENTRIES[0]["text"], exact=True).hover()
    expect(page.locator(".hint-bubble")).not_to_be_visible()
    Path(output_path).mkdir(parents=True, exist_ok=True)
    entry.screenshot(path=str(Path(output_path) / "history-metadata.png"))
    page.get_by_role(
        "button", name="Список" if locale == "ru" else "List", exact=True
    ).click()
    expect(entry.get_by_text(count, exact=True)).to_be_visible()
    page.get_by_test_id("history-entry-1").get_by_role("checkbox").check()
    expect(
        page.get_by_role(
            "region",
            name="Действия с выбранными записями"
            if locale == "ru"
            else "Actions on the selected entries",
        )
    ).to_be_visible()
    page.get_by_test_id("history-entry-1").get_by_role("checkbox").uncheck()
    expect(
        page.get_by_role(
            "region",
            name="Действия с выбранными записями"
            if locale == "ru"
            else "Actions on the selected entries",
        )
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


def test_history_plays_a_saved_recording(app, page, browser_name):
    if browser_name == "webkit":
        # Playwright's WebKit build has no audio playback: play() rejects any
        # WAV with NotSupportedError. The failed-load path is covered below.
        pytest.skip("Playwright WebKit cannot play audio")
    recorded = {**ENTRIES[0], "has_recording": True}
    ui = app(history=[recorded, ENTRIES[1]])
    ui.nav("history")
    expect(page.get_by_test_id("history-play-2")).to_have_count(0)
    play = page.get_by_test_id("history-play-1")
    play.click()
    slider = page.get_by_test_id("history-entry-1").get_by_role(
        "slider", name="Позиция воспроизведения"
    )
    expect(slider).to_be_visible()
    expect(page.get_by_test_id("history-entry-1")).to_contain_text("/ 0:02")
    assert ui.calls("history_recording") == [
        {"command": "history_recording", "args": {"id": 1}}
    ]
    expect(play).to_have_attribute("aria-pressed", "true")
    play.click()
    expect(play).to_have_attribute("aria-pressed", "false")


def test_history_reports_a_recording_that_cannot_be_read(app, page):
    ui = app(history=[{**ENTRIES[0], "has_recording": True}])
    ui.nav("history")
    ui.queue("history_recording", {"error": "read recording: not found"})
    page.get_by_test_id("history-play-1").click()
    expect(page.get_by_role("alert")).to_contain_text("read recording: not found")
    expect(page.get_by_role("slider", name="Позиция воспроизведения")).to_have_count(0)


def test_history_pages_by_fifty_and_search_returns_to_first_page(app, page):
    entries = [
        {
            "id": index,
            "timestamp": 1789200000 + index * 60,
            "text": f"Synthetic entry {index}",
            "length": 18,
        }
        for index in range(120, 0, -1)
    ]
    ui = app(history=entries)
    ui.nav("history")
    pager = page.get_by_role("navigation", name="Страницы истории")
    expect(page.get_by_test_id("history-entry-120")).to_be_visible()
    expect(page.get_by_test_id("history-entry-71")).to_be_visible()
    expect(page.get_by_test_id("history-entry-70")).to_have_count(0)
    expect(pager.get_by_role("button", name="Предыдущая страница")).to_be_disabled()

    pager.get_by_role("button", name="Страница 3", exact=True).click()
    expect(
        pager.get_by_role("button", name="Страница 3", exact=True)
    ).to_have_attribute("aria-current", "page")
    expect(page.get_by_test_id("history-entry-20")).to_be_visible()
    expect(page.get_by_test_id("history-entry-21")).to_have_count(0)
    expect(pager.get_by_role("button", name="Следующая страница")).to_be_disabled()

    page.get_by_role("searchbox", name="Поиск по истории").fill("entry 1")
    expect(page.get_by_test_id("history-entry-119")).to_be_visible()
    expect(page.get_by_test_id("history-entry-1")).to_be_visible()
    expect(pager).to_have_count(0)


def test_long_reprocess_diff_marks_only_edits_and_apply_keeps_entry_in_view(app, page):
    words = [f"слово{i}" for i in range(400)]
    long_text = " ".join(words)
    for index in (17, 233, 391):
        words[index] += ","
    revised = " ".join(words)
    entries = [
        {"id": 3, "timestamp": 1789200120, "text": long_text, "length": len(long_text)},
        # Enough entries below that the page stays long once the panel closes.
        *[
            {
                "id": 100 + index,
                "timestamp": 1789200000 - index * 60,
                "text": f"Synthetic older entry {index}",
                "length": 24,
            }
            for index in range(30)
        ],
    ]
    ui = app(history=entries)
    page.set_viewport_size({"width": 1100, "height": 700})
    ui.nav("history")
    card = page.get_by_test_id("history-entry-3")
    card.get_by_role("button", name="Обработать через LLM", exact=True).click()
    ui.queue(
        "preview_history_ai_processing",
        {
            "result": {
                "ok": True,
                "text": revised,
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
    apply = card.get_by_role("button", name="Заменить текст", exact=True)
    expect(apply).to_be_visible()
    marked = card.locator("span[style*='line-through'], span[style*='--ok']")
    assert marked.all_inner_texts() == [",", ",", ","]

    ui.queue(
        "apply_history_ai_processing",
        {"result": {"updated": True, "entry": {**entries[0], "text": revised}}},
    )
    apply.scroll_into_view_if_needed()
    apply.click()
    expect(apply).not_to_be_visible()
    expect(card).to_be_in_viewport()


def test_open_history_entry_shows_what_a_late_answer_changed(app, page):
    late_entry = {
        "id": 5,
        "timestamp": 1789200120,
        "text": "Привет, как дела?",
        "formatted_text": "привет как дела",
        "raw_text": "привет как дела",
        "length": 17,
        "ai_processing": {
            "attempted": True,
            "used": True,
            "late": True,
            "enabled": True,
            "provider": "openai",
            "model": "synthetic-model",
        },
    }
    ui = app(history=[*ENTRIES, late_entry])
    ui.nav("settings")
    ui.emit("open-history-entry", 5)
    card = page.get_by_test_id("history-entry-5")
    expect(card.get_by_text("Diff: до LLM → финальный")).to_be_visible()
    expect(
        card.get_by_label("LLM: обработано позже · openai / synthetic-model")
    ).to_be_attached()
    # The request is spent: coming back to the page does not reopen the diff.
    card.get_by_role("button", name="Скрыть diff", exact=True).click()
    ui.nav("settings")
    ui.nav("history")
    expect(card.get_by_text("Diff: до LLM → финальный")).to_have_count(0)


def test_history_names_a_meaning_change_fallback(app, page):
    entry = {
        "id": 6,
        "timestamp": 1789200180,
        "text": "I'd prefer to never merge this",
        "formatted_text": "I'd prefer to never merge this",
        "raw_text": "I'd prefer to never merge this",
        "length": 30,
        "ai_processing": {
            "attempted": True,
            "used": False,
            "fallback": True,
            "enabled": True,
            "provider": "openai",
            "model": "synthetic-model",
            "error_type": "altered_response",
            "skipped_reason": "model_dropped_negation",
        },
    }
    app(history=[entry]).nav("history")
    card = page.get_by_test_id("history-entry-6")
    expect(
        card.get_by_label("LLM: модель убрала отрицание · openai / synthetic-model")
    ).to_be_attached()
