from pathlib import Path

import pytest
from playwright.sync_api import expect


def test_unknown_speed_uses_hatching(app, page):
    ui = app()
    ui.nav("models")
    card = page.get_by_test_id("model-tiny")
    expect(card.get_by_role("img", name="Скорость: Нет замера")).to_be_visible()
    expect(card.locator(".model-meter__unknown")).to_have_count(1)


def test_search_and_reset(app, page):
    ui = app()
    ui.nav("models")
    search = page.get_by_role("searchbox", name="Поиск по названию")
    search.fill("GigaAM")
    expect(page.get_by_test_id("model-gigaam")).to_be_visible()
    expect(page.get_by_test_id("model-tiny")).not_to_be_visible()
    search.fill("missing synthetic model")
    expect(page.get_by_test_id("model-gigaam")).not_to_be_visible()
    search.fill("")
    expect(page.get_by_test_id("model-tiny")).to_be_visible()


def test_select_installed_model_with_keyboard(app, page):
    ui = app()
    ui.nav("models")
    model = page.get_by_test_id("model-gigaam")
    model.focus()
    model.press("Enter")
    page.get_by_role("dialog").get_by_role("button", name="Выбрать", exact=True).click()
    ui.saved("model", "gigaam")
    expect(model).to_have_attribute("aria-pressed", "true")
    page.reload()
    ui.nav("models")
    expect(page.get_by_test_id("model-gigaam")).to_have_attribute(
        "aria-pressed", "true"
    )


def test_download_confirmation_can_cancel(app, page):
    ui = app()
    ui.nav("models")
    page.get_by_test_id("model-base").click()
    dialog = page.get_by_role("dialog", name="Скачать модель?")
    expect(dialog).to_contain_text("Whisper Base")
    page.keyboard.press("Escape")
    expect(dialog).not_to_be_visible()
    assert not ui.calls("download_model")


def test_delete_model_updates_catalog(app, page):
    ui = app()
    ui.nav("models")
    card = page.get_by_test_id("model-gigaam")
    card.get_by_role("button", name="Действия с моделью").click()
    page.get_by_role("menuitem", name="Удалить", exact=True).click()
    page.get_by_role("dialog").get_by_role("button", name="Удалить", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.state.models.find(m=>m.id==='gigaam').downloaded === false"
    )
    card.click()
    expect(page.get_by_role("dialog", name="Скачать модель?")).to_be_visible()


def test_no_model_banner_navigates_to_catalog(app, page):
    app(
        models=[
            {
                "id": "tiny",
                "label": "Whisper Tiny",
                "size": "75 MB",
                "ram": "400 MB",
                "downloaded": False,
                "selected": True,
            }
        ],
        runtime={"model_loaded": False, "loaded_model": None},
    )
    expect(
        page.get_by_text("Модель распознавания не скачана.", exact=True)
    ).to_be_visible()
    page.get_by_role("button", name="Скачать модель", exact=True).click()
    expect(page.get_by_test_id("page-models")).to_be_visible()


def test_model_selection_failure_preserves_previous(app, page):
    ui = app()
    ui.nav("models")
    ui.queue("set_model", {"error": "Synthetic model load failure"})
    page.get_by_test_id("model-gigaam").click()
    page.get_by_role("dialog").get_by_role("button", name="Выбрать", exact=True).click()
    expect(page.get_by_role("alert")).to_contain_text("Synthetic model load failure")
    expect(page.get_by_test_id("model-tiny")).to_have_attribute("aria-pressed", "true")
    assert ui.state()["config"]["model"] == "tiny"


def test_download_progress_cancel_and_retry(app, page):
    ui = app()
    ui.nav("models")
    ui.queue("download_model", {"hold": True})
    ui.queue("cancel_model_download", {"result": None})
    page.get_by_test_id("model-base").click()
    page.get_by_role("dialog").get_by_role(
        "button", name="Скачать", exact=False
    ).click()
    expect(
        page.get_by_role("button", name="Отменить скачивание", exact=True)
    ).to_be_visible()
    ui.emit(
        "model-download-progress",
        {"model": "base", "percent": 50, "downloaded": 50, "total": 100},
    )
    expect(page.get_by_role("progressbar")).to_have_attribute("aria-valuenow", "50")
    page.get_by_role("button", name="Отменить скачивание", exact=True).click()
    ui.settle("download_model", result=None)
    expect(page.get_by_text("Загрузка отменена", exact=True)).to_be_visible()
    ui.queue("download_model", {"error": "Synthetic network failure"})
    page.get_by_test_id("model-base").click()
    page.get_by_role("dialog").get_by_role(
        "button", name="Скачать", exact=False
    ).click()
    expect(page.get_by_role("alert")).to_contain_text("Synthetic network failure")


def test_download_success_selects_model(app, page):
    ui = app()
    ui.nav("models")
    ui.queue("download_model", {"hold": True})
    page.get_by_test_id("model-base").click()
    page.get_by_role("dialog").get_by_role(
        "button", name="Скачать", exact=False
    ).click()
    # Model availability is a backend output; the test supplies the completed catalog.
    next_models = ui.state()["models"]
    next_models[1]["downloaded"] = True
    ui.queue("list_models", {"result": next_models}, {"result": next_models})
    ui.settle("download_model", result={"downloaded": True})
    expect(page.get_by_role("status")).to_contain_text("Модель скачана и активна")
    ui.saved("model", "base")


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_compact_speed_reads_without_a_detail_panel(
    app, page, locale, theme, output_path
):
    ui = app(
        config={"ui_language": locale, "theme": theme},
        assessments=[
            {
                "id": "tiny",
                "compute": "cpu",
                "speed": {"score": 0.9, "source": "reference"},
                "memory": {
                    "score": 0.7,
                    "status": "enough",
                    "required_bytes": 2 * 1024**3,
                    "available_bytes": 8 * 1024**3,
                },
            }
        ],
    )
    ui.nav("models")
    card = page.get_by_test_id("model-tiny")
    speed_name = "Скорость" if locale == "ru" else "Speed"
    speed = card.get_by_role("img", name=speed_name + ":")
    expect(speed.locator("svg")).to_have_count(0)
    expect(speed.locator(".model-meter__fill")).to_have_css("width", "48px")
    speed.hover()
    tooltip = page.get_by_role("tooltip")
    expect(tooltip).to_contain_text(
        "Высокая относительная" if locale == "ru" else "High relative"
    )
    # The number is a comparative benchmark, and the card says so rather than
    # naming the machine it came from.
    expect(tooltip).to_contain_text(
        "может отличаться" if locale == "ru" else "may differ"
    )
    speed.focus()
    expect(tooltip).to_be_visible()
    speed.press("Escape")
    expect(tooltip).not_to_be_visible()
    speed.click()
    speed.press("Enter")
    expect(page.get_by_role("region", name=speed_name, exact=True)).to_have_count(0)
    expect(page.get_by_role("dialog")).to_have_count(0)
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(path=str(Path(output_path) / "models.png"), animations="disabled")


@pytest.mark.parametrize("available", [None, 0, 2 * 1024**3])
def test_disk_capacity_controls_download_without_blocking_unknown(app, page, available):
    ui = app(
        assessments=[
            {
                "id": "base",
                "compute": "cpu",
                "speed": {"score": None, "source": "unknown"},
                "memory": {
                    "status": "unknown",
                    "required_bytes": None,
                    "available_bytes": None,
                },
                "download": {
                    "required_bytes": 1024**3,
                    "available_bytes": available,
                    "insufficient": available == 0,
                },
            }
        ]
    )
    ui.nav("models")
    card = page.get_by_test_id("model-base")
    if available == 0:
        warning = card.get_by_role("button", name="Не хватает места")
        expect(warning).to_be_visible()
        expect(card.get_by_role("button", name="Скачать модель")).to_have_count(0)
        warning.focus()
        expect(page.get_by_role("tooltip")).to_contain_text("свободно 0 ГБ")
        card.click()
        expect(page.get_by_role("dialog")).to_have_count(0)
        assert not ui.calls("download_model")
    else:
        expect(card.get_by_role("button", name="Скачать модель")).to_be_visible()


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_disk_warning_recovers_on_focus_and_spares_installed_models(
    app, page, locale, theme, output_path
):
    rows = [
        {
            "id": model,
            "compute": "cpu",
            "speed": {"score": None, "source": "unknown"},
            "memory": {
                "status": "unknown",
                "required_bytes": None,
                "available_bytes": None,
            },
            "download": {
                "required_bytes": 1024**3,
                "available_bytes": 0,
                "insufficient": True,
            },
        }
        for model in ["tiny", "base"]
    ]
    ui = app(config={"ui_language": locale, "theme": theme}, assessments=rows)
    ui.nav("models")
    card = page.get_by_test_id("model-base")
    warning_name = "Не хватает места" if locale == "ru" else "Not enough space"
    download_name = "Скачать модель" if locale == "ru" else "Download model"
    expect(
        page.get_by_test_id("model-tiny").get_by_role("button", name=warning_name)
    ).to_have_count(0)
    warning = card.get_by_role("button", name=warning_name)
    expect(warning).to_be_visible()
    warning.focus()
    expect(page.get_by_role("tooltip")).to_be_visible()
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(path=str(Path(output_path) / "disk.png"), animations="disabled")
    for row in rows:
        row["download"]["available_bytes"] = 2 * 1024**3
        row["download"]["insufficient"] = False
    ui.queue("model_assessments", {"result": rows})
    page.evaluate("() => window.dispatchEvent(new Event('focus'))")
    expect(card.get_by_role("button", name=download_name)).to_be_visible()
    expect(card.get_by_role("button", name=warning_name)).to_have_count(0)
