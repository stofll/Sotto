"""Browser-only UI tests. Never launch Tauri or access application data."""

import json
import os
import re
import secrets
import shutil
import socket
import subprocess
import time
from contextlib import contextmanager
from fnmatch import fnmatch
from pathlib import Path
from urllib.request import urlopen

import pytest
from filelock import FileLock
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


def pytest_configure(config):
    worker = getattr(config, "workerinput", None)
    if worker is not None:
        # pytest-playwright clears --output at session start; each worker needs
        # its own directory so one cannot erase another's traces or screenshots.
        config.option.output = str(
            Path(config.getoption("--output")) / worker["workerid"]
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


def shared_build(tmp_path_factory, name, build):
    """Run `build(out_dir)` once per test run and return `out_dir`.

    xdist workers share the parent of their base temp directories, so the first
    worker builds there and the others wait for its result instead of repeating
    the same minified build.
    """
    if "PYTEST_XDIST_WORKER" not in os.environ:
        out = tmp_path_factory.mktemp(name) / "dist"
        build(out)
        return out
    root = tmp_path_factory.getbasetemp().parent / name
    out = root / "dist"
    with FileLock(f"{root}.lock"):
        if not (root / "complete").exists():
            # A failed attempt by another worker is rebuilt, not reused.
            shutil.rmtree(root, ignore_errors=True)
            build(out)
            (root / "complete").touch()
    return out


def vite_build(out, args=(), env=None):
    subprocess.run(
        ["node", "node_modules/vite/bin/vite.js", "build", *args, "--outDir", str(out)],
        cwd=ROOT / "desktop",
        env=env,
        check=True,
    )


def build_env(pytestconfig=None):
    env = dict(os.environ)
    # A developer's debug environment must not silently disable minification.
    env.pop("TAURI_ENV_DEBUG", None)
    if pytestconfig is not None:
        env["TAURI_ENV_PLATFORM"] = pytestconfig.getoption("--ui-build-platform")
    return env


@contextmanager
def _vite_server(work, args, *, env=None, log_prefix="vite"):
    env = dict(os.environ if env is None else env)
    # Dev servers sharing a dependency cache, with each other or with a running
    # `pnpm dev`, swap optimized dependencies under one another (504s). Keep one
    # per worker between runs: a cold cache can outlast the first page load.
    worker = os.environ.get("PYTEST_XDIST_WORKER", "main")
    env["SOTTO_VITE_CACHE_DIR"] = str(
        ROOT / "desktop/node_modules" / f".vite-tests-{log_prefix}-{worker}"
    )
    failures = []
    for _ in range(3):
        port = _free_port()
        url = f"http://127.0.0.1:{port}"
        with (work / f"{log_prefix}-{port}.log").open("w+") as log:
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
                if _serving(process, url):
                    yield url
                    return
                log.seek(0)
                failures.append(f"port {port}:\n{log.read()}")
            finally:
                _stop(process)
    pytest.fail("Vite failed to start:\n" + "\n".join(failures))


def warm_dev_server(browser, url, entries, init_script=None):
    """Load each entry once before the tests use a fresh dev server.

    Vite transforms modules on first request. With one server per worker, the
    first page of each can outlast a test's navigation timeout.
    """
    context = browser.new_context()
    try:
        if init_script:
            context.add_init_script(init_script)
        page = context.new_page()
        for entry in entries:
            page.goto(url + entry, timeout=180_000)
            page.wait_for_load_state("networkidle", timeout=180_000)
    finally:
        context.close()


@pytest.fixture(scope="session")
def ui_server(tmp_path_factory, pytestconfig, browser):
    work = tmp_path_factory.mktemp("sotto-ui")
    harness = work / "harness.js"
    subprocess.run(
        ["node", str(ROOT / "tests/ui/harness/build.mjs"), str(harness)], check=True
    )
    server_args = []
    dev = pytestconfig.getoption("--ui-mode") == "dev"
    if not dev:
        env = build_env(pytestconfig)
        dist = shared_build(
            tmp_path_factory, "sotto-ui-dist", lambda out: vite_build(out, env=env)
        )
        server_args = ["preview", "--outDir", str(dist)]
    # A probed port can be taken before Vite binds it; the shared server
    # helper retries both application and setup previews on a fresh port.
    with _vite_server(work, server_args) as url:
        if dev:
            warm_dev_server(
                browser,
                url,
                ["/", "/overlay.html", "/tray.html"],
                harness.read_text() + "\nSottoHarness.install({});",
            )
        yield url, harness.read_text()


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
def app(page, ui_server, pytestconfig, monkeypatch, browser_name):
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

    def report_csp_violation(violation):
        # Playwright's WebKit screenshot preparation inserts this empty rule
        # to synchronize animations. Ignore only that known tooling violation.
        if (
            browser_name == "webkit"
            and violation["directive"] == "style-src-elem"
            and violation["blocked"] == "inline"
            and violation["sample"] == "body {}"
        ):
            return
        csp_violations.append(violation)

    page.expose_function("__sottoReportCspViolation", report_csp_violation)
    page.add_init_script("""window.addEventListener('securitypolicyviolation', event => {
        window.__sottoReportCspViolation({
            directive: event.effectiveDirective, blocked: event.blockedURI,
            sample: event.sample
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
                # Tauri authorizes bundled <style> blocks with per-document
                # nonces. A nonce makes CSP ignore unsafe-inline, including
                # for styles React creates after the document loads.
                nonces = []

                def authorize_style(match):
                    nonce = secrets.token_urlsafe(16)
                    nonces.append(f"'nonce-{nonce}'")
                    return f'<style nonce="{nonce}"'

                body = re.sub(r"<style(?=[\s>])", authorize_style, response.text())
                policy = csp
                if nonces:
                    policy = re.sub(
                        r"(style-src\s+[^;]*)",
                        lambda match: match[1] + " 'report-sample' " + " ".join(nonces),
                        policy,
                    )
                request_route.fulfill(
                    response=response,
                    body=body,
                    headers={**response.headers, "Content-Security-Policy": policy},
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
        # English strings load asynchronously after the window first renders
        # in Russian; clicking before they arrive can hit a shifting layout.
        locale = seed.get("config", {}).get("ui_language", "ru")
        expect(page.locator("html")).to_have_attribute("lang", locale)
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
