use std::sync::Arc;

use tauri::AppHandle;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use twogis_application::ApplicationService;

pub struct AppState {
    pub app: AppHandle,
    pub service: Arc<ApplicationService>,
    pub cancellation: Mutex<Option<CancellationToken>>,
}
