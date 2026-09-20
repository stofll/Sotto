"""Browser-only UI tests. Never launch Tauri or access application data."""

import json
import os
import socket
import subprocess
import time
from fnmatch import fnmatch
from pathlib import Path
from urllib.request import urlopen

import pytest
from playwright.sync_api import Page, expect
from playwright.sync_api import TimeoutError as PlaywrightTimeoutError

ROOT = Path(__file__).resolve().parents[2]


def pytest_addoption(parser):
    parser.addoption(
        "--ui-mode",
        choices=["production", "dev"],
        default="production",
        help="Test a fresh release build (default) or Vite's development server.",
    )
    parser.addoption(
        "--ui-build-platform",
        choices=["windows", "macos"],
        default="windows",
        help="Tauri frontend build target; does not emulate native OS behavior.",
    )


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
def ui_server(tmp_path_factory, pytestconfig):
    work = tmp_path_factory.mktemp("sotto-ui")
    harness = work / "harness.js"
    subprocess.run(
        ["node", str(ROOT / "tests/ui/harness/build.mjs"), str(harness)], check=True
    )
    production = pytestconfig.getoption("--ui-mode") == "production"
    server_args = []
    if production:
        dist = work / "dist"
        env = dict(os.environ)
        # A developer's debug environment must not silently disable minification.
        env.pop("TAURI_DEBUG", None)
        env["TAURI_PLATFORM"] = pytestconfig.getoption("--ui-build-platform")
        subprocess.run(
            ["node", "node_modules/vite/bin/vite.js", "build", "--outDir", str(dist)],
            cwd=ROOT / "desktop",
            env=env,
            check=True,
        )
        server_args = ["preview", "--outDir", str(dist)]
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
                    *server_args,
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
    def __init__(self, page: Page, allowed_asset_failures):
        self.page = page
        self.allowed_asset_failures = allowed_asset_failures

    def allow_asset_failure(self, pattern):
        """Explicitly allow only the resource a failure-path test interrupts."""
        self.allowed_asset_failures.append(pattern)

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
def app(page, ui_server, pytestconfig, monkeypatch):
    url, harness = ui_server
    page.set_default_timeout(7000)
    page.set_default_navigation_timeout(30000)
    errors = []
    external = []
    asset_failures = []
    allowed_asset_failures = []
    csp_violations = []
    production = pytestconfig.getoption("--ui-mode") == "production"
    if production:
        # Playwright's wait_for_function uses eval on animation frames; WebKit
        # applies script-src to those retries. Poll through the automation API
        # instead of relaxing the application's CSP or bypassing it entirely.
        def wait_for_function(expression, *, arg=None, timeout=7000, polling=50):
            deadline = time.monotonic() + timeout / 1000
            while True:
                handle = page.evaluate_handle(expression, arg=arg)
                if handle.evaluate("value => Boolean(value)"):
                    return handle
                handle.dispose()
                if time.monotonic() >= deadline:
                    raise PlaywrightTimeoutError(
                        f"Predicate did not become true: {expression}"
                    )
                page.wait_for_timeout(
                    polling if isinstance(polling, (int, float)) else 50
                )

        monkeypatch.setattr(page, "wait_for_function", wait_for_function)
    csp = json.loads((ROOT / "desktop/src-tauri/tauri.conf.json").read_text())["app"][
        "security"
    ]["csp"]
    page.on("pageerror", lambda error: errors.append(str(error)))
    page.expose_function(
        "__sottoReportCspViolation", lambda violation: csp_violations.append(violation)
    )
    page.add_init_script("""window.addEventListener('securitypolicyviolation', event => {
        window.__sottoReportCspViolation({
            directive: event.effectiveDirective, blocked: event.blockedURI
        });
    });""")

    asset_types = {"script", "stylesheet", "font", "image"}
    page.on(
        "requestfailed",
        lambda request: (
            asset_failures.append((request.url, request.failure))
            if request.resource_type in asset_types
            else None
        ),
    )
    page.on(
        "response",
        lambda response: (
            asset_failures.append((response.url, response.status))
            if response.status >= 400 and response.request.resource_type in asset_types
            else None
        ),
    )

    def route(request_route):
        if request_route.request.url.startswith(url + "/"):
            if production and request_route.request.resource_type == "document":
                response = request_route.fetch()
                request_route.fulfill(
                    response=response,
                    headers={**response.headers, "Content-Security-Policy": csp},
                )
            else:
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
        return App(page, allowed_asset_failures)

    yield open_app
    unknown = page.evaluate("window.__sottoTest?.unknown ?? []")
    assert not unknown, f"Unhandled IPC commands: {unknown}"
    assert not errors, f"Uncaught browser errors: {errors}"
    assert not external, f"Unexpected external requests: {external}"
    assert not csp_violations, f"Content Security Policy violations: {csp_violations}"
    unexpected_assets = [
        failure
        for failure in asset_failures
        if not any(fnmatch(failure[0], pattern) for pattern in allowed_asset_failures)
    ]
    assert not unexpected_assets, f"Failed application assets: {unexpected_assets}"
