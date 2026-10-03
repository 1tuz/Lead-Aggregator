mod parse;

use std::{collections::HashSet, time::Duration};

use async_trait::async_trait;
use futures::{StreamExt, stream};
use scraper::{Html, Selector};
use twogis_domain::{
    AppError, ErrorKind, ProgressPhase, ProviderRunState, ScrapeProgress, SearchRequest, SourceKind,
};
use twogis_provider_core::{
    CatalogHttpClient, DirectoryProvider, ProgressSink, ProviderControl, ProviderOutput,
    ProviderPolicy, ProviderRuntime,
};
use url::Url;

#[derive(Clone)]
pub struct TwoGisHtmlProvider {
    http: CatalogHttpClient,
    base: Url,
}

impl TwoGisHtmlProvider {
    pub fn new() -> Result<Self, AppError> {
        let http = CatalogHttpClient::new()?;
        let base = Url::parse("https://2gis.ru/")
            .map_err(|e| AppError::new(ErrorKind::Internal, e.to_string(), false))?;
        Ok(Self { http, base })
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
        self.http.get_html(SourceKind::TwoGis, url).await
    }

    async fn fetch_with_backoff(
        &self,
        request: &SearchRequest,
        runtime: &ProviderRuntime,
        control: &ProviderControl,
        progress: &ProgressSink,
        url: &Url,
    ) -> Result<String, AppError> {
        let config = request.config_for(SourceKind::TwoGis);
        for attempt in 0..=config.max_retries {
            match runtime.run(control, self.fetch_html(url)).await {
                Ok(html) => return Ok(html),
                Err(err)
                    if matches!(err.kind, ErrorKind::RateLimited)
                        && attempt < config.max_retries =>
                {
                    let fallback = u64::from(config.backoff_base_seconds)
                        .saturating_mul(1_u64 << attempt.min(3));
                    let delay = err.retry_after_seconds.unwrap_or(fallback).clamp(5, 900);
                    (progress)(ScrapeProgress {
                        phase: ProgressPhase::Discovering,
                        current: 0,
                        total: None,
                        message: format!("2GIS rate limit: retry in {delay}s"),
                        source: Some(SourceKind::TwoGis),
                        region: Some(request.region.clone()),
                        state: Some(ProviderRunState::RateLimited),
                        retry_after_seconds: Some(delay),
                    });
                    control.sleep(Duration::from_secs(delay)).await?;
                }
                Err(err)
                    if matches!(
                        err.kind,
                        ErrorKind::Blocked
                            | ErrorKind::CaptchaRequired
                            | ErrorKind::ChallengeRequired
                            | ErrorKind::RateLimited
                    ) =>
                {
                    control.cancel();
                    return Err(err);
                }
                Err(err) => return Err(err),
            }
        }
        Err(AppError::network("2GIS retry loop exhausted"))
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
        ProviderPolicy::conservative(1_500, 1)
    }

    async fn search(
        &self,
        request: &SearchRequest,
        progress: ProgressSink,
        control: ProviderControl,
    ) -> Result<ProviderOutput, AppError> {
        request.validate()?;
        let config = request.config_for(SourceKind::TwoGis);
        let runtime = ProviderRuntime::for_request(self.policy(), request, SourceKind::TwoGis);

        let mut candidates = Vec::<(String, String)>::new();
        let mut seen = HashSet::new();

        for page in 1..=config.max_pages {
            if control.is_cancelled() {
                return Err(AppError::cancelled());
            }
            (progress)(ScrapeProgress {
                phase: ProgressPhase::Discovering,
                current: u32::from(page),
                total: Some(u32::from(config.max_pages)),
                message: format!("2GIS: scanning page {page}"),
                source: Some(SourceKind::TwoGis),
                region: Some(request.region.clone()),
                state: Some(ProviderRunState::Running),
                retry_after_seconds: None,
            });
            let url = self.search_url(request, page)?;
            let html = self
                .fetch_with_backoff(request, &runtime, &control, &progress, &url)
                .await?;
            let found = self.discover_firms(&html);
            if found.is_empty() {
                break;
            }
            let before = candidates.len();
            for candidate in found {
                if seen.insert(candidate.0.clone()) {
                    candidates.push(candidate);
                    if candidates.len() >= config.max_results as usize {
                        break;
                    }
                }
            }
            if candidates.len() >= config.max_results as usize || candidates.len() == before {
                break;
            }
        }

        candidates.truncate(config.max_results as usize);
        let total = candidates.len() as u32;
        let concurrency = runtime.concurrency();
        let provider = self.clone();
        let runtime_for_stream = runtime.clone();
        let progress_for_stream = progress.clone();
        let control_for_stream = control.clone();
        let request_for_stream = request.clone();

        let results = stream::iter(candidates.into_iter().enumerate())
            .map(move |(index, (url, fallback_name))| {
                let provider = provider.clone();
                let runtime = runtime_for_stream.clone();
                let progress = progress_for_stream.clone();
                let control = control_for_stream.clone();
                let request = request_for_stream.clone();
                async move {
                    if control.is_cancelled() {
                        return Err(AppError::cancelled());
                    }
                    let parsed_url = Url::parse(&url)
                        .map_err(|e| AppError::parse(format!("bad organization URL: {e}")))?;
                    let html = provider
                        .fetch_with_backoff(&request, &runtime, &control, &progress, &parsed_url)
                        .await?;
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
                        source: Some(SourceKind::TwoGis),
                        region: Some(request.region.clone()),
                        state: Some(ProviderRunState::Running),
                        retry_after_seconds: None,
                    });
                    Ok::<_, AppError>(organization)
                }
            })
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;

        let mut output = ProviderOutput::default();
        let mut terminal_error = None;
        for result in results {
            match result {
                Ok(org) => output.organizations.push(org),
                Err(err)
                    if matches!(
                        err.kind,
                        ErrorKind::RateLimited
                            | ErrorKind::Blocked
                            | ErrorKind::CaptchaRequired
                            | ErrorKind::ChallengeRequired
                    ) =>
                {
                    terminal_error.get_or_insert(err);
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
}
