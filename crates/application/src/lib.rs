use std::{path::Path, sync::Arc};

use chrono::Utc;
use futures::{StreamExt, stream};
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

fn assign_stable_ids(rows: &mut [Organization]) {
    for row in rows {
        let fingerprint = if row.dedupe.fingerprint.is_empty() {
            format!("source:{}", row.id)
        } else {
            row.dedupe.fingerprint.clone()
        };
        row.id = stable_organization_id(&fingerprint);
    }
}

fn stable_organization_id(fingerprint: &str) -> String {
    const OFFSET: u128 = 0x6c62272e07bb014262b821756295c58d;
    const PRIME: u128 = 0x0000000001000000000000000000013b;
    let mut hash = OFFSET;
    for byte in fingerprint.as_bytes() {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("org-{hash:032x}")
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

        let mut selected = Vec::new();
        for source in request.sources.iter().copied() {
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
            selected.push((source, provider.clone()));
        }

        let provider_count = selected.len().max(1);
        let request_for_stream = request.clone();
        let progress_for_stream = progress.clone();
        let cancel_for_stream = cancel.clone();
        let outputs = stream::iter(selected)
            .map(move |(source, provider)| {
                let request = request_for_stream.clone();
                let progress = progress_for_stream.clone();
                let cancel = cancel_for_stream.clone();
                async move {
                    let result = provider.search(&request, progress, cancel).await;
                    (source, result)
                }
            })
            .buffer_unordered(provider_count)
            .collect::<Vec<_>>()
            .await;

        for (source, result) in outputs {
            match result {
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
        let mut resolved = twogis_dedupe::deduplicate(raw);
        assign_stable_ids(&mut resolved.organizations);

        (progress)(ScrapeProgress {
            phase: ProgressPhase::Saving,
            current: resolved.organizations.len() as u32,
            total: Some(resolved.organizations.len() as u32),
            message: "Сохранение объединённых лидов в SQLite".into(),
        });
        self.store.upsert_many(&resolved.organizations).await?;
        let finished_at = Utc::now().to_rfc3339();
        let summary = RunSummary {
            run_id,
            started_at,
            finished_at,
            organizations: resolved.organizations,
            warnings,
            raw_records,
            duplicates_merged: resolved.merged_count,
        };
        self.store.record_run(&summary, &request).await?;

        (progress)(ScrapeProgress {
            phase: ProgressPhase::Done,
            current: summary.organizations.len() as u32,
            total: Some(summary.organizations.len() as u32),
            message: "Готово".into(),
        });

        Ok(summary)
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
