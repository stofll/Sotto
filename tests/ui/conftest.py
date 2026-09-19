"""Browser-only UI tests. Never launch Tauri or access application data."""

import json
import socket
import subprocess
import time
from pathlib import Path
from urllib.request import urlopen

import pytest
from playwright.sync_api import Page, expect

ROOT = Path(__file__).resolve().parents[2]

# The sidebar group each tab lives in; a collapsed group hides its tabs.
NAV_GROUPS = {
    "settings": "core",
    "models": "core",
    "text": "processing",
    "ai": "processing",
    "integrations": "integrations",
    "history": "data",
    "stats": "data",
    "info": "help",
}


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _serving(process, url, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if process.poll() is not None:
            return False
        try:
            with urlopen(url, timeout=1) as response:
                if response.status == 200:
                    return True
        except OSError:
            time.sleep(0.1)
    return False


def _stop(process):
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


@pytest.fixture(scope="session")
def ui_server(tmp_path_factory):
    work = tmp_path_factory.mktemp("sotto-ui")
    harness = work / "harness.js"
    subprocess.run(
        ["node", str(ROOT / "tests/ui/harness/build.mjs"), str(harness)], check=True
    )
    # Something else can take the probed port before Vite binds it, and
    # --strictPort turns that race into an immediate exit. Retry on a fresh
    # port rather than failing the whole session on a lost race.
    failures = []
    for _ in range(3):
        port = _free_port()
        url = f"http://127.0.0.1:{port}"
        with (work / f"vite-{port}.log").open("w+") as log:
            process = subprocess.Popen(
                [
                    "node",
                    "node_modules/vite/bin/vite.js",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    str(port),
                    "--strictPort",
                ],
                cwd=ROOT / "desktop",
                stdout=log,
                stderr=subprocess.STDOUT,
            )
            try:
                if _serving(process, url):
                    yield url, harness.read_text()
                    return
                log.seek(0)
                failures.append(f"port {port}:\n{log.read()}")
            finally:
                _stop(process)
    pytest.fail("Vite failed to start:\n" + "\n".join(failures))


@pytest.fixture(scope="session")
def browser_context_args(browser_context_args):
    return {
        **browser_context_args,
        "viewport": {"width": 1280, "height": 1000},
        "locale": "ru-RU",
    }


class App:
    def __init__(self, page: Page):
        self.page = page

    def nav(self, tab):
        group = self.page.get_by_test_id(f"nav-group-{NAV_GROUPS[tab]}")
        if group.is_visible() and group.get_attribute("aria-expanded") == "false":
            group.click()
        self.page.get_by_test_id(f"nav-{tab}").click()
        expect(self.page.get_by_test_id(f"page-{tab}")).to_be_visible()
        return self.page.get_by_test_id(f"page-{tab}")

    def queue(self, command, *answers):
        self.page.evaluate(
            "([c,a]) => window.__sottoTest.queue(c,a)", [command, list(answers)]
        )

    def emit(self, event, payload=None):
        self.page.wait_for_function(
            "e => window.__sottoTest.subscriptions[e] > 0", arg=event
        )
        self.page.evaluate("([e,p]) => window.__sottoTest.emit(e,p)", [event, payload])

    def settle(self, command, **answer):
        self.page.wait_for_function("c => window.__sottoTest.pending(c)", arg=command)
        self.page.evaluate(
            "([c,a]) => window.__sottoTest.settle(c,a)", [command, answer]
        )

    def state(self):
        return self.page.evaluate("window.__sottoTest.state")

    def calls(self, command):
        return self.page.evaluate(
            "c => window.__sottoTest.calls.filter(x => x.command === c)", command
        )

    def saved(self, field, value):
        self.page.wait_for_function(
            "([f,v]) => JSON.stringify(window.__sottoTest.state.config[f]) === JSON.stringify(v)",
            arg=[field, value],
        )


@pytest.fixture
def app(page, ui_server):
    url, harness = ui_server
    page.set_default_timeout(7000)
    page.set_default_navigation_timeout(30000)
    errors = []
    external = []
    page.on("pageerror", lambda error: errors.append(str(error)))

    def route(request_route):
        if request_route.request.url.startswith(url + "/"):
            request_route.continue_()
        else:
            external.append(request_route.request.url)
            request_route.abort()

    page.route("**/*", route)

    opened = []

    def open_app(window="main", **seed):
        # One harness per test. Init scripts accumulate across navigations, so a
        # second call would install the harness twice in the new document and
        # silently keep the first seed. Exercise another window in its own test.
        assert not opened, f"already opened the {opened[0]} window in this test"
        opened.append(window)
        page.add_init_script(
            harness + "\nSottoHarness.install(" + json.dumps(seed) + ");"
        )
        page.goto(url + ("/" if window == "main" else f"/{window}.html"))
        if window == "main":
            expect(page.get_by_test_id("startup-loading")).not_to_be_visible()
            expect(page.get_by_test_id("page-settings")).to_be_visible()
        return App(page)

    yield open_app
    unknown = page.evaluate("window.__sottoTest?.unknown ?? []")
    assert not unknown, f"Unhandled IPC commands: {unknown}"
    assert not errors, f"Uncaught browser errors: {errors}"
    assert not external, f"Unexpected external requests: {external}"
