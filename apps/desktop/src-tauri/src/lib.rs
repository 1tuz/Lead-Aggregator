mod commands;
mod credentials;
mod state;
mod updater;

use std::{fs, path::Path, sync::Arc};

#[cfg(debug_assertions)]
use specta_typescript::Typescript;
use tauri::{Manager, Wry};
use tauri_specta::{Builder, collect_commands};
use twogis_application::ApplicationService;
use twogis_domain::AppError;
use twogis_provider::TwoGisHtmlProvider;
use twogis_provider_core::DirectoryProvider;
use twogis_provider_public_catalogs::{RusprofileHtmlProvider, YellHtmlProvider, ZoonHtmlProvider};
use twogis_storage_sqlite::SqliteStore;

use crate::{
    commands::{
        cancel_search, delete_2gis_api_key, export_results, health, pause_provider,
        recent_collection_jobs, recent_results, recent_runs, results_for_run, results_for_run_page,
        resume_provider, resume_search, save_2gis_api_key, start_search, two_gis_api_key_saved,
        two_gis_categories,
    },
    state::AppState,
    updater::{check_for_updates, install_update},
};

fn specta_builder() -> Builder<Wry> {
    Builder::<Wry>::new().commands(collect_commands![
        start_search,
        resume_search,
        pause_provider,
        resume_provider,
        cancel_search,
        recent_results,
        recent_runs,
        recent_collection_jobs,
        results_for_run,
        results_for_run_page,
        export_results,
        health,
        save_2gis_api_key,
        delete_2gis_api_key,
        two_gis_api_key_saved,
        two_gis_categories,
        check_for_updates,
        install_update
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let builder = specta_builder();

    #[cfg(debug_assertions)]
    builder.export(Typescript::default(), "../src/bindings.ts")?;

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            let handle = app.handle().clone();
            let app_data = handle.path().app_data_dir()?;
            fs::create_dir_all(&app_data)?;
            let db_path = app_data.join("lead-aggregator.db");
            migrate_legacy_database(&app_data, &db_path)?;

            let api_key = credentials::load_2gis_api_key().unwrap_or_default();
            let api_key_state = Arc::new(std::sync::RwLock::new(api_key));

            let two_gis_provider = Arc::new(
                TwoGisHtmlProvider::new()
                    .map_err(as_setup_error)?
                    .with_api_key_state(api_key_state.clone()),
            );
            let providers: Vec<Arc<dyn DirectoryProvider>> = vec![
                two_gis_provider.clone(),
                Arc::new(YellHtmlProvider::new().map_err(as_setup_error)?),
                Arc::new(ZoonHtmlProvider::new().map_err(as_setup_error)?),
                Arc::new(RusprofileHtmlProvider::new().map_err(as_setup_error)?),
            ];
            let store = tauri::async_runtime::block_on(SqliteStore::connect(&db_path))
                .map_err(as_setup_error)?;
            let service = Arc::new(ApplicationService::new(providers, store));
            app.manage(AppState {
                app: handle,
                service,
                two_gis_provider,
                cancellation: tokio::sync::Mutex::new(None),
                api_key_state,
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}

fn migrate_legacy_database(app_data: &Path, db_path: &Path) -> Result<(), std::io::Error> {
    if db_path.exists() {
        return Ok(());
    }

    let mut candidates = vec![app_data.join("twogis-extractor.db")];
    if let Some(parent) = app_data.parent() {
        candidates.push(
            parent
                .join("dev.local.twogis-extractor")
                .join("twogis-extractor.db"),
        );
    }
    if let Some(source) = candidates.into_iter().find(|candidate| candidate.exists()) {
        // Copy instead of rename so downgrading to an older build remains safe.
        fs::copy(source, db_path)?;
    }
    Ok(())
}

fn as_setup_error(error: AppError) -> Box<dyn std::error::Error> {
    Box::<dyn std::error::Error>::from(error.to_string())
}

#[allow(dead_code)]
fn _assert_error_is_serializable(_: AppError) {}
