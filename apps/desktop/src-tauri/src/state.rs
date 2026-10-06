use std::sync::{Arc, RwLock};

use tauri::AppHandle;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use twogis_application::ApplicationService;
use twogis_provider::parselab::ParselabProvider;

pub struct AppState {
    pub app: AppHandle,
    pub service: Arc<ApplicationService>,
    pub parselab_provider: Arc<ParselabProvider>,
    pub cancellation: Mutex<Option<CancellationToken>>,
    pub license_key_state: Arc<RwLock<Option<String>>>,
}
