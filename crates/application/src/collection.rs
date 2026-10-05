use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use chrono::Utc;
use futures::{StreamExt, stream::FuturesUnordered};
use tokio_util::sync::CancellationToken;
use twogis_domain::{
    AppError, CollectionJobState, ErrorKind, ProgressPhase, ProviderRunState, RunSummary,
    ScrapeProgress, SearchRequest, SourceKind,
};
use twogis_provider_core::{ProgressSink, ProviderControl};
use uuid::Uuid;

use crate::ApplicationService;

impl ApplicationService {
    pub async fn resume_search(
        &self,
        job_id: &str,
        progress: ProgressSink,
        cancel: CancellationToken,
    ) -> Result<RunSummary, AppError> {
        let request = self.store.collection_job_request(job_id).await?;
        self.run_collection(request, progress, cancel, Some(job_id.to_owned()))
            .await
    }

    pub async fn pause_provider(&self, source: SourceKind) -> bool {
        let controls = self.controls.read().await;
        if let Some(control) = controls.get(&source) {
            control.pause();
            return true;
        }
        false
    }

    pub async fn resume_provider(&self, source: SourceKind) -> bool {
        let controls = self.controls.read().await;
        if let Some(control) = controls.get(&source) {
            control.resume();
            return true;
        }
        false
    }

    pub(crate) async fn run_collection(
        &self,
        request: SearchRequest,
        progress: ProgressSink,
        cancel: CancellationToken,
        resume_job_id: Option<String>,
    ) -> Result<RunSummary, AppError> {
        request.validate()?;
        let now = Utc::now().to_rfc3339();
        let is_resume = resume_job_id.is_some();
        let job_id = resume_job_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let started_at = if is_resume {
            self.store.collection_job_started_at(&job_id).await?
        } else {
            now.clone()
        };
        let regions = request.effective_regions();
        let sources = request
            .sources
            .iter()
            .copied()
            .filter(|source| request.config_for(*source).enabled)
            .collect::<Vec<_>>();
        if sources.is_empty() {
            return Err(AppError::validation("Select at least one enabled source"));
        }
        let total_targets =
            u32::try_from(regions.len().saturating_mul(sources.len())).unwrap_or(u32::MAX);
        self.store
            .create_or_resume_job(&job_id, &request, &started_at, &now, total_targets)
            .await?;
        let completed_initial = self.store.completed_job_targets(&job_id).await?;
        let completed_targets = Arc::new(AtomicU32::new(completed_initial));
        (progress)(ScrapeProgress {
            phase: ProgressPhase::Discovering,
            current: completed_initial,
            total: Some(total_targets),
            message: format!("Большой сбор · {completed_initial}/{total_targets} целей"),
            source: None,
            region: None,
            state: None,
            retry_after_seconds: None,
        });

        let mut warnings = Vec::<String>::new();
        let pending = FuturesUnordered::new();

        for source in sources.iter().copied() {
            let Some(provider) = self
                .providers
                .iter()
                .find(|provider| provider.source() == source)
                .cloned()
            else {
                warnings.push(format!("{}: provider is not available", source.label()));
                continue;
            };

            let control = ProviderControl::new(cancel.child_token());
            self.controls.write().await.insert(source, control.clone());

            let request = request.clone();
            let regions = regions.clone();
            let progress = progress.clone();
            let store = self.store.clone();
            let job_id = job_id.clone();
            let global_cancel = cancel.clone();
            let completed_targets = completed_targets.clone();
            pending.push(async move {
                let mut source_warnings = Vec::new();
                for region in regions {
                    if global_cancel.is_cancelled() {
                        return Err(AppError::cancelled());
                    }
                    if store.job_target_completed(&job_id, source, &region).await? {
                        continue;
                    }

                    store
                        .update_job_target(
                            &job_id,
                            source,
                            &region,
                            ProviderRunState::Running,
                            0,
                            "Сбор",
                            None,
                        )
                        .await?;
                    (progress)(ScrapeProgress {
                        phase: ProgressPhase::Discovering,
                        current: 0,
                        total: Some(request.config_for(source).max_results),
                        message: format!("{} · {region}", source.label()),
                        source: Some(source),
                        region: Some(region.clone()),
                        state: Some(ProviderRunState::Running),
                        retry_after_seconds: None,
                    });

                    let mut region_request = request.clone();
                    region_request.region = region.clone();
                    region_request.regions.clear();
                    match provider
                        .search(&region_request, progress.clone(), control.clone())
                        .await
                    {
                        Ok(mut output) => {
                            for row in &mut output.organizations {
                                row.attach_source(source);
                            }
                            for chunk in output.organizations.chunks(500) {
                                store.record_source_many(chunk).await?;
                            }
                            store
                                .record_job_source_many(&job_id, &region, &output.organizations)
                                .await?;
                            store
                                .update_job_target(
                                    &job_id,
                                    source,
                                    &region,
                                    ProviderRunState::Completed,
                                    u32::try_from(output.organizations.len()).unwrap_or(u32::MAX),
                                    "Готово",
                                    None,
                                )
                                .await?;
                            (progress)(ScrapeProgress {
                                phase: ProgressPhase::Enriching,
                                current: u32::try_from(output.organizations.len())
                                    .unwrap_or(u32::MAX),
                                total: Some(request.config_for(source).max_results),
                                message: format!("{} · {region}: готово", source.label()),
                                source: Some(source),
                                region: Some(region.clone()),
                                state: Some(ProviderRunState::Completed),
                                retry_after_seconds: None,
                            });
                            let completed = completed_targets
                                .fetch_add(1, Ordering::AcqRel)
                                .saturating_add(1);
                            (progress)(ScrapeProgress {
                                phase: ProgressPhase::Enriching,
                                current: completed,
                                total: Some(total_targets),
                                message: format!(
                                    "Большой сбор · {completed}/{total_targets} целей"
                                ),
                                source: None,
                                region: None,
                                state: None,
                                retry_after_seconds: None,
                            });
                            source_warnings.extend(output.warnings.into_iter().map(|warning| {
                                format!("{} · {region}: {warning}", source.label())
                            }));
                            let inter_region_delay = request
                                .config_for(source)
                                .request_delay_ms
                                .max(provider.policy().min_delay_ms);
                            control
                                .sleep(Duration::from_millis(u64::from(inter_region_delay)))
                                .await?;
                        }
                        Err(err) if matches!(err.kind, ErrorKind::Cancelled) => {
                            if global_cancel.is_cancelled() {
                                return Err(err);
                            }
                            source_warnings
                                .push(format!("{} · {region}: paused/cancelled", source.label()));
                            break;
                        }
                        Err(err) => {
                            let state = provider_state_for_error(err.kind);
                            store
                                .update_job_target(
                                    &job_id,
                                    source,
                                    &region,
                                    state,
                                    0,
                                    &err.message,
                                    err.retry_after_seconds,
                                )
                                .await?;
                            (progress)(ScrapeProgress {
                                phase: ProgressPhase::Discovering,
                                current: 0,
                                total: None,
                                message: err.message.clone(),
                                source: Some(source),
                                region: Some(region.clone()),
                                state: Some(state),
                                retry_after_seconds: err.retry_after_seconds,
                            });
                            source_warnings.push(format!(
                                "{} · {region}: {}",
                                source.label(),
                                err.message
                            ));
                            // A city without a catalog page must not discard the
                            // remaining cities in a regional collection.
                            if err.diagnostics.as_ref().and_then(|value| value.http_status)
                                == Some(404)
                            {
                                let delay = request
                                    .config_for(source)
                                    .request_delay_ms
                                    .max(provider.policy().min_delay_ms);
                                control
                                    .sleep(Duration::from_millis(u64::from(delay)))
                                    .await?;
                                continue;
                            }
                            // A blocked/rate-limited source stops here; other sources continue.
                            break;
                        }
                    }
                }
                Ok::<_, AppError>((source, source_warnings))
            });
        }

        let mut pending = pending;
        while let Some(result) = pending.next().await {
            match result {
                Ok((source, source_warnings)) => {
                    warnings.extend(source_warnings);
                    self.controls.write().await.remove(&source);
                }
                Err(err) if matches!(err.kind, ErrorKind::Cancelled) => {
                    self.store
                        .set_job_state(
                            &job_id,
                            CollectionJobState::Cancelled,
                            &Utc::now().to_rfc3339(),
                            "Сбор отменён",
                        )
                        .await?;
                    self.controls.write().await.clear();
                    return Err(err);
                }
                Err(err) => {
                    self.store
                        .set_job_state(
                            &job_id,
                            CollectionJobState::Failed,
                            &Utc::now().to_rfc3339(),
                            &err.message,
                        )
                        .await?;
                    self.controls.write().await.clear();
                    return Err(err);
                }
            }
        }
        self.controls.write().await.clear();

        if cancel.is_cancelled() {
            self.store
                .set_job_state(
                    &job_id,
                    CollectionJobState::Cancelled,
                    &Utc::now().to_rfc3339(),
                    "Сбор отменён; его можно продолжить позже",
                )
                .await?;
            self.controls.write().await.clear();
            return Err(AppError::cancelled());
        }

        let raw = self.store.load_job_source_rows(&job_id).await?;
        let raw_records = u32::try_from(raw.len()).unwrap_or(u32::MAX);
        (progress)(ScrapeProgress {
            phase: ProgressPhase::Resolving,
            current: raw_records,
            total: Some(raw_records),
            message: "Нормализация и объединение дублей".into(),
            source: None,
            region: None,
            state: None,
            retry_after_seconds: None,
        });
        let mut resolved = twogis_dedupe::deduplicate(raw);
        let organization_count = u32::try_from(resolved.organizations.len()).unwrap_or(u32::MAX);

        (progress)(ScrapeProgress {
            phase: ProgressPhase::Saving,
            current: organization_count,
            total: Some(organization_count),
            message: "Пакетное сохранение лидов в SQLite".into(),
            source: None,
            region: None,
            state: None,
            retry_after_seconds: None,
        });
        self.store
            .upsert_many_batched(&mut resolved.organizations, 500)
            .await?;
        let finished_at = Utc::now().to_rfc3339();
        let summary = RunSummary {
            run_id: job_id.clone(),
            started_at,
            finished_at: finished_at.clone(),
            organizations: resolved.organizations,
            warnings,
            raw_records,
            duplicates_merged: resolved.merged_count,
            organization_count,
        };
        self.store.record_run(&summary, &request).await?;
        let completed = self.store.completed_job_targets(&job_id).await?;
        let state = if completed >= total_targets {
            CollectionJobState::Completed
        } else {
            CollectionJobState::Partial
        };
        self.store
            .set_job_state(
                &job_id,
                state,
                &finished_at,
                if completed >= total_targets {
                    if summary.warnings.is_empty() {
                        "Сбор завершён"
                    } else {
                        "Сбор завершён с предупреждениями"
                    }
                } else {
                    "Сбор завершён частично; незавершённые источники можно продолжить позже"
                },
            )
            .await?;
        if matches!(state, CollectionJobState::Completed) {
            self.store.cleanup_collection_job_records(&job_id).await?;
        }

        (progress)(ScrapeProgress {
            phase: ProgressPhase::Done,
            current: organization_count,
            total: Some(organization_count),
            message: format!("Готово · {organization_count} уникальных лидов"),
            source: None,
            region: None,
            state: None,
            retry_after_seconds: None,
        });

        let mut response = summary;
        response.organizations.truncate(500);
        Ok(response)
    }
}

fn provider_state_for_error(kind: ErrorKind) -> ProviderRunState {
    match kind {
        ErrorKind::RateLimited => ProviderRunState::RateLimited,
        ErrorKind::Blocked => ProviderRunState::Blocked,
        ErrorKind::CaptchaRequired => ProviderRunState::CaptchaRequired,
        ErrorKind::ChallengeRequired => ProviderRunState::ChallengeRequired,
        ErrorKind::Cancelled => ProviderRunState::Cancelled,
        _ => ProviderRunState::Failed,
    }
}
