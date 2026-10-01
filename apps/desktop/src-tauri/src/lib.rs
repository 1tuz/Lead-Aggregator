mod commands;
mod state;

use std::{fs, sync::Arc};

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
    commands::{cancel_search, export_results, health, recent_results, start_search},
    state::AppState,
};

fn specta_builder() -> Builder<Wry> {
    Builder::<Wry>::new().commands(collect_commands![
        start_search,
        cancel_search,
        recent_results,
        export_results,
        health
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let builder = specta_builder();

    #[cfg(debug_assertions)]
    builder.export(Typescript::default(), "../src/bindings.ts")?;

    tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            let handle = app.handle().clone();
            let app_data = handle.path().app_data_dir()?;
            fs::create_dir_all(&app_data)?;
            let db_path = app_data.join("twogis-extractor.db");

            let providers: Vec<Arc<dyn DirectoryProvider>> = vec![
                Arc::new(TwoGisHtmlProvider::new().map_err(as_setup_error)?),
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
                cancellation: tokio::sync::Mutex::new(None),
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}

fn as_setup_error(error: AppError) -> Box<dyn std::error::Error> {
    Box::<dyn std::error::Error>::from(error.to_string())
}

#[allow(dead_code)]
fn _assert_error_is_serializable(_: AppError) {}
