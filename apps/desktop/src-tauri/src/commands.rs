use std::sync::Arc;

use chrono::Utc;
use tauri::{Emitter, Manager, State};
use tokio_util::sync::CancellationToken;
use twogis_domain::{
    AppError, CollectionJobInfo, ExportFormat, ExportReceipt, HealthInfo, Organization,
    RunResultsPage, RunSummary, ScrapeProgress, SearchRequest, SearchRunInfo, SourceKind,
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
pub async fn resume_search(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<RunSummary, AppError> {
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
        .resume_search(&job_id, progress, token.clone())
        .await;
    let mut guard = state.cancellation.lock().await;
    if guard.as_ref().is_some_and(|active| active == &token) {
        *guard = None;
    }
    result
}

#[tauri::command]
#[specta::specta]
pub async fn pause_provider(
    state: State<'_, AppState>,
    source: SourceKind,
) -> Result<bool, AppError> {
    Ok(state.service.pause_provider(source).await)
}

#[tauri::command]
#[specta::specta]
pub async fn resume_provider(
    state: State<'_, AppState>,
    source: SourceKind,
) -> Result<bool, AppError> {
    Ok(state.service.resume_provider(source).await)
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
pub async fn recent_runs(
    state: State<'_, AppState>,
    limit: u32,
) -> Result<Vec<SearchRunInfo>, AppError> {
    state.service.recent_runs(limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn results_for_run(
    state: State<'_, AppState>,
    run_id: String,
    limit: u32,
) -> Result<Vec<Organization>, AppError> {
    state.service.results_for_run(&run_id, limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn recent_collection_jobs(
    state: State<'_, AppState>,
    limit: u32,
) -> Result<Vec<CollectionJobInfo>, AppError> {
    state.service.recent_collection_jobs(limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn results_for_run_page(
    state: State<'_, AppState>,
    run_id: String,
    offset: u32,
    limit: u32,
) -> Result<RunResultsPage, AppError> {
    state
        .service
        .results_for_run_page(&run_id, offset, limit)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn export_results(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    run_id: String,
    format: ExportFormat,
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
    let rows = state.service.export_run(&path, format, &run_id).await?;
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
