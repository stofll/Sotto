from playwright.sync_api import expect


def test_history_disposes_registration_that_finishes_after_navigation(app, page):
    ui = app()
    page.evaluate("""() => {
        const internals = window.__TAURI_INTERNALS__;
        const invoke = internals.invoke;
        window.heldHistoryListeners = [];
        internals.invoke = async (command, args) => {
            const result = await invoke(command, args);
            if (command === 'plugin:event|listen' && args.event === 'history-updated') {
                await new Promise(resolve => window.heldHistoryListeners.push(resolve));
            }
            return result;
        };
    }""")
    ui.nav("history")
    page.wait_for_function("window.heldHistoryListeners.length > 0")
    ui.nav("settings")
    before = len(ui.calls("list_history"))
    # The native callback exists even though its registration promise is held.
    page.evaluate("window.__sottoTest.emit('history-updated', {})")
    assert len(ui.calls("list_history")) == before
    page.evaluate("window.heldHistoryListeners.forEach(resolve => resolve())")
    page.wait_for_function("window.__sottoTest.subscriptions['history-updated'] === 0")
    page.evaluate("window.__sottoTest.emit('history-updated', {})")
    assert len(ui.calls("list_history")) == before


def test_overlay_ignores_initial_state_after_strict_mode_cleanup(app, page):
    # Only the live mount may issue the readiness handshake; StrictMode cleanup
    # must also release the first mount's native window listeners.
    ui = app("overlay", responses={"current_state": [{"hold": True}]})
    page.wait_for_function("window.__sottoTest.pending('current_state')")
    assert len(ui.calls("overlay_ready")) == 1
    ui.settle("current_state", result="processing")
    expect(page.get_by_test_id("overlay")).to_have_attribute("data-state", "processing")
    page.wait_for_function("window.__sottoTest.subscriptions['overlay-state'] === 1")
    page.wait_for_function("window.__sottoTest.subscriptions['overlay-reset'] === 1")
