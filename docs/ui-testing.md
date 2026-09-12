# Browser UI testing

The UI suite uses Python, pytest and Playwright against Sotto's real Vite entry points. It complements Vitest logic/bridge tests and native Rust tests. These are browser UI integration tests with a controlled backend, not end-to-end verification of a packaged Tauri application.

## Run

Use the repository's Node and pnpm versions. Install frontend dependencies with `pnpm install --frozen-lockfile` in `desktop/`, then run from the repository root:

```bash
uv sync --locked --project tests/ui
uv run --locked --project tests/ui playwright install chromium webkit
uv run --locked --project tests/ui pytest tests/ui --browser chromium --browser webkit
```

Python 3.12 or newer is required. `tests/ui/uv.lock` pins Python dependencies; CI installs uv 0.11.29 and uses Python 3.12 on Ubuntu 24.04. On a fresh Linux machine, use `playwright install --with-deps chromium webkit` to install browser system dependencies too.

The session fixture starts Vite on an available loopback port, builds the test harness in a temporary directory, waits for server readiness and stops its own process at teardown. Do not launch `pnpm tauri dev` for this suite. No Rust build, microphone, model download, credentials or existing Sotto installation is needed.

Useful focused runs:

```bash
uv run --locked --project tests/ui pytest tests/ui/test_settings.py --headed
uv run --locked --project tests/ui pytest tests/ui/test_windows.py --browser webkit
uv run --locked --project tests/ui pytest tests/ui -k cancellation
uv run --locked --project tests/ui ruff check tests/ui
uv run --locked --project tests/ui ruff format --check tests/ui
cd desktop
pnpm exec tsc -p ../tests/ui/tsconfig.json
```

## Test boundary and isolation

Playwright injects the separately bundled `tests/ui/harness/runtime.ts` before application JavaScript. It uses the installed Tauri package's `mockIPC` and `mockWindows`, including event mocking. Production entry points do not import this harness, and no production test mode or IPC bypass is added.

Each test receives a fresh browser context. Synthetic configuration, model metadata, history and key availability live in memory; session storage preserves them only across that test's page reloads. No application config directory, history database, recording directory or operating-system credential store is accessed. Test keys and paths must always be synthetic.

Unknown IPC commands, uncaught page errors and external HTTP requests fail the test. An expected backend failure must be explicitly queued in the test and handled by the actual UI. Processing outputs are supplied fixtures: the harness does not reproduce Rust transcription, formatting or provider algorithms. A UI assertion about a mocked result proves how the frontend presents that result, not whether Rust can produce it.

`app()` opens settings by default; `app("overlay")` and `app("tray")` open the other entry points. Initial state can be supplied through `config`, `models`, `history`, `keys`, `runtime` and `stats`. `queue` supplies one-shot command results, errors or held promises; `settle` completes a held command. `emit` waits for a live subscription before delivering an event, avoiding arbitrary UI sleeps.

## Coverage map

The executable test modules are the detailed scenario inventory. Extend the relevant module when adding a feature or fixing a regression; do not infer complete code or branch coverage from a passing suite.

| Area | Test module | Covered behavior |
| --- | --- | --- |
| Shell | `test_navigation.py` | All eight pages, RU/EN and themes, theme persistence/rollback, startup failure, permission banners, legacy navigation events |
| Settings | `test_settings.py` | Preferences and reload, paste dependencies, recording mode, locale, microphone selection/test/meter, hotkey validation/cancel, portable autostart, telemetry |
| Models | `test_models.py` | Search, selection/rollback, confirmations, download progress/cancellation/failure/success, deletion, missing-model guidance |
| Text | `test_text.py` | Preview draft and error, replacement persistence, import/export and failed-save retry, dictionary creation/search/deletion and unsaved changes |
| LLM and files | `test_ai.py` | Keyboard route selection, missing-key gate, manual result/fallback/error, file selection/loading/result/error/retry/cancellation |
| Providers and keys | `test_profiles.py` | Wizard search, URL validation, unsaved guard, local profile creation, rename/delete, key masking/replacement/deletion, credential-store failures, connection retry |
| History | `test_history.py` | Empty/loading failure, search including raw text, no results, single deletion/error, clear confirmation, selection/view mode, partial bulk-delete failure, raw details, reprocess preview/apply/retry |
| Help and statistics | `test_info_stats.py` | Update checks/retry/install failure, log cleanup confirmation, statistics periods and refreshed totals |
| Lightweight windows | `test_windows.py` | Tray recording/navigation/settings events, overlay lifecycle, stale events, cancellation/silence/error recovery, streaming, initial-state handshake |
| Layout | `test_layout.py` | Minimum settings-window size, themes/locales, horizontal overflow, reviewable page and overlay screenshots |

## Results and visual review

The suite saves failure traces and screenshots under `test-results/`. The layout suite also saves screenshots on success for visual review; these are not approved pixel-diff baselines. Open an individual trace with `uv run --locked --project tests/ui playwright show-trace <path-to-trace.zip>`. Keep generated reports and screenshots out of commits.

`.github/workflows/ui-tests.yml` runs Chromium and WebKit on pull requests and supports manual dispatch. It uploads the JUnit report and browser artifacts for seven days. Use a different `--output` directory for concurrent local runs because pytest-playwright cleans its output directory at session start.

## Native verification remains required

Browser Chromium and WebKit approximate rendering engines, not Windows WebView2 or macOS WKWebView integration. These tests do not validate actual window focus/positioning, global shortcuts, audio capture, clipboard delivery, model inference, system dialogs, credential storage, updater verification or installation. The windows are exercised as separate browser pages with controlled events, not as a running multi-window Tauri process.

Follow [Testing](testing.md#native-verification-before-release) for native checks on each affected OS with isolated application data. Add native automation separately when testing these boundaries; do not replace it with extra mocked UI assertions or describe this suite as 100% application coverage.
