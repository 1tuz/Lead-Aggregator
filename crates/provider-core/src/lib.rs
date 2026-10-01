use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use twogis_domain::{AppError, Organization, ScrapeProgress, SearchRequest, SourceKind};

pub type ProgressSink = Arc<dyn Fn(ScrapeProgress) + Send + Sync>;

#[derive(Debug, Default)]
pub struct ProviderOutput {
    pub organizations: Vec<Organization>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProviderPolicy {
    pub min_delay_ms: u32,
    pub max_concurrency: u8,
}

impl ProviderPolicy {
    pub const fn conservative(min_delay_ms: u32, max_concurrency: u8) -> Self {
        Self {
            min_delay_ms,
            max_concurrency,
        }
    }
}

#[async_trait]
pub trait DirectoryProvider: Send + Sync {
    fn source(&self) -> SourceKind;
    fn id(&self) -> &'static str;
    fn policy(&self) -> ProviderPolicy;

    async fn search(
        &self,
        request: &SearchRequest,
        progress: ProgressSink,
        cancel: CancellationToken,
    ) -> Result<ProviderOutput, AppError>;
}
