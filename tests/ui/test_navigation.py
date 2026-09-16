import pytest
from playwright.sync_api import expect

TABS = {
    "settings": "Настройки",
    "models": "Модели распознавания",
    "text": "Текст",
    "ai": "LLM-обработка",
    "integrations": "Провайдеры и ключи",
    "history": "История транскрипций",
    "stats": "Статистика",
    "info": "Справка",
}


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_all_pages_render(app, page, locale, theme):
    ui = app(config={"ui_language": locale, "theme": theme})
    expect(page.locator("html")).to_have_attribute("data-theme", theme)
    for tab, title in TABS.items():
        region = ui.nav(tab)
        expect(region.get_by_role("heading", level=1)).to_be_visible()
        if locale == "ru":
            expect(region.get_by_role("heading", level=1)).to_have_text(title)
        if tab != "ai":
            expect(page.get_by_role("alert")).to_have_count(0)


def test_startup_error_is_visible(app, page):
    ui = app(responses={"get_stats": [{"error": "Synthetic startup failure"}] * 2})
    expect(page.get_by_role("alert")).to_contain_text("Synthetic startup failure")
    ui.nav("history")
    expect(page.get_by_text("История пуста", exact=True)).to_be_visible()


def test_history_module_load_failure_offers_reload(app, page):
    ui = app(config={"ui_accent": "#e68a3d"})
    module_url = "**/src/pages/HistoryPage.tsx*"
    page.route(module_url, lambda route: route.abort())
    ui.nav("history")
    expect(page.get_by_role("alert")).to_contain_text("Не удалось открыть историю.")
    page.unroute(module_url)
    page.get_by_role("button", name="Перезагрузить", exact=True).click()
    expect(page.get_by_test_id("page-settings")).to_be_visible()
    ui.nav("history")
    expect(page.get_by_text("История пуста", exact=True)).to_be_visible()


def test_theme_persists_after_reload(app, page):
    ui = app()
    page.get_by_role("button", name="Включить светлую тему").click()
    ui.saved("theme", "light")
    page.reload()
    expect(page.locator("html")).to_have_attribute("data-theme", "light")


def test_theme_failure_rolls_back(app, page):
    ui = app()
    ui.queue("save_config", {"error": "Cannot save theme"})
    page.get_by_role("button", name="Включить светлую тему").click()
    expect(page.get_by_role("alert")).to_contain_text("Cannot save theme")
    expect(page.locator("html")).to_have_attribute("data-theme", "dark")


@pytest.mark.parametrize(
    "legacy,current",
    [
        ("formatting", "text"),
        ("replacements", "text"),
        ("providers", "integrations"),
        ("api-keys", "integrations"),
        ("overview", "settings"),
    ],
)
def test_legacy_navigation_events(app, page, legacy, current):
    ui = app()
    ui.emit("navigate-tab", legacy)
    expect(page.get_by_test_id(f"page-{current}")).to_be_visible()


def test_permission_banner_deduplicates_and_dismisses(app, page):
    ui = app()
    payload = {
        "kind": "permission",
        "permission": "microphone",
        "hint": "Test microphone",
    }
    ui.emit("app-error", payload)
    ui.emit("app-error", payload)
    banner = page.get_by_role("alert")
    expect(banner).to_have_count(1)
    expect(banner).to_contain_text("Test microphone")
    banner.get_by_role("button", name="Открыть System Settings").click()
    page.wait_for_function(
        "window.__sottoTest.calls.some(x => x.command === 'open_url')"
    )
    assert "Privacy_Microphone" in ui.calls("open_url")[0]["args"]["url"]
    banner.get_by_role("button", name="Закрыть", exact=True).click()
    expect(banner).to_have_count(0)
