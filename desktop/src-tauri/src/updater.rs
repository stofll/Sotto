//! Самообновление через `tauri-plugin-updater`.
//!
//! Плагин ходит за `latest.json` на GitHub Releases, сверяет minisign-подпись
//! с публичным ключом из `tauri.conf.json` и ставит артефакт. Здесь — только
//! обёртка над ним: три команды и события прогресса.
//!
//! Что подпись даёт и чего не даёт: minisign подтверждает, что артефакт не
//! подменили по дороге, и что его собрал держатель приватного ключа. Он **не**
//! Authenticode: без сертификата подписи кода Windows покажет SmartScreen с
//! «неизвестным издателем» и на установке, и на каждом обновлении.
//!
//! Обновление никогда не ставится само. Проверка при старте — тихая, ошибки
//! в ней не всплывают: сеть может быть недоступна, релизов может не быть
//! вовсе, и ни то ни другое не повод дёргать пользователя. Скачивание
//! начинается только по явному нажатию.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

/// Событие прогресса скачивания. `total` приходит из `Content-Length` и
/// может отсутствовать — тогда фронт показывает неопределённый индикатор.
pub const PROGRESS_EVENT: &str = "update-download-progress";

/// Что известно про доступное обновление. `available: false` — актуальная
/// версия; это нормальный ответ, а не ошибка.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateInfo {
    pub available: bool,
    /// Версия, которая стоит сейчас.
    pub current_version: String,
    /// Версия из манифеста. `None`, когда обновления нет.
    pub version: Option<String>,
    /// Дата публикации в том виде, в каком её положили в манифест (RFC 3339).
    pub date: Option<String>,
    /// Текст «что нового». В манифесте это одно поле `notes`; форматирование
    /// (markdown, списки) остаётся на совести релизных заметок.
    pub notes: Option<String>,
}

impl UpdateInfo {
    pub fn none(current_version: impl Into<String>) -> Self {
        Self {
            available: false,
            current_version: current_version.into(),
            version: None,
            date: None,
            notes: None,
        }
    }
}

/// Прогресс скачивания в байтах. Долю считает фронт: ему всё равно нужны
/// сами байты, чтобы показать «12 из 34 МБ».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// Причина, по которой проверку нет смысла даже начинать.
///
/// Обновлять можно только установленную сборку: у `cargo tauri dev` нет
/// установщика, который апдейтер мог бы заменить, — плагин на такой запрос
/// вернёт ошибку, и показывать её пользователю бессмысленно.
pub fn unsupported_reason() -> Option<&'static str> {
    cfg!(debug_assertions).then_some("обновления работают только в собранном приложении")
}

/// Спросить сервер про обновление.
pub async fn check(app: &AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    if let Some(reason) = unsupported_reason() {
        log::debug!("проверка обновлений пропущена: {reason}");
        return Ok(UpdateInfo::none(current));
    }
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(UpdateInfo {
            available: true,
            current_version: current,
            version: Some(update.version.clone()),
            date: update.date.map(|d| d.to_string()),
            notes: update.body.clone().filter(|s| !s.trim().is_empty()),
        }),
        Ok(None) => Ok(UpdateInfo::none(current)),
        Err(e) => Err(e.to_string()),
    }
}

/// Скачать и поставить обновление, затем перезапустить приложение.
///
/// Проверка выполняется заново, а не берётся из результата [`check`]:
/// `Update` из плагина не переживает границу IPC, и повторный запрос всё
/// равно дешевле, чем держать его в состоянии между двумя командами.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    if let Some(reason) = unsupported_reason() {
        return Err(reason.to_string());
    }
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "обновление больше недоступно".to_string())?;

    let app_for_progress = app.clone();
    let mut downloaded: u64 = 0;
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ =
                    app_for_progress.emit(PROGRESS_EVENT, DownloadProgress { downloaded, total });
            },
            || log::info!("обновление скачано, ставим"),
        )
        .await
        .map_err(|e| e.to_string())?;

    // На Windows installMode = passive: инсталлятор уже запущен и попросит
    // закрыть приложение. Выходим сами, чтобы он не ждал.
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_update_reports_current_version() {
        let info = UpdateInfo::none("0.1.1");
        assert!(!info.available);
        assert_eq!(info.current_version, "0.1.1");
        assert_eq!(info.version, None);
        assert_eq!(info.notes, None);
    }

    #[test]
    fn progress_keeps_an_unknown_total_unknown() {
        // Сервер без Content-Length — законный случай: фронт показывает
        // неопределённый индикатор, а не 0 %.
        let json = serde_json::to_value(DownloadProgress {
            downloaded: 5,
            total: None,
        })
        .unwrap();
        assert_eq!(json["downloaded"], 5);
        assert!(json["total"].is_null());
    }

    #[test]
    fn dev_builds_never_check() {
        // Тесты идут в debug, так что причина обязана быть.
        assert_eq!(
            unsupported_reason(),
            Some("обновления работают только в собранном приложении")
        );
    }

    #[test]
    fn info_serializes_with_the_shape_the_frontend_expects() {
        let json = serde_json::to_value(UpdateInfo {
            available: true,
            current_version: "0.1.1".into(),
            version: Some("0.2.0".into()),
            date: Some("2026-08-15T10:00:00Z".into()),
            notes: Some("Светлая тема".into()),
        })
        .unwrap();
        assert_eq!(json["available"], true);
        assert_eq!(json["current_version"], "0.1.1");
        assert_eq!(json["version"], "0.2.0");
        assert_eq!(json["notes"], "Светлая тема");
    }
}
