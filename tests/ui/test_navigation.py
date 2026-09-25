import re
from pathlib import Path

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


def test_startup_waits_for_stylesheet_before_deriving_accent(app, page, pytestconfig):
    if pytestconfig.getoption("--ui-mode") != "production":
        pytest.skip(
            "Production links CSS separately; Vite dev injects it with JavaScript"
        )

    # Keep CSS pending while browser callbacks and module scripts continue.
    # The load event must recompute contrast after the stylesheet arrives.
    def delayed_stylesheet(route):
        # Explicit import reproduces WebKit's early module execution even in
        # browsers that normally block the initial module on CSS readiness.
        page.wait_for_function("document.getElementById('root') !== null")
        page.evaluate(
            "async () => { await import(document.querySelector('script[type=module]').src); }"
        )
        page.wait_for_function(
            "document.documentElement.style.getPropertyValue('--accent') === '#102040'"
        )
        assert (
            page.evaluate(
                "getComputedStyle(document.documentElement).getPropertyValue('--bg-0')"
            )
            == ""
        )
        route.continue_()

    page.route("**/assets/styles-*.css", delayed_stylesheet)
    app(config={"ui_accent": "#102040", "theme": "light"})
    expect(page.locator("html")).to_have_css("--accent", "#102040")
    assert page.locator("html").evaluate(
        "el => el.style.getPropertyValue('--accent-text').trim().length > 0"
    )


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


def test_accessibility_notice_can_be_dismissed(app, page):
    ui = app(responses={"check_accessibility": [{"result": False}] * 8})
    notice = page.get_by_test_id("accessibility-notice")
    expect(notice).to_be_visible()
    expect(notice).to_contain_text("Для автоматической вставки нужен доступ macOS")
    notice.get_by_role("button", name="Закрыть", exact=True).click()
    expect(notice).to_have_count(0)
    assert ui.calls("check_accessibility")


def test_startup_error_is_visible(app, page):
    ui = app(responses={"get_stats": [{"error": "Synthetic startup failure"}] * 2})
    expect(page.get_by_role("alert")).to_contain_text("Synthetic startup failure")
    ui.nav("history")
    expect(page.get_by_text("История пуста", exact=True)).to_be_visible()


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_history_module_load_failure_offers_recovery(
    app, page, pytestconfig, locale, theme, output_path
):
    page.set_viewport_size({"width": 1000, "height": 710})
    ui = app(config={"ui_accent": "#e68a3d", "ui_language": locale, "theme": theme})
    failure = (
        "Не удалось открыть историю." if locale == "ru" else "Could not open history."
    )
    restart = "полностью закройте Sotto" if locale == "ru" else "quit Sotto completely"
    production = pytestconfig.getoption("--ui-mode") == "production"
    module_url = (
        "**/assets/HistoryPage-*.js" if production else "**/src/pages/HistoryPage.tsx*"
    )
    ui.allow_asset_failure(module_url)
    page.route(
        module_url,
        lambda route: route.fulfill(
            status=503,
            body="Synthetic module unavailable",
            headers={"Cache-Control": "no-store"},
        ),
    )
    ui.nav("history")
    expect(page.get_by_role("alert")).to_contain_text(failure)
    expect(page.get_by_role("alert")).to_contain_text(restart)
    reload_button = page.get_by_role(
        "button", name="Перезагрузить" if locale == "ru" else "Reload", exact=True
    )
    reload_button.focus()
    expect(reload_button).to_be_focused()
    shots = Path(output_path)
    shots.mkdir(parents=True, exist_ok=True)
    page.screenshot(path=str(shots / "history-error.png"), animations="disabled")
    page.unroute(module_url)
    page.get_by_role(
        "button", name="Перезагрузить" if locale == "ru" else "Reload", exact=True
    ).click()
    expect(page.get_by_test_id("page-settings")).to_be_visible()
    ui.nav("history")
    expect(
        page.get_by_text(
            "История пуста" if locale == "ru" else "History is empty", exact=True
        )
    ).to_be_visible()


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_theme_persists_after_reload(app, page, locale):
    ui = app(config={"ui_language": locale})
    for theme in ["light", "dark"]:
        page.locator(".theme-toggle").click()
        ui.saved("theme", theme)
        expect(page.get_by_test_id("page-settings")).to_be_visible()
        page.reload()
        expect(page.locator("html")).to_have_attribute("data-theme", theme)
        expect(page.get_by_test_id("page-settings")).to_be_visible()
        page.locator(".theme-toggle").focus()
        expect(page.locator(".theme-toggle")).to_be_focused()


@pytest.mark.parametrize("window", ["main", "overlay"])
def test_built_entry_loads_under_application_csp(app, page, pytestconfig, window):
    if pytestconfig.getoption("--ui-mode") != "production":
        pytest.skip("Release assets and CSP are production-only")
    responses = []
    page.on(
        "response",
        lambda response: (
            responses.append(response)
            if response.request.resource_type == "document"
            else None
        ),
    )
    ui = app(window)
    if window == "overlay":
        ui.emit("recording-started", 1)
        expect(page.get_by_test_id("overlay")).to_be_visible()
    if window != "main":
        for selector in ["html", "body", "#root"]:
            expect(page.locator(selector)).to_have_css(
                "background-color", "rgba(0, 0, 0, 0)"
            )
    assert (
        responses
        and "script-src 'self'" in responses[0].headers["content-security-policy"]
    )
    expect(page.locator('script[type="module"][src]').first).to_have_attribute(
        "src", re.compile(r"/assets/.*\.js$")
    )
    assert not page.locator('script[src*="/src/"], script[src*="@vite/client"]').count()


def test_theme_failure_rolls_back(app, page):
    ui = app()
    ui.queue("save_config", {"error": "Cannot save theme"})
    page.get_by_role("button", name="Включить светлую тему").click()
    expect(page.get_by_role("alert")).to_contain_text("Cannot save theme")
    expect(page.locator("html")).to_have_attribute("data-theme", "dark")


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
