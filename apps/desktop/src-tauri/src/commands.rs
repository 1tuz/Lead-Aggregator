use std::sync::Arc;

use chrono::Utc;
use tauri::{Emitter, Manager, State};
use tokio_util::sync::CancellationToken;
use twogis_domain::{
    AppError, ExportFormat, ExportReceipt, HealthInfo, Organization, RunSummary, ScrapeProgress,
    SearchRequest,
};
use twogis_provider_core::ProgressSink;

use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn start_search(
    state: State<'_, AppState>,
    request: SearchRequest,
) -> Result<RunSummary, AppError> {
    request.validate()?;
    let token = CancellationToken::new();
    {
        let mut guard = state.cancellation.lock().await;
        if let Some(previous) = guard.replace(token.clone()) {
            previous.cancel();
        }
    }

    let app = state.app.clone();
    let progress: ProgressSink = Arc::new(move |payload: ScrapeProgress| {
        let _ = app.emit("scrape-progress", payload);
    });
    let result = state
        .service
        .run_search(request, progress, token.clone())
        .await;

    let mut guard = state.cancellation.lock().await;
    if guard.as_ref().is_some_and(|active| active == &token) {
        *guard = None;
    }
    result
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_search(state: State<'_, AppState>) -> Result<bool, AppError> {
    let guard = state.cancellation.lock().await;
    if let Some(token) = guard.as_ref() {
        token.cancel();
        return Ok(true);
    }
    Ok(false)
}

#[tauri::command]
#[specta::specta]
pub async fn recent_results(
    state: State<'_, AppState>,
    limit: u32,
) -> Result<Vec<Organization>, AppError> {
    state.service.recent_results(limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn export_results(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    format: ExportFormat,
    limit: u32,
) -> Result<ExportReceipt, AppError> {
    let dir = app
        .path()
        .download_dir()
        .map_err(|e| AppError::export(format!("failed to resolve Downloads directory: {e}")))?;
    let extension = match format {
        ExportFormat::Csv => "csv",
        ExportFormat::Json => "json",
        ExportFormat::Xlsx => "xlsx",
    };
    let filename = format!(
        "lead-export-{}.{}",
        Utc::now().format("%Y%m%d-%H%M%S"),
        extension
    );
    let path = dir.join(filename);
    let rows = state.service.export_recent(&path, format, limit).await?;
    Ok(ExportReceipt {
        path: path.to_string_lossy().into_owned(),
        rows,
        format,
    })
}

#[tauri::command]
#[specta::specta]
pub fn health(state: State<'_, AppState>) -> HealthInfo {
    HealthInfo {
        app_version: env!("CARGO_PKG_VERSION").into(),
        provider: state.service.provider_id(),
        browser_engine: "none (Rust HTTP; Tauri UI uses system WebKit)".into(),
        api_key_required: false,
        available_sources: state.service.available_sources(),
    }
}
