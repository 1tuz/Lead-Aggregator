use std::time::Duration;

use reqwest::{Client, redirect::Policy};
use twogis_domain::{AppError, ErrorKind, ResponseDiagnostics, SourceKind};
use url::Url;

use crate::challenge::{ChallengeEvidence, ChallengeKind, detect_challenge};

pub const DESKTOP_USER_AGENT: &str = concat!(
    "Lead-Aggregator/",
    env!("CARGO_PKG_VERSION"),
    " (+desktop app; public catalog HTML; no browser automation)"
);

#[derive(Debug, Clone)]
pub struct HtmlFetchReport {
    pub request_url: String,
    pub final_url: String,
    pub http_status: u16,
    pub content_type: Option<String>,
    pub retry_after_seconds: Option<u64>,
    pub body: String,
    pub challenge: crate::challenge::ChallengeDetection,
}

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
        let report = self.fetch_html_report(source, url).await?;
        let status = report.http_status;

        if classify_http_block(status) == Some(ErrorKind::RateLimited) {
            let diagnostics = ResponseDiagnostics {
                source,
                http_status: Some(status),
                request_url: report.request_url.clone(),
                final_url: Some(report.final_url.clone()),
                page_title: report.challenge.page_title.clone(),
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
            .with_retry_after(report.retry_after_seconds)
            .with_diagnostics(diagnostics));
        }

        if classify_http_block(status) == Some(ErrorKind::Blocked) {
            let diagnostics = ResponseDiagnostics {
                source,
                http_status: Some(403),
                request_url: report.request_url,
                final_url: Some(report.final_url),
                page_title: report.challenge.page_title,
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

        if !(200..300).contains(&status) {
            return Err(AppError::network(format!(
                "{} returned HTTP {status} for {}",
                source.label(),
                report.request_url
            ))
            .with_diagnostics(ResponseDiagnostics {
                source,
                http_status: Some(status),
                request_url: report.request_url,
                final_url: Some(report.final_url),
                page_title: report.challenge.page_title,
                reason: format!("unexpected HTTP {status}"),
            }));
        }

        if let Some(kind) = report.challenge.error_kind() {
            let label = match report.challenge.kind {
                ChallengeKind::Captcha => "CAPTCHA",
                ChallengeKind::AntiBot => "anti-bot challenge",
                ChallengeKind::None => "challenge",
            };
            let diagnostics = ResponseDiagnostics {
                source,
                http_status: Some(status),
                request_url: report.request_url,
                final_url: Some(report.final_url),
                page_title: report.challenge.page_title.clone(),
                reason: report.challenge.reason.clone(),
            };
            return Err(AppError::new(
                kind,
                format!(
                    "{}: обнаружен {label} ({})",
                    source.label(),
                    report.challenge.reason
                ),
                true,
            )
            .with_diagnostics(diagnostics));
        }

        Ok(report.body)
    }

    /// Fetch HTML for diagnostics. Returns body for HTTP responses including 403/404.
    pub async fn fetch_html_report(
        &self,
        source: SourceKind,
        url: &Url,
    ) -> Result<HtmlFetchReport, AppError> {
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

        let status = response.status().as_u16();
        let final_url = response.url().as_str().to_owned();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let retry_after_seconds = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        let body = response
            .text()
            .await
            .map_err(|e| AppError::network(format!("failed to read {request_url}: {e}")))?;
        let challenge = detect_challenge(&ChallengeEvidence {
            source,
            http_status: Some(status),
            request_url: &request_url,
            final_url: Some(&final_url),
            html: &body,
        });

        Ok(HtmlFetchReport {
            request_url,
            final_url,
            http_status: status,
            content_type,
            retry_after_seconds,
            body,
            challenge,
        })
    }
}

/// Pure mapping used by CatalogHttpClient (unit-tested without network).
pub fn classify_http_block(status: u16) -> Option<ErrorKind> {
    match status {
        429 => Some(ErrorKind::RateLimited),
        403 => Some(ErrorKind::Blocked),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_429_and_403_map_for_all_sources() {
        for source in [
            SourceKind::TwoGis,
            SourceKind::Yell,
            SourceKind::Zoon,
            SourceKind::Rusprofile,
        ] {
            assert_eq!(
                classify_http_block(429),
                Some(ErrorKind::RateLimited),
                "{source:?}"
            );
            assert_eq!(
                classify_http_block(403),
                Some(ErrorKind::Blocked),
                "{source:?}"
            );
            assert_eq!(classify_http_block(200), None, "{source:?}");
            assert_eq!(classify_http_block(404), None, "{source:?}");
            let _ = source;
        }
    }
}
