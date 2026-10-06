use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{AppError, Organization, SearchRequest, SourceKind};

pub const MAX_PROVIDER_RESULTS: u32 = 50_000;
pub const MAX_PROVIDER_PAGES: u16 = 5_000;
pub const MAX_COLLECTION_REGIONS: usize = 2_000;
pub const DEFAULT_RESULT_PAGE_SIZE: u32 = 500;
pub const MAX_RESULT_PAGE_SIZE: u32 = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CollectionPreset {
    Gentle,
    Normal,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSearchConfig {
    pub source: SourceKind,
    pub enabled: bool,
    pub max_results: u32,
    pub max_pages: u16,
    pub concurrency: u8,
    pub request_delay_ms: u32,
    pub preset: CollectionPreset,
    pub max_retries: u8,
    pub backoff_base_seconds: u32,
}

impl ProviderSearchConfig {
    pub fn recommended(source: SourceKind, preset: CollectionPreset) -> Self {
        // db-export provider: each rubric is one plain GET of a static file
        // from our own data source, so no throttling is required. The policy
        // floor stays minimal; users can still slow it down manually.
        let (delay, concurrency, results, pages, retries, backoff) = match (source, preset) {
            (SourceKind::TwoGis, CollectionPreset::Gentle) => (0, 1, 50_000, 1_000, 2, 60),
            (SourceKind::TwoGis, _) => (0, 1, 50_000, 2_000, 1, 45),
        };
        Self {
            source,
            // One source by default: parallel catalogs raise CAPTCHA risk on shared IPs.
            enabled: matches!(source, SourceKind::TwoGis),
            max_results: results,
            max_pages: pages,
            concurrency,
            request_delay_ms: delay,
            preset,
            max_retries: retries,
            backoff_base_seconds: backoff,
        }
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if self.max_results == 0 || self.max_results > MAX_PROVIDER_RESULTS {
            return Err(AppError::validation(format!(
                "{} maxResults must be between 1 and {MAX_PROVIDER_RESULTS}",
                self.source.label()
            )));
        }
        if self.max_pages == 0 || self.max_pages > MAX_PROVIDER_PAGES {
            return Err(AppError::validation(format!(
                "{} maxPages must be between 1 and {MAX_PROVIDER_PAGES}",
                self.source.label()
            )));
        }
        if !(1..=8).contains(&self.concurrency) {
            return Err(AppError::validation(format!(
                "{} concurrency must be between 1 and 8",
                self.source.label()
            )));
        }
        if self.request_delay_ms > 10_000 {
            return Err(AppError::validation(format!(
                "{} request delay must be at most 10000ms",
                self.source.label()
            )));
        }
        if self.max_retries > 3 {
            return Err(AppError::validation("maxRetries must be between 0 and 3"));
        }
        if !(5..=900).contains(&self.backoff_base_seconds) {
            return Err(AppError::validation(
                "backoffBaseSeconds must be between 5 and 900",
            ));
        }
        Ok(())
    }
}

pub fn default_provider_configs() -> Vec<ProviderSearchConfig> {
    [SourceKind::TwoGis]
        .into_iter()
        .map(|source| ProviderSearchConfig::recommended(source, CollectionPreset::Gentle))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProviderRunState {
    Queued,
    Running,
    Paused,
    RateLimited,
    Blocked,
    CaptchaRequired,
    ChallengeRequired,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub source: SourceKind,
    pub region: String,
    pub state: ProviderRunState,
    pub current: u32,
    pub total: Option<u32>,
    pub message: String,
    #[specta(type = Option<f64>)]
    pub retry_after_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CollectionJobState {
    Running,
    Paused,
    Completed,
    Partial,
    Failed,
    Interrupted,
    Cancelled,
}

impl CollectionJobState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CollectionJobInfo {
    pub job_id: String,
    pub state: CollectionJobState,
    pub request: SearchRequest,
    pub started_at: String,
    pub updated_at: String,
    pub message: String,
    pub completed_targets: u32,
    pub total_targets: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunResultsPage {
    pub run_id: String,
    pub items: Vec<Organization>,
    pub offset: u32,
    pub limit: u32,
    pub total: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_defaults_enable_two_gis() {
        let two_gis =
            ProviderSearchConfig::recommended(SourceKind::TwoGis, CollectionPreset::Gentle);
        assert_eq!(two_gis.concurrency, 1);
        assert!(two_gis.enabled);
    }

    #[test]
    fn default_provider_configs_use_gentle_preset() {
        let configs = default_provider_configs();
        assert!(
            configs
                .iter()
                .all(|config| config.preset == CollectionPreset::Gentle)
        );
        assert_eq!(
            configs
                .iter()
                .filter(|config| config.enabled)
                .map(|config| config.source)
                .collect::<Vec<_>>(),
            vec![SourceKind::TwoGis]
        );
    }

    #[test]
    fn large_but_bounded_provider_config_is_valid() {
        let config = ProviderSearchConfig {
            max_results: 50_000,
            max_pages: 5_000,
            ..ProviderSearchConfig::recommended(SourceKind::TwoGis, CollectionPreset::Custom)
        };
        assert!(config.validate().is_ok());
    }
}
