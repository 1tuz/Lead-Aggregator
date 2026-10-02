mod parse;

use std::{collections::HashSet, time::Duration};

use async_trait::async_trait;
use futures::{StreamExt, stream};
use reqwest::{Client, StatusCode};
use scraper::{Html, Selector};
use tokio_util::sync::CancellationToken;
use twogis_domain::{
    AppError, ErrorKind, ProgressPhase, ScrapeProgress, SearchRequest, SourceKind,
};
use twogis_provider_core::{
    DirectoryProvider, ProgressSink, ProviderOutput, ProviderPolicy, ProviderRuntime,
};
use url::Url;

#[derive(Clone)]
pub struct TwoGisHtmlProvider {
    client: Client,
    base: Url,
}

impl TwoGisHtmlProvider {
    pub fn new() -> Result<Self, AppError> {
        let client = Client::builder()
            .user_agent("lead-aggregator/0.2 (+local desktop app; public HTML only)")
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| AppError::network(format!("failed to create HTTP client: {e}")))?;
        let base = Url::parse("https://2gis.ru/")
            .map_err(|e| AppError::new(ErrorKind::Internal, e.to_string(), false))?;
        Ok(Self { client, base })
    }

    fn search_url(&self, request: &SearchRequest, page: u16) -> Result<Url, AppError> {
        let mut url = self.base.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| AppError::new(ErrorKind::Internal, "invalid base URL", false))?;
            segments.push(&request.region);
            segments.push("search");
            segments.push(request.query.trim());
            if page > 1 {
                segments.push("page");
                segments.push(&page.to_string());
            }
        }
        Ok(url)
    }

    async fn fetch_html(&self, url: &Url) -> Result<String, AppError> {
        let response = self
            .client
            .get(url.clone())
            .header("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.6")
            .send()
            .await
            .map_err(|e| AppError::network(format!("request failed for {url}: {e}")))?;

        match response.status() {
            StatusCode::TOO_MANY_REQUESTS => {
                return Err(AppError::new(
                    ErrorKind::RateLimited,
                    "2GIS returned HTTP 429. The app stops instead of bypassing the limit.",
                    true,
                ));
            }
            StatusCode::FORBIDDEN => {
                return Err(AppError::new(
                    ErrorKind::Blocked,
                    "2GIS returned HTTP 403. No anti-bot bypass is attempted.",
                    true,
                ));
            }
            status if !status.is_success() => {
                return Err(AppError::network(format!(
                    "2GIS returned HTTP {status} for {url}"
                )));
            }
            _ => {}
        }

        let html = response
            .text()
            .await
            .map_err(|e| AppError::network(format!("failed to read {url}: {e}")))?;
        if is_challenge_html(&html) {
            return Err(AppError::new(
                ErrorKind::Blocked,
                "2GIS requested an anti-bot check; provider stopped",
                true,
            ));
        }
        Ok(html)
    }

    fn discover_firms(&self, html: &str) -> Vec<(String, String)> {
        let document = Html::parse_document(html);
        let Ok(selector) = Selector::parse(r#"a[href*="/firm/"]"#) else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        let mut out = Vec::new();

        for anchor in document.select(&selector) {
            let Some(href) = anchor.value().attr("href") else {
                continue;
            };
            let Some(url) = self.normalize_firm_url(href) else {
                continue;
            };
            let key = url.path().to_owned();
            if !seen.insert(key) {
                continue;
            }
            let name = anchor
                .text()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            out.push((url.to_string(), name));
        }
        out
    }

    fn normalize_firm_url(&self, href: &str) -> Option<Url> {
        let mut url = self.base.join(href).ok()?;
        if !url.path().contains("/firm/") {
            return None;
        }
        url.set_query(None);
        url.set_fragment(None);
        Some(url)
    }
}

#[async_trait]
impl DirectoryProvider for TwoGisHtmlProvider {
    fn source(&self) -> SourceKind {
        SourceKind::TwoGis
    }

    fn id(&self) -> &'static str {
        "2gis-public-html"
    }

    fn policy(&self) -> ProviderPolicy {
        ProviderPolicy::conservative(650, 4)
    }

    async fn search(
        &self,
        request: &SearchRequest,
        progress: ProgressSink,
        cancel: CancellationToken,
    ) -> Result<ProviderOutput, AppError> {
        request.validate()?;
        let runtime = ProviderRuntime::for_request(self.policy(), request);

        let mut candidates = Vec::<(String, String)>::new();
        let mut seen = HashSet::new();

        for page in 1..=request.max_pages {
            if cancel.is_cancelled() {
                return Err(AppError::cancelled());
            }
            (progress)(ScrapeProgress {
                phase: ProgressPhase::Discovering,
                current: u32::from(page),
                total: Some(u32::from(request.max_pages)),
                message: format!("Scanning search page {page}"),
            });
            let url = self.search_url(request, page)?;
            let html = runtime.run(&cancel, self.fetch_html(&url)).await?;
            let found = self.discover_firms(&html);
            if found.is_empty() {
                break;
            }
            let before = candidates.len();
            for candidate in found {
                if seen.insert(candidate.0.clone()) {
                    candidates.push(candidate);
                    if candidates.len() >= request.max_results as usize {
                        break;
                    }
                }
            }
            if candidates.len() >= request.max_results as usize || candidates.len() == before {
                break;
            }
        }

        candidates.truncate(request.max_results as usize);
        let total = candidates.len() as u32;
        let concurrency = runtime.concurrency();
        let provider = self.clone();
        let runtime_for_stream = runtime.clone();
        let progress_for_stream = progress.clone();
        let provider_cancel = cancel.child_token();
        let cancel_for_stream = provider_cancel.clone();

        let results = stream::iter(candidates.into_iter().enumerate())
            .map(move |(index, (url, fallback_name))| {
                let provider = provider.clone();
                let runtime = runtime_for_stream.clone();
                let progress = progress_for_stream.clone();
                let cancel = cancel_for_stream.clone();
                async move {
                    if cancel.is_cancelled() {
                        return Err(AppError::cancelled());
                    }
                    let parsed_url = Url::parse(&url)
                        .map_err(|e| AppError::parse(format!("bad organization URL: {e}")))?;
                    let html = match runtime.run(&cancel, provider.fetch_html(&parsed_url)).await {
                        Ok(html) => html,
                        Err(err)
                            if matches!(err.kind, ErrorKind::RateLimited | ErrorKind::Blocked) =>
                        {
                            cancel.cancel();
                            return Err(err);
                        }
                        Err(err) => return Err(err),
                    };
                    let mut organization = parse::parse_firm_page(&url, &html)?;
                    organization.attach_source(SourceKind::TwoGis);
                    if organization.name.is_empty() {
                        organization.name = fallback_name;
                    }
                    (progress)(ScrapeProgress {
                        phase: ProgressPhase::Enriching,
                        current: index as u32 + 1,
                        total: Some(total),
                        message: organization.name.clone(),
                    });
                    Ok::<_, AppError>(organization)
                }
            })
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;

        if cancel.is_cancelled() {
            return Err(AppError::cancelled());
        }

        let mut output = ProviderOutput::default();
        let mut terminal_error = None;
        for result in results {
            match result {
                Ok(org) => output.organizations.push(org),
                Err(err) if matches!(err.kind, ErrorKind::RateLimited | ErrorKind::Blocked) => {
                    terminal_error.get_or_insert(err);
                }
                Err(err) if matches!(err.kind, ErrorKind::Cancelled) && cancel.is_cancelled() => {
                    return Err(err);
                }
                Err(err) if matches!(err.kind, ErrorKind::Cancelled) => {}
                Err(err) => output.warnings.push(err.message),
            }
        }
        if let Some(err) = terminal_error {
            return Err(err);
        }
        output.organizations.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(output)
    }
}

fn is_challenge_html(html: &str) -> bool {
    let lower = html.to_lowercase();
    lower.contains("captcha")
        || lower.contains("подтвердите, что вы не робот")
        || lower.contains("проверка безопасности")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_unique_firm_links() -> Result<(), Box<dyn std::error::Error>> {
        let provider = TwoGisHtmlProvider::new()?;
        let html = r#"
          <html><body>
            <a href="/moscow/firm/70000000000000001">Alpha</a>
            <a href="/moscow/firm/70000000000000001?m=1,2">Alpha duplicate</a>
            <a href="/moscow/firm/70000000000000002">Beta</a>
          </body></html>
        "#;
        let rows = provider.discover_firms(html);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, "Alpha");
        Ok(())
    }

    #[test]
    fn identifies_captcha_response_in_html() {
        assert!(is_challenge_html("<title>CAPTCHA</title>"));
        assert!(!is_challenge_html("<h1>Автосервис</h1>"));
    }
}
