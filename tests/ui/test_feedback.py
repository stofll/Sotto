from pathlib import Path
from urllib.parse import parse_qs, urlparse

import pytest
from playwright.sync_api import expect


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_feedback_preview_export_and_opt_out(app, page, locale, theme, output_path):
    ui = app(config={"ui_language": locale, "theme": theme})
    ui.nav("info")
    ru = locale == "ru"
    trigger = page.get_by_role(
        "button", name="Сообщить о проблеме" if ru else "Report a problem", exact=True
    )
    trigger.click()
    dialog = page.get_by_role("dialog")
    expect(dialog).to_contain_text("Model: tiny")
    page.keyboard.press("Tab")
    assert dialog.evaluate("e => e.contains(document.activeElement)")
    dialog.get_by_role(
        "button", name="Подготовить очищенные логи" if ru else "Prepare sanitized logs"
    ).click()
    expect(dialog).to_contain_text("[message omitted]")
    dialog.get_by_role(
        "button", name="Сохранить очищенные логи" if ru else "Save sanitized logs"
    ).click()
    expect(dialog.get_by_role("status")).to_contain_text(
        "Файл сохранён" if ru else "File saved"
    )
    assert ui.calls("save_public_logs")[-1]["args"]["content"].startswith(
        "Public diagnostic log"
    )
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(path=str(Path(output_path) / "feedback.png"))
    dialog.get_by_role("checkbox").uncheck()
    dialog.get_by_role(
        "button", name="Продолжить на GitHub" if ru else "Continue on GitHub"
    ).click()
    expect(dialog.get_by_role("status")).to_be_empty()
    query = parse_qs(urlparse(ui.calls("open_url")[-1]["args"]["url"]).query)
    assert query["template"] == ["bug_report.md"]
    assert "body" not in query
    page.keyboard.press("Escape")
    expect(dialog).not_to_be_visible()
    expect(trigger).to_be_focused()


def test_feedback_failure_retry_and_feature_template(app, page):
    ui = app()
    ui.nav("info")
    page.get_by_role("button", name="Предложить улучшение", exact=True).click()
    assert "feature_request.md" in ui.calls("open_url")[-1]["args"]["url"]
    ui.queue("get_public_diagnostics", {"error": "synthetic"})
    page.get_by_role("button", name="Сообщить о проблеме", exact=True).click()
    dialog = page.get_by_role("dialog")
    expect(dialog).to_contain_text("Можно продолжить без неё")
    ui.queue("get_public_logs", {"error": "synthetic"})
    prepare = dialog.get_by_role("button", name="Подготовить очищенные логи")
    prepare.click()
    expect(dialog.get_by_role("status")).to_contain_text("Не удалось выполнить")
    prepare.click()
    expect(dialog).to_contain_text("[message omitted]")
    ui.queue("save_public_logs", {"result": False})
    dialog.get_by_role("button", name="Сохранить очищенные логи").click()
    expect(dialog.get_by_role("status")).to_be_empty()
    ui.queue("open_url", {"error": "synthetic"})
    proceed = dialog.get_by_role("button", name="Продолжить на GitHub")
    proceed.click()
    expect(dialog.get_by_role("status")).to_contain_text("Не удалось открыть браузер")
    proceed.click()
    expect(dialog.get_by_role("status")).to_be_empty()
