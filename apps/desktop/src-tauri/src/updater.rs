use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;
use twogis_domain::AppError;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current_version: String,
    pub version: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    downloaded: usize,
    total: Option<u64>,
}

#[tauri::command]
#[specta::specta]
pub async fn check_for_updates(app: AppHandle) -> Result<UpdateInfo, AppError> {
    let current_version = app.package_info().version.to_string();
    let update = app
        .updater()
        .map_err(|error| AppError::network(format!("Не удалось проверить обновления: {error}")))?
        .check()
        .await
        .map_err(|error| AppError::network(format!("Не удалось проверить обновления: {error}")))?;
    Ok(UpdateInfo {
        current_version,
        version: update.as_ref().map(|update| update.version.clone()),
        notes: update.and_then(|update| update.body),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn install_update(app: AppHandle) -> Result<bool, AppError> {
    let update = app
        .updater()
        .map_err(|error| AppError::network(format!("Не удалось проверить обновление: {error}")))?
        .check()
        .await
        .map_err(|error| AppError::network(format!("Не удалось проверить обновление: {error}")))?
        .ok_or_else(|| AppError::validation("Новых обновлений нет"))?;
    let mut downloaded = 0;
    let progress_app = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk;
                let _ = progress_app.emit("update-progress", UpdateProgress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|error| AppError::network(format!("Не удалось установить обновление: {error}")))?;
    app.request_restart();
    Ok(true)
}
