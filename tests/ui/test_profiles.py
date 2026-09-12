from playwright.sync_api import expect


def wizard(ui, page):
    ui.nav("integrations")
    page.get_by_role("button", name="Новый профиль", exact=True).click()
    return page.get_by_role("dialog", name="Новый профиль LLM")


def test_wizard_search_and_clean_close(app, page):
    ui = app()
    dialog = wizard(ui, page)
    dialog.get_by_placeholder("Поиск: провайдер, пресет, адрес…").fill(
        "nonexistent-provider"
    )
    expect(
        dialog.get_by_role("button", name="Своя конфигурация", exact=False)
    ).to_be_visible()
    dialog.get_by_role("button", name="Закрыть", exact=True).click()
    expect(dialog).not_to_be_visible()
    assert not ui.state()["config"]["ai_processing"]["profiles"]


def test_wizard_invalid_url_and_unsaved_guard(app, page):
    ui = app()
    dialog = wizard(ui, page)
    dialog.get_by_role("button", name="Своя конфигурация", exact=False).click()
    dialog.get_by_label("Base URL", exact=True).fill("not a url")
    expect(dialog.get_by_role("button", name="Далее", exact=True)).to_be_disabled()
    dialog.get_by_label("Base URL", exact=True).fill("https://api.example.com/v1")
    expect(dialog.get_by_role("button", name="Далее", exact=True)).to_be_enabled()
    dialog.get_by_role("button", name="Закрыть", exact=True).click()
    guard = page.get_by_role("alertdialog")
    expect(guard.get_by_role("button", name="Остаться", exact=True)).to_be_focused()
    guard.get_by_role("button", name="Остаться", exact=True).click()
    expect(dialog.get_by_label("Base URL", exact=True)).to_have_value(
        "https://api.example.com/v1"
    )
    dialog.get_by_role("button", name="Закрыть", exact=True).click()
    guard.get_by_role("button", name="Закрыть без сохранения", exact=True).click()
    expect(dialog).not_to_be_visible()


def test_create_local_profile(app, page):
    ui = app()
    dialog = wizard(ui, page)
    dialog.get_by_role("button", name="Своя конфигурация", exact=False).click()
    dialog.get_by_label("Base URL", exact=True).fill("http://localhost:1234/v1")
    dialog.get_by_role("button", name="Далее", exact=True).click()
    dialog.get_by_role(
        "checkbox", name="Ключ не нужен — сервер локальный", exact=False
    ).check()
    dialog.get_by_role("button", name="Далее", exact=True).click()
    dialog.get_by_label("Название профиля", exact=True).fill("Synthetic local profile")
    dialog.get_by_placeholder("например: gpt-oss-120b").fill("synthetic-model")
    dialog.get_by_role("button", name="Создать профиль", exact=True).click()
    expect(dialog).not_to_be_visible()
    expect(page.get_by_test_id("page-integrations")).to_contain_text(
        "Synthetic local profile"
    )
    page.reload()
    ui.nav("integrations")
    expect(page.get_by_test_id("page-integrations")).to_contain_text(
        "Synthetic local profile"
    )
    profiles = ui.state()["config"]["ai_processing"]["profiles"]
    assert len(profiles) == 1
    assert profiles[0]["model"] == "synthetic-model"


PROFILE = {
    "id": "synthetic",
    "name": "Synthetic profile",
    "provider": "openai",
    "model": "synthetic-model",
    "api_key_ref": "openai",
    "system_prompt": "Synthetic prompt",
}


def test_profile_rename_and_delete(app, page):
    ui = app(
        config={
            "ai_processing": {"profiles": [PROFILE], "active_profile_id": "synthetic"}
        }
    )
    ui.nav("integrations")
    row = page.get_by_test_id("profile-synthetic")
    row.get_by_role("button", name="Действия с профилем", exact=True).click()
    page.get_by_role("menuitem", name="Переименовать", exact=True).click()
    name = row.get_by_role("textbox", name="Название профиля")
    name.fill("Renamed profile")
    name.press("Enter")
    expect(row).to_contain_text("Renamed profile")
    page.wait_for_function(
        "window.__sottoTest.state.config.ai_processing.profiles[0].name === 'Renamed profile'"
    )
    row.get_by_role("button", name="Действия с профилем", exact=True).click()
    page.get_by_role("menuitem", name="Удалить", exact=True).click()
    page.get_by_role("alertdialog").get_by_role(
        "button", name="Удалить", exact=True
    ).click()
    expect(row).not_to_be_visible()
    assert ui.state()["config"]["ai_processing"]["profiles"] == []


def test_api_key_create_reveal_replace_delete(app, page):
    ui = app()
    ui.nav("integrations")
    page.get_by_role("button", name="Добавить ключ", exact=True).click()
    dialog = page.get_by_role("dialog", name="Новый API-ключ")
    expect(
        dialog.get_by_role("button", name="Сохранить ключ", exact=True)
    ).to_be_disabled()
    dialog.get_by_label("Метка (опционально)", exact=True).fill("Synthetic key")
    key = dialog.get_by_placeholder("sk-...", exact=True)
    key.fill("synthetic-not-a-real-secret")
    expect(key).to_have_attribute("type", "password")
    dialog.get_by_role("button", name="Показать ключ", exact=True).click()
    expect(key).to_have_attribute("type", "text")
    dialog.get_by_role("button", name="Скрыть ключ", exact=True).click()
    expect(key).to_have_attribute("type", "password")
    dialog.get_by_role("button", name="Сохранить ключ", exact=True).click()
    expect(dialog).not_to_be_visible()
    ref = ui.state()["config"]["ai_processing"]["key_slots"][0]["ref"]
    row = page.get_by_test_id(f"key-{ref}")
    expect(row).to_contain_text("Synthetic key")
    expect(row).not_to_contain_text("synthetic-not-a-real-secret")
    row.get_by_role("button", name="Действия с ключом", exact=True).click()
    page.get_by_role("menuitem", name="Заменить ключ", exact=True).click()
    row.get_by_placeholder("Новое значение ключа", exact=True).fill(
        "synthetic-replacement"
    )
    row.get_by_role("button", name="Сохранить", exact=True).click()
    expect(
        row.get_by_placeholder("Новое значение ключа", exact=True)
    ).not_to_be_visible()
    page.reload()
    ui.nav("integrations")
    row.get_by_role("button", name="Действия с ключом", exact=True).click()
    page.get_by_role("menuitem", name="Удалить", exact=True).click()
    page.get_by_role("alertdialog").get_by_role(
        "button", name="Удалить", exact=True
    ).click()
    expect(row).not_to_be_visible()


def test_api_key_save_failure_is_visible_and_retryable(app, page):
    ui = app()
    ui.nav("integrations")
    page.get_by_role("button", name="Добавить ключ", exact=True).click()
    dialog = page.get_by_role("dialog", name="Новый API-ключ")
    dialog.get_by_placeholder("sk-...", exact=True).fill("synthetic-secret")
    ui.queue("save_api_key", {"error": "Synthetic credential store failure"})
    dialog.get_by_role("button", name="Сохранить ключ", exact=True).click()
    expect(dialog.get_by_role("alert")).to_contain_text(
        "Synthetic credential store failure"
    )
    expect(dialog).to_be_visible()
    dialog.get_by_role("button", name="Сохранить ключ", exact=True).click()
    expect(dialog).not_to_be_visible()


def test_key_replace_and_delete_failure_preserve_key(app, page):
    ui = app(
        config={
            "ai_processing": {
                "key_slots": [
                    {
                        "ref": "synthetic-key",
                        "label": "Synthetic key",
                        "provider": "openai",
                    }
                ]
            }
        },
        keys={
            "synthetic-key": {
                "available": True,
                "label": "Synthetic key",
                "masked": "test-***",
            }
        },
    )
    ui.nav("integrations")
    row = page.get_by_test_id("key-synthetic-key")
    row.get_by_role("button", name="Действия с ключом", exact=True).click()
    page.get_by_role("menuitem", name="Заменить ключ", exact=True).click()
    ui.queue("save_api_key", {"error": "Synthetic replace failure"})
    row.get_by_placeholder("Новое значение ключа", exact=True).fill(
        "synthetic-replacement"
    )
    row.get_by_role("button", name="Сохранить", exact=True).click()
    expect(page.get_by_role("status")).to_contain_text("Synthetic replace failure")
    expect(row.get_by_placeholder("Новое значение ключа", exact=True)).to_be_visible()
    row.get_by_role("button", name="Отмена", exact=True).click()
    ui.queue("delete_api_key", {"error": "Synthetic delete failure"})
    row.get_by_role("button", name="Действия с ключом", exact=True).click()
    page.get_by_role("menuitem", name="Удалить", exact=True).click()
    page.get_by_role("alertdialog").get_by_role(
        "button", name="Удалить", exact=True
    ).click()
    expect(page.get_by_role("status")).to_contain_text("Synthetic delete failure")
    expect(row).to_be_visible()
    assert ui.state()["keys"]["synthetic-key"]["available"]


def test_profile_connection_failure_and_retry(app, page):
    ui = app(
        config={"ai_processing": {"profiles": [PROFILE]}},
        keys={
            "openai": {"available": True, "label": "Synthetic", "masked": "test-***"}
        },
    )
    ui.nav("integrations")
    row = page.get_by_test_id("profile-synthetic")
    row.get_by_role("button", name="Synthetic profile", exact=False).click()
    ui.queue("test_ai_prompt", {"error": "Synthetic connection failure"})
    row.get_by_role("button", name="Проверить связь", exact=True).click()
    expect(row.get_by_role("status")).to_contain_text("Synthetic connection failure")
    ui.queue("test_ai_prompt", {"result": {"available": True}})
    row.get_by_role("button", name="Проверить связь", exact=True).click()
    expect(row.get_by_role("status")).to_contain_text("Тест пройден")
