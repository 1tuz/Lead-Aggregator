mod collection;

use std::{collections::HashMap, path::Path, sync::Arc};

use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use twogis_domain::{
    AppError, CollectionJobInfo, ExportFormat, Organization, RunResultsPage, RunSummary,
    SearchRequest, SearchRunInfo, SourceKind,
};
use twogis_provider_core::{DirectoryProvider, ProgressSink, ProviderControl};
use twogis_storage_sqlite::SqliteStore;

#[derive(Clone)]
pub struct ApplicationService {
    providers: Vec<Arc<dyn DirectoryProvider>>,
    store: SqliteStore,
    controls: Arc<RwLock<HashMap<SourceKind, ProviderControl>>>,
}

impl ApplicationService {
    pub fn new(providers: Vec<Arc<dyn DirectoryProvider>>, store: SqliteStore) -> Self {
        Self {
            providers,
            store,
            controls: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn provider_id(&self) -> String {
        self.providers
            .iter()
            .map(|provider| provider.id())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn available_sources(&self) -> Vec<SourceKind> {
        self.providers
            .iter()
            .map(|provider| provider.source())
            .collect()
    }

    pub async fn run_search(
        &self,
        request: SearchRequest,
        progress: ProgressSink,
        cancel: CancellationToken,
    ) -> Result<RunSummary, AppError> {
        self.run_collection(request, progress, cancel, None).await
    }

    pub async fn recent_results(&self, limit: u32) -> Result<Vec<Organization>, AppError> {
        self.store.recent(limit).await
    }

    pub async fn recent_runs(&self, limit: u32) -> Result<Vec<SearchRunInfo>, AppError> {
        self.store.recent_runs(limit).await
    }

    pub async fn results_for_run(
        &self,
        run_id: &str,
        limit: u32,
    ) -> Result<Vec<Organization>, AppError> {
        self.store.results_for_run(run_id, limit).await
    }

    pub async fn results_for_run_page(
        &self,
        run_id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<RunResultsPage, AppError> {
        self.store.results_for_run_page(run_id, offset, limit).await
    }

    pub async fn recent_collection_jobs(
        &self,
        limit: u32,
    ) -> Result<Vec<CollectionJobInfo>, AppError> {
        self.store.recent_collection_jobs(limit).await
    }

    pub async fn export_run(
        &self,
        path: &Path,
        format: ExportFormat,
        run_id: &str,
    ) -> Result<u32, AppError> {
        let rows = self.store.all_results_for_run(run_id).await?;
        twogis_export::export(path, format, &rows)?;
        Ok(rows.len() as u32)
    }
}
