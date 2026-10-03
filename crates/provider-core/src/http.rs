use std::time::Duration;

use reqwest::{Client, StatusCode, redirect::Policy};
use twogis_domain::{AppError, ErrorKind, ResponseDiagnostics, SourceKind};
use url::Url;

use crate::challenge::{ChallengeEvidence, ChallengeKind, detect_challenge};

const DESKTOP_USER_AGENT: &str =
    "Lead-Aggregator/0.4.3 (+desktop app; public catalog HTML; no browser automation)";

#[derive(Clone)]
pub struct CatalogHttpClient {
    client: Client,
}

impl CatalogHttpClient {
    pub fn new() -> Result<Self, AppError> {
        let client = Client::builder()
            .user_agent(DESKTOP_USER_AGENT)
            .timeout(Duration::from_secs(25))
            .connect_timeout(Duration::from_secs(10))
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(4)
            .cookie_store(true)
            .redirect(Policy::limited(10))
            .build()
            .map_err(|e| AppError::network(format!("failed to create HTTP client: {e}")))?;
        Ok(Self { client })
    }

    pub const fn client(&self) -> &Client {
        &self.client
    }

    pub async fn get_html(&self, source: SourceKind, url: &Url) -> Result<String, AppError> {
        let request_url = url.as_str().to_owned();
        let response = self
            .client
            .get(url.clone())
            .header(
                reqwest::header::ACCEPT,
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header(
                reqwest::header::ACCEPT_LANGUAGE,
                "ru-RU,ru;q=0.9,en-US;q=0.8,en;q=0.7",
            )
            .send()
            .await
            .map_err(|e| {
                AppError::network(format!(
                    "{} request failed for {request_url}: {e}",
                    source.label()
                ))
            })?;

        let status = response.status();
        let final_url = response.url().as_str().to_owned();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());

        if status == StatusCode::TOO_MANY_REQUESTS {
            let diagnostics = ResponseDiagnostics {
                source,
                http_status: Some(status.as_u16()),
                request_url: request_url.clone(),
                final_url: Some(final_url.clone()),
                page_title: None,
                reason: "HTTP 429 Too Many Requests".into(),
            };
            return Err(AppError::new(
                ErrorKind::RateLimited,
                format!(
                    "{} вернул HTTP 429 (слишком много запросов). Ждём Retry-After/backoff.",
                    source.label()
                ),
                true,
            )
            .with_retry_after(retry_after)
            .with_diagnostics(diagnostics));
        }

        if status == StatusCode::FORBIDDEN {
            let body = response.text().await.unwrap_or_default();
            let title = crate::challenge::detect_challenge(&ChallengeEvidence {
                source,
                http_status: Some(403),
                request_url: &request_url,
                final_url: Some(&final_url),
                html: &body,
            })
            .page_title;
            let diagnostics = ResponseDiagnostics {
                source,
                http_status: Some(403),
                request_url,
                final_url: Some(final_url),
                page_title: title,
                reason: "HTTP 403 Forbidden".into(),
            };
            return Err(AppError::new(
                ErrorKind::Blocked,
                format!(
                    "{} вернул HTTP 403 (доступ ограничен). Обход anti-bot не выполняется.",
                    source.label()
                ),
                true,
            )
            .with_diagnostics(diagnostics));
        }

        if !status.is_success() {
            return Err(AppError::network(format!(
                "{} returned HTTP {status} for {request_url}",
                source.label()
            ))
            .with_diagnostics(ResponseDiagnostics {
                source,
                http_status: Some(status.as_u16()),
                request_url,
                final_url: Some(final_url),
                page_title: None,
                reason: format!("unexpected HTTP {}", status.as_u16()),
            }));
        }

        let body = response
            .text()
            .await
            .map_err(|e| AppError::network(format!("failed to read {request_url}: {e}")))?;

        let detection = detect_challenge(&ChallengeEvidence {
            source,
            http_status: Some(status.as_u16()),
            request_url: &request_url,
            final_url: Some(&final_url),
            html: &body,
        });

        if let Some(kind) = detection.error_kind() {
            let label = match detection.kind {
                ChallengeKind::Captcha => "CAPTCHA",
                ChallengeKind::AntiBot => "anti-bot challenge",
                ChallengeKind::None => "challenge",
            };
            let diagnostics = ResponseDiagnostics {
                source,
                http_status: Some(status.as_u16()),
                request_url,
                final_url: Some(final_url),
                page_title: detection.page_title.clone(),
                reason: detection.reason.clone(),
            };
            return Err(AppError::new(
                kind,
                format!(
                    "{}: обнаружен {label} ({})",
                    source.label(),
                    detection.reason
                ),
                true,
            )
            .with_diagnostics(diagnostics));
        }

        Ok(body)
    }
}
