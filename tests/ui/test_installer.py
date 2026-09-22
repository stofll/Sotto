"""Setup UI only: synthetic IPC, isolated browsers, no native installer execution."""

import json
import os
import subprocess
from pathlib import Path

import pytest
from conftest import ROOT, _free_port, _serving, _stop
from playwright.sync_api import expect


@pytest.fixture(scope="session")
def setup_server(tmp_path_factory, pytestconfig):
    work = tmp_path_factory.mktemp("sotto-setup-ui")
    dist = work / "dist"
    production = pytestconfig.getoption("--ui-mode") == "production"
    args = ["--config", "vite.setup.config.ts"]
    env = {**os.environ}
    env.pop("TAURI_DEBUG", None)
    if production:
        subprocess.run(
            [
                "node",
                "node_modules/vite/bin/vite.js",
                "build",
                *args,
                "--outDir",
                str(dist),
            ],
            cwd=ROOT / "desktop",
            env=env,
            check=True,
        )
        args = ["preview", *args, "--outDir", str(dist)]
    port = _free_port()
    url = f"http://127.0.0.1:{port}"
    with (work / "server.log").open("w") as log:
        process = subprocess.Popen(
            [
                "node",
                "node_modules/vite/bin/vite.js",
                *args,
                "--host",
                "127.0.0.1",
                "--port",
                str(port),
                "--strictPort",
            ],
            cwd=ROOT / "desktop",
            stdout=log,
            stderr=subprocess.STDOUT,
            env=env,
        )
        try:
            assert _serving(process, url), "Setup preview server did not start"
            yield url
        finally:
            _stop(process)


MOCK = r"""
(() => {
  let id = 0, finish = null;
  const callbacks = new Map();
  const listeners = new Map();
  const state = { phase: "ready", revision: 0, version: "1.2.3", preview: false, error: null };
  const calls = [];
  function update(phase, error = null) {
    Object.assign(state, { phase, error, revision: state.revision + 1 });
    for (const [id, listener] of listeners) listener({ event: "setup-status", id, payload: { ...state } });
  }
  window.__setupTest = {
    calls,
    installOptions: null,
    defaults: { options: { install_dir: "C:\\Users\\Demo\\AppData\\Local\\Sotto", desktop_shortcut: true, start_menu_shortcut: true }, directory_locked: false },
    selectedDirectory: null,
    finish(error = null) { update(error ? "failed" : "complete", error); finish?.(); },
    update,
    failLaunch: false,
  };
  window.__TAURI_INTERNALS__ = {
    transformCallback(fn) { callbacks.set(++id, fn); return id; },
    unregisterCallback(id) { callbacks.delete(id); },
    async invoke(command, args) {
      if (command === "plugin:event|listen") { listeners.set(args.handler, callbacks.get(args.handler)); return args.handler; }
      if (command === "plugin:event|unlisten") { listeners.delete(args.eventId); return; }
      if (command === "setup_status") return { ...state };
      if (command === "setup_options") return structuredClone(window.__setupTest.defaults);
      if (command === "setup_choose_directory") return window.__setupTest.selectedDirectory;
      calls.push(command);
      if (command === "setup_install") {
        window.__setupTest.installOptions = structuredClone(args.options);
        update("installing");
        return new Promise(resolve => { finish = resolve; });
      }
      if (command === "setup_launch") {
        if (window.__setupTest.failLaunch) throw "launch_failed";
        return;
      }
      if (command === "setup_close") return;
      throw new Error("Unexpected IPC: " + command);
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
})();
"""


@pytest.fixture
def setup_app(page, setup_server, pytestconfig):
    errors = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    if pytestconfig.getoption("--ui-mode") == "production":
        csp = json.loads(
            (ROOT / "desktop/setup/src-tauri/tauri.conf.json").read_text()
        )["app"]["security"]["csp"]

        def headers(route):
            response = route.fetch()
            route.fulfill(
                response=response,
                headers={**response.headers, "Content-Security-Policy": csp},
            )

        page.route(setup_server + "/", headers)
    page.add_init_script(MOCK)
    page.set_viewport_size({"width": 760, "height": 580})

    def open_setup():
        page.goto(setup_server)
        expect(
            page.get_by_role("button", name="Установить", exact=True)
        ).to_be_enabled()
        return page

    yield open_setup
    assert not errors, errors


@pytest.mark.parametrize("theme", ["dark", "light"])
@pytest.mark.parametrize("locale", ["ru", "en"])
def test_setup_layout_and_keyboard(setup_app, page, theme, locale, output_path):
    page.emulate_media(color_scheme=theme, reduced_motion="reduce")
    setup_app()
    if locale == "en":
        page.get_by_role("button", name="Сменить язык").click()
    page.set_viewport_size({"width": 540, "height": 520})
    button = page.get_by_role(
        "button", name="Install" if locale == "en" else "Установить", exact=True
    )
    button.focus()
    expect(button).to_be_focused()
    assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
    assert (
        page.locator(".setup-wave").first.evaluate(
            "e => getComputedStyle(e).animationName"
        )
        == "none"
    )
    expect(page.locator(".setup-content img")).to_have_count(0)
    expect(page.get_by_text("1.2.3", exact=True)).to_be_visible()
    about = page.get_by_role(
        "button", name="About this installation" if locale == "en" else "Об установке"
    )
    before = button.bounding_box()
    about.click()
    panel = page.get_by_role("region", name=about.inner_text())
    expect(panel).to_be_visible()
    assert button.bounding_box() == before
    assert page.evaluate("document.documentElement.scrollHeight <= innerHeight")
    panel.click()
    expect(panel).to_be_visible()
    page.screenshot(
        path=str(Path(output_path) / "setup-details.png"), animations="disabled"
    )
    page.keyboard.press("Escape")
    expect(panel).to_have_count(0)
    expect(about).to_be_focused()
    about.click()
    page.get_by_role("heading", name="Sotto", exact=True).click()
    expect(panel).to_have_count(0)
    about.focus()
    page.keyboard.press("Enter")
    expect(panel).to_be_visible()
    page.keyboard.press("Tab")
    expect(panel).to_have_count(0)
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(path=str(Path(output_path) / "setup.png"), animations="disabled")


def test_setup_waves_move_and_theme_icon_switches(setup_app, page):
    page.emulate_media(color_scheme="dark", reduced_motion="no-preference")
    setup_app()
    theme = page.get_by_role("button", name="Светлая тема")
    expect(theme).to_have_text("")
    theme.click()
    expect(page.locator("html")).to_have_attribute("data-theme", "light")
    page.get_by_role("button", name="Тёмная тема").click()
    expect(page.locator("html")).to_have_attribute("data-theme", "dark")
    wave = page.locator(".setup-wave").first
    initial = wave.evaluate("e => getComputedStyle(e).transform")
    page.wait_for_function(
        "initial => getComputedStyle(document.querySelector('.setup-wave')).transform !== initial",
        arg=initial,
    )


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_options_survive_back_and_reach_installer(
    setup_app, page, locale, theme, output_path
):
    page.emulate_media(color_scheme=theme, reduced_motion="reduce")
    setup_app()
    if locale == "en":
        page.get_by_role("button", name="Сменить язык").click()
    page.set_viewport_size({"width": 540, "height": 520})
    options = page.get_by_role(
        "button",
        name="Installation options" if locale == "en" else "Параметры установки",
    )
    options.click()
    heading = page.get_by_role(
        "heading",
        name="Installation options" if locale == "en" else "Параметры установки",
    )
    expect(heading).to_be_focused()
    folder = page.get_by_label(
        "Application folder" if locale == "en" else "Папка приложения"
    )
    expect(folder).to_have_value(r"C:\Users\Demo\AppData\Local\Sotto")
    folder.fill(r"D:\Мои программы\Sotto")
    desktop = page.get_by_label(
        "Desktop shortcut" if locale == "en" else "Ярлык на рабочем столе"
    )
    desktop.uncheck()
    page.get_by_role(
        "button", name="Back" if locale == "en" else "Назад", exact=True
    ).click()
    expect(options).to_be_focused()
    options.click()
    expect(folder).to_have_value(r"D:\Мои программы\Sotto")
    expect(desktop).not_to_be_checked()
    assert page.evaluate("document.documentElement.scrollHeight <= innerHeight")
    page.screenshot(
        path=str(Path(output_path) / "setup-options.png"), animations="disabled"
    )
    page.get_by_role(
        "button", name="Install" if locale == "en" else "Установить", exact=True
    ).click()
    expect(
        page.get_by_role(
            "heading",
            name="Installing Sotto" if locale == "en" else "Устанавливаем Sotto",
        )
    ).to_be_visible()
    assert page.evaluate("window.__setupTest.installOptions") == {
        "install_dir": r"D:\Мои программы\Sotto",
        "desktop_shortcut": False,
        "start_menu_shortcut": True,
    }
    page.evaluate("window.__setupTest.finish()")


def test_folder_picker_cancel_validation_and_retry(setup_app, page):
    setup_app()
    page.get_by_role("button", name="Параметры установки").click()
    folder = page.get_by_label("Папка приложения")
    original = folder.input_value()
    page.get_by_role("button", name="Обзор…").click()
    expect(folder).to_have_value(original)
    page.evaluate(r"window.__setupTest.selectedDirectory = 'D:\\Apps\\Sotto'")
    page.get_by_role("button", name="Обзор…").click()
    expect(folder).to_have_value(r"D:\Apps\Sotto")
    folder.fill("relative/path")
    page.get_by_role("button", name="Установить", exact=True).click()
    expect(page.get_by_role("alert")).to_contain_text("полный путь")
    assert page.evaluate("window.__setupTest.installOptions") is None
    folder.fill(r"D:\Apps\Sotto")
    page.get_by_role("button", name="Установить", exact=True).click()
    page.evaluate("window.__setupTest.finish('install_directory_not_empty')")
    expect(page.get_by_role("alert")).to_contain_text("пустую папку")
    expect(folder).to_have_value(r"D:\Apps\Sotto")


def test_update_keeps_registered_directory(setup_app, page):
    page.add_init_script("window.__setupTest.defaults.directory_locked = true")
    setup_app()
    page.get_by_role("button", name="Параметры установки").click()
    expect(page.get_by_label("Папка приложения")).to_have_attribute("readonly", "")
    expect(page.get_by_role("button", name="Обзор…")).to_have_count(0)
    expect(
        page.get_by_text("При обновлении используется папка установленной Sotto.")
    ).to_be_visible()


def test_install_failure_retry_and_launch_error(setup_app, page):
    setup_app()
    page.get_by_role("button", name="Установить", exact=True).click()
    expect(page.get_by_role("heading", name="Устанавливаем Sotto")).to_be_visible()
    expect(page.get_by_role("button", name="Отмена", exact=True)).to_have_count(0)
    expect(page.get_by_role("progressbar")).not_to_have_attribute(
        "aria-valuenow", "100"
    )
    page.evaluate("window.__setupTest.finish('install_failed')")
    expect(
        page.get_by_role("heading", name="Не удалось установить Sotto")
    ).to_be_focused()
    page.get_by_role("button", name="Повторить попытку").click()
    page.evaluate("window.__setupTest.finish()")
    expect(page.get_by_role("heading", name="Sotto установлено")).to_be_focused()
    page.evaluate("window.__setupTest.failLaunch = true")
    page.get_by_role("button", name="Запустить Sotto").click()
    expect(page.get_by_role("alert")).to_contain_text("меню «Пуск»")
    assert (
        page.evaluate(
            "window.__setupTest.calls.filter(c => c === 'setup_install').length"
        )
        == 2
    )


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_newer_installed_version_is_explained(setup_app, page, locale):
    setup_app()
    if locale == "en":
        page.get_by_role("button", name="Сменить язык").click()
    page.get_by_role(
        "button", name="Install" if locale == "en" else "Установить", exact=True
    ).click()
    page.evaluate("window.__setupTest.finish('installed_version_newer')")
    expect(
        page.get_by_role(
            "heading",
            name="Could not install Sotto"
            if locale == "en"
            else "Не удалось установить Sotto",
        )
    ).to_be_visible()
    expect(page.locator(".setup-main .setup-description")).to_have_text(
        "A newer version of Sotto is already installed. Download the latest installer."
        if locale == "en"
        else "Уже установлена более новая версия Sotto. Скачайте актуальный установщик."
    )


def test_browser_preview_cannot_install(page, setup_server):
    page.goto(setup_server)
    expect(page.get_by_text("Демонстрация · установка не выполняется")).to_be_visible()
    page.get_by_role("button", name="Установить", exact=True).click()
    expect(page.get_by_role("heading", name="Sotto установлено")).to_be_visible(
        timeout=7000
    )
    page.get_by_role("button", name="Запустить Sotto").click()
    expect(page.get_by_role("alert")).to_contain_text("Файлы приложения не изменяются")
    assert page.evaluate("typeof window.__TAURI_INTERNALS__") == "undefined"
