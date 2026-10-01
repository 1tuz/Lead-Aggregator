use std::{path::Path, sync::Arc};

use chrono::Utc;
use tokio_util::sync::CancellationToken;
use twogis_domain::{
    AppError, ErrorKind, ExportFormat, Organization, ProgressPhase, RunSummary, ScrapeProgress,
    SearchRequest, SourceKind,
};
use twogis_provider_core::{DirectoryProvider, ProgressSink};
use twogis_storage_sqlite::SqliteStore;
use uuid::Uuid;

#[derive(Clone)]
pub struct ApplicationService {
    providers: Vec<Arc<dyn DirectoryProvider>>,
    store: SqliteStore,
}

impl ApplicationService {
    pub fn new(providers: Vec<Arc<dyn DirectoryProvider>>, store: SqliteStore) -> Self {
        Self { providers, store }
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
        request.validate()?;
        let started_at = Utc::now().to_rfc3339();
        let run_id = Uuid::new_v4().to_string();
        let mut raw = Vec::<Organization>::new();
        let mut warnings = Vec::<String>::new();

        for source in request.sources.iter().copied() {
            if cancel.is_cancelled() {
                return Err(AppError::cancelled());
            }
            let Some(provider) = self
                .providers
                .iter()
                .find(|provider| provider.source() == source)
            else {
                warnings.push(format!(
                    "{}: provider is not available in this build",
                    source.label()
                ));
                continue;
            };

            match provider
                .search(&request, progress.clone(), cancel.clone())
                .await
            {
                Ok(mut output) => {
                    for row in &mut output.organizations {
                        row.attach_source(source);
                    }
                    self.store.record_source_many(&output.organizations).await?;
                    raw.append(&mut output.organizations);
                    warnings.extend(
                        output
                            .warnings
                            .into_iter()
                            .map(|warning| format!("{}: {warning}", source.label())),
                    );
                }
                Err(err) if matches!(err.kind, ErrorKind::Cancelled) => return Err(err),
                Err(err) => warnings.push(format!("{}: {}", source.label(), err.message)),
            }
        }

        if cancel.is_cancelled() {
            return Err(AppError::cancelled());
        }

        (progress)(ScrapeProgress {
            phase: ProgressPhase::Resolving,
            current: raw.len() as u32,
            total: Some(raw.len() as u32),
            message: "Нормализация и объединение дублей".into(),
        });
        let raw_records = raw.len() as u32;
        let resolved = twogis_dedupe::deduplicate(raw);

        (progress)(ScrapeProgress {
            phase: ProgressPhase::Saving,
            current: resolved.organizations.len() as u32,
            total: Some(resolved.organizations.len() as u32),
            message: "Сохранение объединённых лидов в SQLite".into(),
        });
        self.store.upsert_many(&resolved.organizations).await?;
        let finished_at = Utc::now().to_rfc3339();
        (progress)(ScrapeProgress {
            phase: ProgressPhase::Done,
            current: resolved.organizations.len() as u32,
            total: Some(resolved.organizations.len() as u32),
            message: "Готово".into(),
        });

        Ok(RunSummary {
            run_id,
            started_at,
            finished_at,
            organizations: resolved.organizations,
            warnings,
            raw_records,
            duplicates_merged: resolved.merged_count,
        })
    }

    pub async fn recent_results(&self, limit: u32) -> Result<Vec<Organization>, AppError> {
        self.store.recent(limit).await
    }

    pub async fn export_recent(
        &self,
        path: &Path,
        format: ExportFormat,
        limit: u32,
    ) -> Result<u32, AppError> {
        let rows = self.store.recent(limit).await?;
        twogis_export::export(path, format, &rows)?;
        Ok(rows.len() as u32)
    }
}
