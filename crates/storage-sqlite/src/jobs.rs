use sqlx::Row;
use twogis_domain::{
    AppError, CollectionJobInfo, CollectionJobState, MAX_RESULT_PAGE_SIZE, Organization,
    ProviderRunState, RunResultsPage, SearchRequest, SourceKind,
};

use super::{SqliteStore, decode_row, encode, storage_error};

impl SqliteStore {
    pub(crate) async fn migrate_collection_jobs(&self) -> Result<(), AppError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS collection_jobs (
              job_id TEXT PRIMARY KEY,
              request_json TEXT NOT NULL,
              state TEXT NOT NULL,
              started_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              message TEXT NOT NULL,
              total_targets INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::storage(format!("failed to create collection_jobs: {e}")))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS collection_job_targets (
              job_id TEXT NOT NULL,
              source TEXT NOT NULL,
              region TEXT NOT NULL,
              state TEXT NOT NULL,
              collected INTEGER NOT NULL DEFAULT 0,
              message TEXT NOT NULL DEFAULT '',
              retry_after_seconds INTEGER,
              PRIMARY KEY (job_id, source, region),
              FOREIGN KEY (job_id) REFERENCES collection_jobs(job_id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::storage(format!("failed to create collection_job_targets: {e}")))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS collection_job_records (
              job_id TEXT NOT NULL,
              source TEXT NOT NULL,
              region TEXT NOT NULL,
              source_id TEXT NOT NULL,
              source_url TEXT NOT NULL,
              payload_json TEXT NOT NULL,
              PRIMARY KEY (job_id, source, region, source_id, source_url),
              FOREIGN KEY (job_id) REFERENCES collection_jobs(job_id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::storage(format!("failed to create collection_job_records: {e}")))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_collection_job_records_job ON collection_job_records(job_id)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::storage(format!("failed to index collection records: {e}")))?;

        // A process cannot still be actively collecting after a fresh app start.
        sqlx::query(
            "UPDATE collection_jobs SET state='interrupted', message='Приложение было закрыто во время сбора' WHERE state='running'",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::storage(format!("failed to mark stale jobs interrupted: {e}")))?;
        Ok(())
    }

    pub async fn create_or_resume_job(
        &self,
        job_id: &str,
        request: &SearchRequest,
        started_at: &str,
        updated_at: &str,
        total_targets: u32,
    ) -> Result<(), AppError> {
        let request_json = encode(request, "collection request")?;
        sqlx::query(
            r#"
            INSERT INTO collection_jobs (
              job_id, request_json, state, started_at, updated_at, message, total_targets
            ) VALUES (?, ?, 'running', ?, ?, 'Сбор запущен', ?)
            ON CONFLICT(job_id) DO UPDATE SET
              request_json=excluded.request_json,
              state='running',
              updated_at=excluded.updated_at,
              message='Сбор возобновлён',
              total_targets=excluded.total_targets
            "#,
        )
        .bind(job_id)
        .bind(request_json)
        .bind(started_at)
        .bind(updated_at)
        .bind(i64::from(total_targets))
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::storage(format!("failed to persist collection job: {e}")))?;
        Ok(())
    }

    pub async fn set_job_state(
        &self,
        job_id: &str,
        state: CollectionJobState,
        updated_at: &str,
        message: &str,
    ) -> Result<(), AppError> {
        sqlx::query("UPDATE collection_jobs SET state=?, updated_at=?, message=? WHERE job_id=?")
            .bind(state.as_str())
            .bind(updated_at)
            .bind(message)
            .bind(job_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::storage(format!("failed to update collection job: {e}")))?;
        Ok(())
    }

    pub async fn update_job_target(
        &self,
        job_id: &str,
        source: SourceKind,
        region: &str,
        state: ProviderRunState,
        collected: u32,
        message: &str,
        retry_after_seconds: Option<u64>,
    ) -> Result<(), AppError> {
        sqlx::query(
            r#"
            INSERT INTO collection_job_targets (
              job_id, source, region, state, collected, message, retry_after_seconds
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(job_id, source, region) DO UPDATE SET
              state=excluded.state,
              collected=excluded.collected,
              message=excluded.message,
              retry_after_seconds=excluded.retry_after_seconds
            "#,
        )
        .bind(job_id)
        .bind(source.id())
        .bind(region)
        .bind(provider_state_id(state))
        .bind(i64::from(collected))
        .bind(message)
        .bind(retry_after_seconds.and_then(|value| i64::try_from(value).ok()))
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::storage(format!("failed to update collection target: {e}")))?;
        Ok(())
    }

    pub async fn completed_job_targets(&self, job_id: &str) -> Result<u32, AppError> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM collection_job_targets WHERE job_id=? AND state='completed'",
        )
        .bind(job_id)
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    pub async fn job_target_completed(
        &self,
        job_id: &str,
        source: SourceKind,
        region: &str,
    ) -> Result<bool, AppError> {
        let state = sqlx::query_scalar::<_, String>(
            "SELECT state FROM collection_job_targets WHERE job_id=? AND source=? AND region=?",
        )
        .bind(job_id)
        .bind(source.id())
        .bind(region)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(state.as_deref() == Some("completed"))
    }

    pub async fn record_job_source_many(
        &self,
        job_id: &str,
        region: &str,
        rows: &[Organization],
    ) -> Result<(), AppError> {
        for chunk in rows.chunks(500) {
            let mut tx = self.pool.begin().await.map_err(storage_error)?;
            for row in chunk {
                let payload = encode(row, "collection source row")?;
                for source in &row.sources {
                    sqlx::query(
                        r#"
                        INSERT INTO collection_job_records (
                          job_id, source, region, source_id, source_url, payload_json
                        ) VALUES (?, ?, ?, ?, ?, ?)
                        ON CONFLICT(job_id, source, region, source_id, source_url) DO UPDATE SET
                          payload_json=excluded.payload_json
                        "#,
                    )
                    .bind(job_id)
                    .bind(source.source.id())
                    .bind(region)
                    .bind(&source.source_id)
                    .bind(&source.source_url)
                    .bind(&payload)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| {
                        AppError::storage(format!("failed to persist collection source row: {e}"))
                    })?;
                }
            }
            tx.commit().await.map_err(storage_error)?;
        }
        Ok(())
    }

    pub async fn load_job_source_rows(&self, job_id: &str) -> Result<Vec<Organization>, AppError> {
        let payloads = sqlx::query_scalar::<_, String>(
            "SELECT payload_json FROM collection_job_records WHERE job_id=? ORDER BY rowid",
        )
        .bind(job_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::storage(format!("failed to load collection source rows: {e}")))?;
        payloads
            .into_iter()
            .map(|payload| {
                serde_json::from_str(&payload).map_err(|e| {
                    AppError::storage(format!("failed to decode collection source row: {e}"))
                })
            })
            .collect()
    }

    pub async fn upsert_many_batched(
        &self,
        rows: &mut [Organization],
        batch_size: usize,
    ) -> Result<(), AppError> {
        for chunk in rows.chunks_mut(batch_size.clamp(50, 2_000)) {
            self.upsert_many(chunk).await?;
        }
        Ok(())
    }

    pub async fn cleanup_collection_job_records(&self, job_id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM collection_job_records WHERE job_id=?")
            .bind(job_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::storage(format!("failed to clean collection job rows: {e}")))?;
        Ok(())
    }

    pub async fn collection_job_started_at(&self, job_id: &str) -> Result<String, AppError> {
        sqlx::query_scalar::<_, String>("SELECT started_at FROM collection_jobs WHERE job_id=?")
            .bind(job_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(storage_error)?
            .ok_or_else(|| AppError::storage(format!("collection job {job_id} not found")))
    }

    pub async fn collection_job_request(&self, job_id: &str) -> Result<SearchRequest, AppError> {
        let request_json = sqlx::query_scalar::<_, String>(
            "SELECT request_json FROM collection_jobs WHERE job_id=?",
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?
        .ok_or_else(|| AppError::storage(format!("collection job {job_id} not found")))?;
        serde_json::from_str(&request_json)
            .map_err(|e| AppError::storage(format!("failed to decode collection request: {e}")))
    }

    pub async fn recent_collection_jobs(
        &self,
        limit: u32,
    ) -> Result<Vec<CollectionJobInfo>, AppError> {
        let rows = sqlx::query(
            r#"
            SELECT j.job_id, j.request_json, j.state, j.started_at, j.updated_at,
                   j.message, j.total_targets,
                   (SELECT COUNT(*) FROM collection_job_targets t
                    WHERE t.job_id=j.job_id AND t.state='completed') AS completed_targets
            FROM collection_jobs j
            ORDER BY j.updated_at DESC
            LIMIT ?
            "#,
        )
        .bind(i64::from(limit.clamp(1, 100)))
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;

        rows.into_iter()
            .map(|row| {
                let request_json: String = row.try_get("request_json").map_err(storage_error)?;
                Ok(CollectionJobInfo {
                    job_id: row.try_get("job_id").map_err(storage_error)?,
                    state: parse_job_state(
                        &row.try_get::<String, _>("state").map_err(storage_error)?,
                    ),
                    request: serde_json::from_str(&request_json).map_err(|e| {
                        AppError::storage(format!("failed to decode collection job request: {e}"))
                    })?,
                    started_at: row.try_get("started_at").map_err(storage_error)?,
                    updated_at: row.try_get("updated_at").map_err(storage_error)?,
                    message: row.try_get("message").map_err(storage_error)?,
                    completed_targets: u32::try_from(
                        row.try_get::<i64, _>("completed_targets")
                            .map_err(storage_error)?,
                    )
                    .unwrap_or(u32::MAX),
                    total_targets: u32::try_from(
                        row.try_get::<i64, _>("total_targets")
                            .map_err(storage_error)?,
                    )
                    .unwrap_or(u32::MAX),
                })
            })
            .collect()
    }

    pub async fn results_for_run_page(
        &self,
        run_id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<RunResultsPage, AppError> {
        let limit = limit.clamp(1, MAX_RESULT_PAGE_SIZE);
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM run_results WHERE run_id=?")
            .bind(run_id)
            .fetch_one(&self.pool)
            .await
            .map_err(storage_error)?;
        let items = if total > 0 {
            let payloads = sqlx::query_scalar::<_, String>(
                r#"
                SELECT payload_json FROM run_results
                WHERE run_id=? ORDER BY position ASC LIMIT ? OFFSET ?
                "#,
            )
            .bind(run_id)
            .bind(i64::from(limit))
            .bind(i64::from(offset))
            .fetch_all(&self.pool)
            .await
            .map_err(storage_error)?;
            payloads
                .into_iter()
                .map(|payload| {
                    serde_json::from_str(&payload)
                        .map_err(|e| AppError::storage(format!("failed to decode run result: {e}")))
                })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            let rows = sqlx::query(
                r#"
                SELECT o.id, o.name, o.category, o.address, o.rating, o.review_count,
                       o.phones_json, o.email, o.website, o.socials_json, o.opening_status,
                       o.latitude, o.longitude, o.source_url, o.collected_at, o.inn, o.ogrn,
                       o.sources_json, o.tags_json, o.branches_json, o.dedupe_json
                FROM run_organizations r
                JOIN organizations o ON o.id=r.organization_id
                WHERE r.run_id=? ORDER BY o.name ASC LIMIT ? OFFSET ?
                "#,
            )
            .bind(run_id)
            .bind(i64::from(limit))
            .bind(i64::from(offset))
            .fetch_all(&self.pool)
            .await
            .map_err(storage_error)?;
            rows.into_iter()
                .map(decode_row)
                .collect::<Result<Vec<_>, _>>()?
        };
        let legacy_total = if total == 0 {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM run_organizations WHERE run_id=?")
                .bind(run_id)
                .fetch_one(&self.pool)
                .await
                .map_err(storage_error)?
        } else {
            total
        };
        Ok(RunResultsPage {
            run_id: run_id.to_owned(),
            items,
            offset,
            limit,
            total: u32::try_from(legacy_total).unwrap_or(u32::MAX),
        })
    }

    pub async fn all_results_for_run(&self, run_id: &str) -> Result<Vec<Organization>, AppError> {
        let payloads = sqlx::query_scalar::<_, String>(
            "SELECT payload_json FROM run_results WHERE run_id=? ORDER BY position ASC",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        if !payloads.is_empty() {
            return payloads
                .into_iter()
                .map(|payload| {
                    serde_json::from_str(&payload)
                        .map_err(|e| AppError::storage(format!("failed to decode run result: {e}")))
                })
                .collect();
        }
        let mut rows = Vec::new();
        let mut offset = 0_u32;
        loop {
            let page = self
                .results_for_run_page(run_id, offset, MAX_RESULT_PAGE_SIZE)
                .await?;
            let loaded = u32::try_from(page.items.len()).unwrap_or(u32::MAX);
            rows.extend(page.items);
            offset = offset.saturating_add(loaded);
            if loaded == 0 || offset >= page.total {
                break;
            }
        }
        Ok(rows)
    }
}

fn provider_state_id(state: ProviderRunState) -> &'static str {
    match state {
        ProviderRunState::Queued => "queued",
        ProviderRunState::Running => "running",
        ProviderRunState::Paused => "paused",
        ProviderRunState::RateLimited => "rate_limited",
        ProviderRunState::Blocked => "blocked",
        ProviderRunState::CaptchaRequired => "captcha_required",
        ProviderRunState::Completed => "completed",
        ProviderRunState::Failed => "failed",
        ProviderRunState::Cancelled => "cancelled",
    }
}

fn parse_job_state(value: &str) -> CollectionJobState {
    match value {
        "running" => CollectionJobState::Running,
        "paused" => CollectionJobState::Paused,
        "completed" => CollectionJobState::Completed,
        "partial" => CollectionJobState::Partial,
        "failed" => CollectionJobState::Failed,
        "cancelled" => CollectionJobState::Cancelled,
        _ => CollectionJobState::Interrupted,
    }
}
