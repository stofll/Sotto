import re

from playwright.sync_api import expect


def test_update_current_error_retry(app, page):
    ui = app()
    ui.nav("info")
    expect(
        page.get_by_text("Установлена последняя версия.", exact=True)
    ).to_be_visible()
    ui.queue("check_update", {"error": "Synthetic update failure"})
    page.get_by_role("button", name="Проверить обновления", exact=True).click()
    expect(page.get_by_text("Synthetic update failure", exact=False)).to_be_visible()
    page.get_by_role("button", name="Проверить обновления", exact=True).click()
    expect(
        page.get_by_text("Установлена последняя версия.", exact=True)
    ).to_be_visible()


def test_update_available_and_install_failure(app, page):
    ui = app()
    ui.nav("info")
    expect(
        page.get_by_text("Установлена последняя версия.", exact=True)
    ).to_be_visible()
    ui.queue(
        "check_update",
        {
            "result": {
                "available": True,
                "current_version": "0.0.5",
                "version": "0.0.6",
                "notes": "Synthetic release notes",
            }
        },
    )
    page.get_by_role("button", name="Проверить обновления", exact=True).click()
    expect(page.get_by_text("Synthetic release notes", exact=True)).to_be_visible()
    ui.queue("install_update", {"hold": True})
    page.get_by_role("button", name="Обновить до 0.0.6", exact=True).click()
    ui.settle("install_update", error="Synthetic signature failure")
    expect(page.get_by_text("Synthetic signature failure", exact=False)).to_be_visible()


def test_clear_logs_confirmation_cancel_and_apply(app, page):
    ui = app()
    ui.nav("info")
    page.get_by_role("button", name="Очистить логи", exact=True).click()
    dialog = page.get_by_role("alertdialog")
    expect(dialog.get_by_role("button", name="Отмена", exact=True)).to_be_focused()
    page.keyboard.press("Escape")
    expect(dialog).not_to_be_visible()
    assert not ui.calls("clear_logs")
    page.get_by_role("button", name="Очистить логи", exact=True).click()
    dialog.get_by_role("button", name="Очистить", exact=True).click()
    expect(page.get_by_text("0 КБ", exact=True)).to_be_visible()


def test_stats_periods_and_refresh(app, page):
    ui = app(stats={"total_transcriptions": 1234, "total_characters": 5678})
    ui.nav("stats")
    page.get_by_role("button", name="Всё время", exact=True).click()
    expect(page.get_by_test_id("page-stats")).to_contain_text(re.compile(r"1\s*234"))
    for period in ["Неделя", "Месяц", "Год"]:
        button = page.get_by_role("button", name=period, exact=True)
        button.click()
        expect(button).to_have_attribute("aria-pressed", "true")
    next_stats = {**ui.state()["stats"], "total_transcriptions": 4321}
    ui.queue("get_stats", {"result": next_stats})
    page.get_by_role("button", name="Обновить", exact=True).click()
    page.get_by_role("button", name="Всё время", exact=True).click()
    expect(page.get_by_test_id("page-stats")).to_contain_text(re.compile(r"4\s*321"))


def test_paste_test_waits_before_pasting_and_reports_success(app, page):
    # The countdown is the whole point of the control: pasting on the click
    # itself would deliver the text into this settings window.
    ui = app()
    ui.nav("info")
    ui.queue(
        "test_paste", {"result": "Paste OK. Text на буфере: Тест вставки Sotto — 1"}
    )
    button = page.get_by_test_id("paste-test")
    button.click()
    expect(button).to_be_disabled()
    assert not ui.calls("test_paste")
    expect(page.get_by_test_id("paste-test-result")).to_contain_text(
        "Paste OK", timeout=10_000
    )
    assert len(ui.calls("test_paste")) == 1
    expect(button).to_be_enabled()


def test_paste_test_shows_the_failure_reason(app, page):
    ui = app()
    ui.nav("info")
    ui.queue("test_paste", {"error": "Paste FAILED: all paste strategies failed"})
    page.get_by_test_id("paste-test").click()
    expect(page.get_by_test_id("paste-test-result")).to_contain_text(
        "all paste strategies failed", timeout=10_000
    )
    expect(page.get_by_test_id("paste-test")).to_be_enabled()
