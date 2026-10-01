use std::{future::Future, sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::{
    sync::{Mutex, Semaphore},
    time::{Instant, sleep_until},
};
use tokio_util::sync::CancellationToken;
use twogis_domain::{AppError, ErrorKind, Organization, ScrapeProgress, SearchRequest, SourceKind};

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

#[derive(Clone)]
pub struct ProviderRuntime {
    semaphore: Arc<Semaphore>,
    next_request: Arc<Mutex<Instant>>,
    delay: Duration,
    concurrency: usize,
}

impl ProviderRuntime {
    pub fn for_request(policy: ProviderPolicy, request: &SearchRequest) -> Self {
        let concurrency = usize::from(request.concurrency.min(policy.max_concurrency).max(1));
        let delay =
            Duration::from_millis(u64::from(request.request_delay_ms.max(policy.min_delay_ms)));
        Self {
            semaphore: Arc::new(Semaphore::new(concurrency)),
            next_request: Arc::new(Mutex::new(Instant::now())),
            delay,
            concurrency,
        }
    }

    pub const fn concurrency(&self) -> usize {
        self.concurrency
    }

    async fn acquire(
        &self,
        cancel: &CancellationToken,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, AppError> {
        let permit = tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::cancelled()),
            permit = self.semaphore.clone().acquire_owned() => permit.map_err(|_| {
                AppError::new(ErrorKind::Internal, "provider runtime semaphore closed", false)
            })?,
        };

        let mut next_request = self.next_request.lock().await;
        let now = Instant::now();
        if *next_request > now {
            tokio::select! {
                _ = cancel.cancelled() => return Err(AppError::cancelled()),
                _ = sleep_until(*next_request) => {}
            }
        }
        *next_request = Instant::now() + self.delay;
        Ok(permit)
    }

    pub async fn run<T, F>(&self, cancel: &CancellationToken, future: F) -> Result<T, AppError>
    where
        F: Future<Output = Result<T, AppError>>,
    {
        let _permit = self.acquire(cancel).await?;
        tokio::select! {
            _ = cancel.cancelled() => Err(AppError::cancelled()),
            result = future => result,
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
