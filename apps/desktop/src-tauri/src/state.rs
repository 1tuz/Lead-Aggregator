use std::sync::{Arc, RwLock};

use tauri::AppHandle;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use twogis_application::ApplicationService;

pub struct AppState {
    pub app: AppHandle,
    pub service: Arc<ApplicationService>,
    pub cancellation: Mutex<Option<CancellationToken>>,
    pub api_key_state: Arc<RwLock<Option<String>>>,
}
