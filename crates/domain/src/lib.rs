use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    TwoGis,
    Yell,
    Zoon,
    Rusprofile,
}

impl SourceKind {
    pub const fn id(self) -> &'static str {
        match self {
            Self::TwoGis => "2gis",
            Self::Yell => "yell",
            Self::Zoon => "zoon",
            Self::Rusprofile => "rusprofile",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::TwoGis => "2GIS",
            Self::Yell => "Yell",
            Self::Zoon => "Zoon",
            Self::Rusprofile => "Rusprofile",
        }
    }
}

fn default_sources() -> Vec<SourceKind> {
    vec![SourceKind::TwoGis]
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub region: String,
    pub query: String,
    pub max_results: u32,
    pub max_pages: u16,
    pub concurrency: u8,
    pub request_delay_ms: u32,
    #[serde(default = "default_sources")]
    pub sources: Vec<SourceKind>,
}

impl Default for SearchRequest {
    fn default() -> Self {
        Self {
            region: "moscow".into(),
            query: "автосервис".into(),
            max_results: 100,
            max_pages: 5,
            concurrency: 2,
            request_delay_ms: 650,
            sources: default_sources(),
        }
    }
}

impl SearchRequest {
    pub fn validate(&self) -> Result<(), AppError> {
        let region_ok = !self.region.is_empty()
            && self.region.len() <= 64
            && self
                .region
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !region_ok {
            return Err(AppError::validation(
                "Region must be a URL slug such as moscow, spb or kazan",
            ));
        }
        let query = self.query.trim();
        if query.len() < 2 || query.len() > 160 {
            return Err(AppError::validation("Query must contain 2-160 characters"));
        }
        if self.max_results == 0 || self.max_results > 5_000 {
            return Err(AppError::validation(
                "maxResults must be between 1 and 5000",
            ));
        }
        if self.max_pages == 0 || self.max_pages > 100 {
            return Err(AppError::validation("maxPages must be between 1 and 100"));
        }
        if !(1..=8).contains(&self.concurrency) {
            return Err(AppError::validation("concurrency must be between 1 and 8"));
        }
        if self.request_delay_ms < 250 {
            return Err(AppError::validation(
                "requestDelayMs must be at least 250ms",
            ));
        }
        if self.sources.is_empty() {
            return Err(AppError::validation("Select at least one source"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceAttribution {
    pub source: SourceKind,
    pub source_id: String,
    pub source_url: String,
    pub collected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LeadBranch {
    pub address: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub source: SourceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DedupeInfo {
    pub merged_records: u32,
    pub fingerprint: String,
    pub possible_duplicate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    pub address: Option<String>,
    pub rating: Option<f64>,
    pub review_count: Option<u32>,
    pub phones: Vec<String>,
    pub email: Option<String>,
    pub website: Option<String>,
    pub socials: Vec<String>,
    pub opening_status: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub source_url: String,
    pub collected_at: String,
    #[serde(default)]
    pub inn: Option<String>,
    #[serde(default)]
    pub ogrn: Option<String>,
    #[serde(default)]
    pub sources: Vec<SourceAttribution>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub branches: Vec<LeadBranch>,
    #[serde(default)]
    pub dedupe: DedupeInfo,
}

impl Organization {
    pub fn attach_source(&mut self, source: SourceKind) {
        if self.sources.is_empty() {
            self.sources.push(SourceAttribution {
                source,
                source_id: self.id.clone(),
                source_url: self.source_url.clone(),
                collected_at: self.collected_at.clone(),
            });
        }
        let source_tag = source.label().to_owned();
        if !self.tags.contains(&source_tag) {
            self.tags.push(source_tag);
        }
        if let Some(address) = self.address.clone() {
            let branch = LeadBranch {
                address,
                latitude: self.latitude,
                longitude: self.longitude,
                source,
            };
            if !self.branches.contains(&branch) {
                self.branches.push(branch);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub run_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub organizations: Vec<Organization>,
    pub warnings: Vec<String>,
    pub raw_records: u32,
    pub duplicates_merged: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchRunInfo {
    pub run_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub request: SearchRequest,
    pub warnings: Vec<String>,
    pub raw_records: u32,
    pub duplicates_merged: u32,
    pub organization_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeProgress {
    pub phase: ProgressPhase,
    pub current: u32,
    pub total: Option<u32>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProgressPhase {
    Discovering,
    Enriching,
    Resolving,
    Saving,
    Done,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Csv,
    Json,
    Xlsx,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportReceipt {
    pub path: String,
    pub rows: u32,
    pub format: ExportFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HealthInfo {
    pub app_version: String,
    pub provider: String,
    pub browser_engine: String,
    pub api_key_required: bool,
    pub available_sources: Vec<SourceKind>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    Validation,
    Network,
    RateLimited,
    Blocked,
    Parse,
    Storage,
    Export,
    Cancelled,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Error)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl AppError {
    pub fn new(kind: ErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Validation, message, false)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Network, message, true)
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Parse, message, true)
    }

    pub fn storage(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Storage, message, true)
    }

    pub fn export(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Export, message, true)
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorKind::Cancelled, "Search cancelled", true)
    }
}
