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


def test_sidebar_follows_width_boundary_until_manually_overridden(app, page):
    page.set_viewport_size({"width": 1099, "height": 900})
    app()
    window = page.locator(".win")
    expect(window).to_have_class("win collapsed")
    page.set_viewport_size({"width": 1100, "height": 900})
    expect(window).to_have_class("win")
    page.set_viewport_size({"width": 1099, "height": 900})
    expect(window).to_have_class("win collapsed")

    page.get_by_role("button", name="Развернуть сайдбар", exact=True).click()
    for width in [1200, 1000]:
        page.set_viewport_size({"width": width, "height": 900})
        expect(window).to_have_class("win")
    page.reload()
    expect(page.get_by_test_id("page-settings")).to_be_visible()
    expect(window).to_have_class("win")

    page.get_by_role("button", name="Свернуть сайдбар", exact=True).click()
    page.set_viewport_size({"width": 1200, "height": 900})
    expect(window).to_have_class("win collapsed")
    page.reload()
    expect(page.get_by_test_id("page-settings")).to_be_visible()
    expect(window).to_have_class("win collapsed")


def test_accessibility_notice_can_be_dismissed(app, page):
    ui = app(responses={"check_accessibility": [{"result": False}] * 8})
    notice = page.locator(".accessibility-notice")
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


@pytest.mark.parametrize(
    "command,tab",
    [
        ("get_stats", "stats"),
        ("list_models", "models"),
        ("has_api_key", "integrations"),
        ("list_microphones", None),
    ],
)
def test_slow_optional_startup_read_keeps_settings_usable(
    app, page, pytestconfig, command, tab
):
    # StrictMode repeats the initial effect; key checks begin only after the
    # surviving configuration read. Hold its first key, not every key slot.
    count = (
        2
        if pytestconfig.getoption("--ui-mode") == "dev" and command != "has_api_key"
        else 1
    )
    ui = app(responses={command: [{"hold": True}] * count})
    page.wait_for_function("c => window.__sottoTest.pending(c)", arg=command)
    expect(page.get_by_test_id("startup-loading")).not_to_be_visible()
    page.get_by_role("button", name="Включить светлую тему").click()
    ui.saved("theme", "light")
    expect(page.locator("html")).to_have_attribute("data-theme", "light")

    if tab:
        ui.nav(tab)
        expect(page.get_by_test_id("page-data-loading")).to_have_text("Загрузка…")
    else:
        expect(page.locator(".custom-select--mic")).to_have_count(0)

    state = ui.state()
    results = {
        "get_stats": state["stats"],
        "list_models": state["models"],
        "has_api_key": {"available": False},
        "list_microphones": [{"id": "test-mic", "name": "Synthetic microphone"}],
    }
    ui.settle(command, result=results[command])
    if tab:
        expect(page.get_by_test_id("page-data-loading")).not_to_be_visible()
        expect(
            page.get_by_test_id(f"page-{tab}").get_by_role("heading", level=1)
        ).to_be_visible()
    else:
        expect(page.locator(".custom-select--mic")).to_be_visible()
    expect(page.get_by_role("alert")).to_have_count(0)


@pytest.mark.parametrize("tab", ["ai", "integrations"])
def test_unreadable_key_slot_does_not_block_key_pages(app, page, tab):
    ui = app(responses={"has_api_key": [{"error": "Synthetic keychain failure"}]})
    ui.nav(tab)
    expect(
        page.get_by_test_id(f"page-{tab}").get_by_role("heading", level=1)
    ).to_be_visible()
    expect(page.get_by_test_id("page-data-loading")).to_have_count(0)
    expect(page.get_by_text("Synthetic keychain failure")).to_have_count(0)


@pytest.mark.parametrize(
    "command,tab",
    [
        ("get_stats", "stats"),
        ("list_models", "models"),
    ],
)
def test_failed_startup_resource_retries_without_reloading_settings(
    app, page, pytestconfig, command, tab
):
    count = 2 if pytestconfig.getoption("--ui-mode") == "dev" else 1
    ui = app(responses={command: [{"error": "Synthetic unavailable data"}] * count})
    expect(page.get_by_role("alert")).to_contain_text("Synthetic unavailable data")
    ui.nav(tab)
    page.get_by_role("button", name="Повторить попытку", exact=True).click()
    expect(page.get_by_test_id("page-data-loading")).not_to_be_visible()
    expect(
        page.get_by_test_id(f"page-{tab}").get_by_role("heading", level=1)
    ).to_be_visible()
    expect(page.get_by_role("alert")).to_have_count(0)


def test_new_stats_refresh_wins_over_slow_startup_read(app, page, pytestconfig):
    count = 2 if pytestconfig.getoption("--ui-mode") == "dev" else 1
    ui = app(responses={"get_stats": [{"hold": True}] * count})
    old = ui.state()["stats"]
    current = {**old, "total_transcriptions": 42}
    ui.queue("get_stats", {"result": current})
    ui.emit("history-updated")
    ui.nav("stats")
    page.get_by_role("button", name="Всё время", exact=True).click()
    expect(page.get_by_text("42", exact=True)).to_be_visible()
    ui.settle("get_stats", result=old)
    expect(page.get_by_text("42", exact=True)).to_be_visible()


def test_background_model_refresh_preserves_catalog_state_and_latest_result(app, page):
    ui = app()
    ui.nav("models")
    search = page.get_by_role("searchbox", name="Поиск по названию", exact=True)
    search.fill("Tiny")
    old = ui.state()["models"]
    ui.queue("list_models", {"hold": True})
    ui.emit("model-ready")
    page.wait_for_function("window.__sottoTest.pending('list_models')")
    expect(page.get_by_test_id("page-data-loading")).to_have_count(0)
    expect(search).to_have_value("Tiny")

    ui.queue("list_models", {"error": "Synthetic refresh failure"})
    ui.emit("model-ready")
    expect(page.get_by_role("alert")).to_contain_text("Synthetic refresh failure")
    expect(search).to_have_value("Tiny")
    expect(page.get_by_test_id("page-data-loading")).to_have_count(0)

    current = [
        {**model, "label": "Updated Whisper Tiny"} if model["id"] == "tiny" else model
        for model in old
    ]
    ui.queue("list_models", {"result": current})
    ui.emit("model-ready")
    expect(page.get_by_test_id("model-tiny")).to_contain_text("Updated Whisper Tiny")
    expect(page.get_by_role("alert")).to_have_count(0)
    ui.settle("list_models", result=old)
    expect(page.get_by_test_id("model-tiny")).to_contain_text("Updated Whisper Tiny")
    expect(search).to_have_value("Tiny")


def test_fresh_runtime_reply_releases_startup_while_old_read_is_pending(
    app, page, pytestconfig
):
    # A native model event can refresh runtime while its first read is pending.
    page.add_init_script("""(() => {
      const timer = setInterval(() => {
        const app = window.__sottoTest;
        if (!app?.pending('get_runtime_status') || !app.subscriptions['model-restored']) return;
        clearInterval(timer);
        app.emit('model-restored');
      }, 10);
    })();""")
    count = 2 if pytestconfig.getoption("--ui-mode") == "dev" else 1
    ui = app(responses={"get_runtime_status": [{"hold": True}] * count})
    expect(page.get_by_role("heading", name="Настройки", exact=True)).to_be_visible()
    assert page.evaluate("window.__sottoTest.pending('get_runtime_status')")
    ui.settle(
        "get_runtime_status",
        result={**ui.state()["runtime"], "loaded_model": "obsolete-runtime"},
    )
    expect(page.get_by_role("complementary")).not_to_contain_text("obsolete-runtime")
    expect(page.get_by_role("complementary")).to_contain_text("tiny")


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
    banner = page.locator(".permission-notice")
    expect(banner).to_have_count(1)
    expect(banner.get_by_role("alert")).to_contain_text("Test microphone")
    banner.get_by_role("button", name="Открыть настройки macOS").click()
    page.wait_for_function(
        "window.__sottoTest.calls.some(x => x.command === 'open_url')"
    )
    assert "Privacy_Microphone" in ui.calls("open_url")[0]["args"]["url"]
    banner.get_by_role("button", name="Закрыть", exact=True).click()
    expect(banner).to_have_count(0)
