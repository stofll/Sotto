import re
from datetime import datetime, timedelta
from pathlib import Path

import pytest
from playwright.sync_api import expect


def test_update_current_error_retry(app, page):
    ui = app()
    ui.nav("info")
    expect(
        page.get_by_text("Установлена последняя версия.", exact=True)
    ).to_be_visible()
    ui.queue("check_update", {"error": "Synthetic update failure"})
    page.get_by_role("button", name="Проверить обновления", exact=True).click()
    expect(page.get_by_text("Synthetic update failure", exact=False)).to_be_visible()
    page.get_by_role("button", name="Проверить обновления", exact=True).click()
    expect(
        page.get_by_text("Установлена последняя версия.", exact=True)
    ).to_be_visible()


def test_update_available_and_install_failure(app, page):
    ui = app()
    ui.nav("info")
    expect(
        page.get_by_text("Установлена последняя версия.", exact=True)
    ).to_be_visible()
    ui.queue(
        "check_update",
        {
            "result": {
                "available": True,
                "current_version": "0.0.5",
                "version": "0.0.6",
                "notes": "Synthetic release notes",
            }
        },
    )
    page.get_by_role("button", name="Проверить обновления", exact=True).click()
    expect(page.get_by_text("Synthetic release notes", exact=True)).to_be_visible()
    ui.queue("install_update", {"hold": True})
    page.get_by_role("button", name="Обновить до 0.0.6", exact=True).click()
    ui.settle("install_update", error="Synthetic signature failure")
    expect(page.get_by_text("Synthetic signature failure", exact=False)).to_be_visible()


def test_clear_logs_confirmation_cancel_and_apply(app, page):
    ui = app()
    ui.nav("info")
    page.get_by_role("button", name="Очистить логи", exact=True).click()
    dialog = page.get_by_role("alertdialog")
    expect(dialog.get_by_role("button", name="Отмена", exact=True)).to_be_focused()
    page.keyboard.press("Escape")
    expect(dialog).not_to_be_visible()
    assert not ui.calls("clear_logs")
    page.get_by_role("button", name="Очистить логи", exact=True).click()
    dialog.get_by_role("button", name="Очистить", exact=True).click()
    expect(page.get_by_text("0 КБ", exact=True)).to_be_visible()


def test_stats_periods_and_refresh(app, page):
    ui = app(stats={"total_transcriptions": 1234, "total_characters": 5678})
    ui.nav("stats")
    page.get_by_role("button", name="Всё время", exact=True).click()
    expect(page.get_by_test_id("page-stats")).to_contain_text(re.compile(r"1\s*234"))
    for period in ["Неделя", "Месяц", "Год"]:
        button = page.get_by_role("button", name=period, exact=True)
        button.click()
        expect(button).to_have_attribute("aria-pressed", "true")
    next_stats = {**ui.state()["stats"], "total_transcriptions": 4321}
    ui.queue("get_stats", {"result": next_stats})
    page.get_by_role("button", name="Обновить", exact=True).click()
    page.get_by_role("button", name="Всё время", exact=True).click()
    expect(page.get_by_test_id("page-stats")).to_contain_text(re.compile(r"4\s*321"))


def test_paste_test_waits_before_pasting_and_reports_success(app, page):
    # The countdown is the whole point of the control: pasting on the click
    # itself would deliver the text into this settings window.
    ui = app()
    ui.nav("info")
    ui.queue(
        "test_paste", {"result": "Paste OK. Text на буфере: Тест вставки Sotto — 1"}
    )
    button = page.get_by_test_id("paste-test")
    button.click()
    expect(button).to_be_disabled()
    assert not ui.calls("test_paste")
    expect(page.get_by_test_id("paste-test-result")).to_contain_text(
        "Paste OK", timeout=10_000
    )
    assert len(ui.calls("test_paste")) == 1
    expect(button).to_be_enabled()


def test_paste_test_shows_the_failure_reason(app, page):
    ui = app()
    ui.nav("info")
    ui.queue("test_paste", {"error": "Paste FAILED: all paste strategies failed"})
    page.get_by_test_id("paste-test").click()
    expect(page.get_by_test_id("paste-test-result")).to_contain_text(
        "all paste strategies failed", timeout=10_000
    )
    expect(page.get_by_test_id("paste-test")).to_be_enabled()


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_stats_silence_estimate_and_legacy_fallback(
    app, page, locale, theme, output_path
):
    page.set_viewport_size({"width": 1000, "height": 710})
    today = datetime.now().astimezone().date().isoformat()
    ui = app(
        config={"ui_language": locale, "theme": theme, "typing_speed_cpm": 240},
        stats={
            "total_transcriptions": 16,
            "total_characters": 1133,
            "total_audio_seconds": 399,
            "total_processing_seconds": 19,
            "total_excluded_silence_seconds": 180,
            "total_speech_timed_transcriptions": 15,
            "daily_history": [
                {
                    "date": today,
                    "count": 16,
                    "chars": 1133,
                    "audio_seconds": 399,
                    "processing_seconds": 19,
                    "excluded_silence_seconds": 180,
                    "speech_timed_count": 15,
                }
            ],
        },
    )
    region = ui.nav("stats")
    label = "Оценка экономии" if locale == "ru" else "Estimated savings"
    card = region.locator(".stat").filter(has=page.get_by_text(label, exact=True))
    expect(card.locator(".stat__value")).to_have_text(
        "1 м" if locale == "ru" else "1 m"
    )
    expect(card.locator(".stat__sub")).to_have_text(
        "паузы учтены частично" if locale == "ru" else "pauses partially accounted for"
    )
    page.mouse.move(0, 0)
    card.locator(".hint").focus()
    expect(
        page.locator(".hint-bubble").filter(
            has_text="Из аудио исключено:" if locale == "ru" else "Excluded from audio:"
        )
    ).to_contain_text("короткими паузами" if locale == "ru" else "short pauses")
    assert region.evaluate("e => e.scrollWidth - e.clientWidth") <= 1
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(
        path=str(Path(output_path) / "stats-silence.png"), animations="disabled"
    )
    page.keyboard.press("Escape")
    for period in (
        ["Неделя", "Год", "Всё время"]
        if locale == "ru"
        else ["Week", "Year", "All time"]
    ):
        page.get_by_role("button", name=period, exact=True).click()
        expect(card.locator(".stat__value")).to_have_text(
            "1 м" if locale == "ru" else "1 m"
        )
    # A refresh from an older backend has no silence fields: retain the old estimate.
    legacy = {**ui.state()["stats"]}
    legacy.pop("total_excluded_silence_seconds")
    legacy.pop("total_speech_timed_transcriptions")
    ui.queue("get_stats", {"result": legacy})
    page.get_by_role(
        "button", name="Обновить" if locale == "ru" else "Refresh", exact=True
    ).click()
    expect(card.locator(".stat__value")).to_have_text(
        "-2 м" if locale == "ru" else "-2 m"
    )


def test_stats_no_detection_and_empty_period(app, page):
    ui = app(
        stats={
            "total_transcriptions": 1,
            "total_characters": 40,
            "total_audio_seconds": 15,
            "total_processing_seconds": 1,
            "total_excluded_silence_seconds": 0,
            "total_speech_timed_transcriptions": 0,
            "daily_history": [],
        }
    )
    ui.nav("stats")
    card = page.locator(".stat").filter(
        has=page.get_by_text("Оценка экономии", exact=True)
    )
    expect(card.locator(".stat__value")).to_have_text("0 м")
    expect(
        page.get_by_text("Записей без оценки пауз:", exact=False)
    ).not_to_be_visible()

    page.get_by_role("button", name="Всё время", exact=True).click()
    # A small negative estimate should round to 0, never '-0'.
    expect(card.locator(".stat__value")).to_have_text("0 м")
    expect(
        page.get_by_text("Записей без оценки пауз:", exact=False)
    ).not_to_be_visible()


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("timed", [0, 1, 501])
def test_stats_hint_explains_measurement_coverage(app, page, locale, timed):
    ui = app(
        config={"ui_language": locale},
        stats={
            "total_transcriptions": 501,
            "total_speech_timed_transcriptions": timed,
            "total_audio_seconds": 1000,
            "total_excluded_silence_seconds": 10 if timed else 0,
            "daily_history": [],
        },
    )
    ui.nav("stats")
    page.get_by_role(
        "button", name="Всё время" if locale == "ru" else "All time", exact=True
    ).click()
    card = page.locator(".stat").filter(
        has=page.get_by_text(
            "Оценка экономии" if locale == "ru" else "Estimated savings", exact=True
        )
    )
    expected = {
        0: ("минус аудио и обработка", "minus audio and processing"),
        1: ("паузы учтены частично", "pauses partially accounted for"),
        501: ("без длительных пауз", "excluding long pauses"),
    }[timed][locale == "en"]
    expect(card.locator(".stat__sub")).to_have_text(expected)
    card.locator(".hint").focus()
    hint = page.locator(".hint-bubble")
    if timed == 0:
        expect(hint).to_contain_text(
            "Замеров речи за этот период нет"
            if locale == "ru"
            else "No speech measurements for this period"
        )
        expect(hint).not_to_contain_text(
            "Из аудио исключено" if locale == "ru" else "Excluded from audio"
        )
    elif timed == 1:
        expect(hint).to_contain_text("1 из 501" if locale == "ru" else "1 of 501")
    else:
        expect(hint).to_contain_text(
            "Длинные паузы исключаются"
            if locale == "ru"
            else "Long pauses are excluded"
        )


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_stats_smooth_timing_chart(app, page, locale, theme, output_path):
    page.set_viewport_size({"width": 1000, "height": 710})
    today = datetime.now().astimezone().date()
    stt = [
        1,
        0,
        0,
        1,
        0,
        0,
        17,
        4,
        10,
        13,
        4,
        1,
        4,
        12,
        4,
        2,
        5,
        4,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        12,
        5,
        4,
        0,
    ]
    llm = [
        2,
        0,
        0,
        9,
        0,
        0,
        76,
        23,
        49,
        44,
        32,
        7,
        50,
        70,
        21,
        34,
        39,
        36,
        1,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        81,
        19,
        36,
        0,
    ]
    ui = app(
        config={"ui_language": locale, "theme": theme},
        stats={
            "daily_history": [
                {
                    "date": (today - timedelta(days=29 - i)).isoformat(),
                    "count": 1,
                    "chars": 100,
                    "speech_timed_count": 1,
                    "whisper_seconds": stt[i],
                    "llm_seconds": llm[i],
                }
                for i in range(30)
            ],
        },
    )
    region = ui.nav("stats")
    chart = region.get_by_role(
        "img", name="Время этапа по дням" if locale == "ru" else "Stage time per day"
    )
    chart.scroll_into_view_if_needed()
    curves = chart.locator('path[fill="none"]')
    expect(curves).to_have_count(2)
    # Inspect the actual SVG geometry in both engines, including gradient resolution.
    geometry = chart.evaluate("""svg => [...svg.querySelectorAll('path[fill="none"]')].map(path => ({
        length: path.getTotalLength(), bounds: {y: path.getBBox().y, height: path.getBBox().height},
        dash: getComputedStyle(path).strokeDasharray,
    }))""")
    for curve in geometry:
        assert curve["length"] > 100
        assert curve["bounds"]["y"] >= 9.99
        assert curve["bounds"]["y"] + curve["bounds"]["height"] <= 100.01
        assert curve["dash"] == "none"
    assert chart.evaluate("""svg => [...svg.querySelectorAll('path[fill^="url"]')].every(path => {
        const id = path.getAttribute('fill').slice(5, -1);
        return svg.querySelectorAll('linearGradient').length === 2 && document.getElementById(id);
    })""")
    Path(output_path).mkdir(parents=True, exist_ok=True)
    region.locator(".chart-card").first.screenshot(
        path=str(Path(output_path) / "timing-chart.png")
    )
    for label in ["Неделя", "Год"] if locale == "ru" else ["Week", "Year"]:
        page.get_by_role("button", name=label, exact=True).click()
        expect(chart).to_be_visible()
    ui.queue("get_stats", {"result": {**ui.state()["stats"], "daily_history": []}})
    page.get_by_role(
        "button", name="Обновить" if locale == "ru" else "Refresh", exact=True
    ).click()
    expect(curves.first).to_have_attribute(
        "d",
        re.compile(
            r"^M0\.0000,100\.0000(?: C[\d.]+,100\.0000 [\d.]+,100\.0000 [\d.]+,100\.0000)+$"
        ),
    )
    assert chart.evaluate("""svg => [...svg.querySelectorAll('path[fill="none"]')].every(path => {
        const box = path.getBBox(); return Math.abs(box.y-100)<0.001 && box.height<0.001;
    })""")
