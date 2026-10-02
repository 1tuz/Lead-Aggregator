use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use tokio::{
    sync::{Mutex, Notify, Semaphore},
    time::{Instant, sleep_until},
};
use tokio_util::sync::CancellationToken;
use twogis_domain::{
    AppError, ErrorKind, Organization, ProviderRunState, ScrapeProgress, SearchRequest, SourceKind,
};

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
pub struct ProviderControl {
    cancel: CancellationToken,
    paused: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl ProviderControl {
    pub fn new(cancel: CancellationToken) -> Self {
        Self {
            cancel,
            paused: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::Release);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    pub async fn wait_ready(&self) -> Result<(), AppError> {
        loop {
            if self.cancel.is_cancelled() {
                return Err(AppError::cancelled());
            }
            if !self.is_paused() {
                return Ok(());
            }
            tokio::select! {
                _ = self.cancel.cancelled() => return Err(AppError::cancelled()),
                _ = self.notify.notified() => {}
            }
        }
    }

    pub async fn sleep(&self, duration: Duration) -> Result<(), AppError> {
        tokio::select! {
            _ = self.cancel.cancelled() => Err(AppError::cancelled()),
            _ = tokio::time::sleep(duration) => Ok(()),
        }
    }

    pub const fn paused_state() -> ProviderRunState {
        ProviderRunState::Paused
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
    pub fn for_request(
        policy: ProviderPolicy,
        request: &SearchRequest,
        source: SourceKind,
    ) -> Self {
        let config = request.config_for(source);
        let concurrency = usize::from(config.concurrency.min(policy.max_concurrency).max(1));
        let delay =
            Duration::from_millis(u64::from(config.request_delay_ms.max(policy.min_delay_ms)));
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
        control: &ProviderControl,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, AppError> {
        control.wait_ready().await?;
        let permit = tokio::select! {
            _ = control.cancel.cancelled() => return Err(AppError::cancelled()),
            permit = self.semaphore.clone().acquire_owned() => permit.map_err(|_| {
                AppError::new(ErrorKind::Internal, "provider runtime semaphore closed", false)
            })?,
        };

        let mut next_request = self.next_request.lock().await;
        let now = Instant::now();
        if *next_request > now {
            tokio::select! {
                _ = control.cancel.cancelled() => return Err(AppError::cancelled()),
                _ = sleep_until(*next_request) => {}
            }
        }
        *next_request = Instant::now() + self.delay;
        Ok(permit)
    }

    pub async fn run<T, F>(&self, control: &ProviderControl, future: F) -> Result<T, AppError>
    where
        F: Future<Output = Result<T, AppError>>,
    {
        let _permit = self.acquire(control).await?;
        tokio::select! {
            _ = control.cancel.cancelled() => Err(AppError::cancelled()),
            result = future => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_concurrency_to_provider_policy() {
        let request = SearchRequest {
            concurrency: 8,
            ..SearchRequest::default()
        };
        let runtime = ProviderRuntime::for_request(
            ProviderPolicy::conservative(250, 2),
            &request,
            SourceKind::TwoGis,
        );
        assert_eq!(runtime.concurrency(), 2);
    }

    #[tokio::test]
    async fn cancelled_runtime_does_not_start_request() {
        let request = SearchRequest::default();
        let runtime = ProviderRuntime::for_request(
            ProviderPolicy::conservative(250, 1),
            &request,
            SourceKind::TwoGis,
        );
        let control = ProviderControl::new(CancellationToken::new());
        control.cancel();

        let result = runtime.run(&control, async { Ok::<_, AppError>(()) }).await;
        assert!(matches!(
            result,
            Err(AppError {
                kind: ErrorKind::Cancelled,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn paused_control_waits_until_resume() {
        let control = ProviderControl::new(CancellationToken::new());
        control.pause();
        let resumed = control.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            resumed.resume();
        });
        control.wait_ready().await.expect("control should resume");
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
        control: ProviderControl,
    ) -> Result<ProviderOutput, AppError>;
}
