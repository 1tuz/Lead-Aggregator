mod parse;

use std::collections::HashSet;

use async_trait::async_trait;
use futures::{StreamExt, stream};
use scraper::{Html, Selector};
use twogis_domain::{
    AppError, ErrorKind, ProgressPhase, ProviderRunState, ScrapeProgress, SearchRequest, SourceKind,
};
use twogis_provider_core::{
    CatalogHttpClient, DirectoryProvider, ProgressSink, ProviderControl, ProviderOutput,
    ProviderPolicy, ProviderRuntime, StopReason, source_region_slug,
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

    pub fn search_url(&self, request: &SearchRequest, page: u16) -> Result<Url, AppError> {
        let region = source_region_slug(SourceKind::TwoGis, &request.region);
        let mut url = self.base.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| AppError::new(ErrorKind::Internal, "invalid base URL", false))?;
            segments.push(&region);
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

    async fn fetch_controlled(
        &self,
        runtime: &ProviderRuntime,
        control: &ProviderControl,
        url: &Url,
    ) -> Result<String, AppError> {
        let result = runtime.run(control, self.fetch_html(url)).await;
        if let Err(err) = &result
            && matches!(
                err.kind,
                ErrorKind::Blocked
                    | ErrorKind::CaptchaRequired
                    | ErrorKind::ChallengeRequired
                    | ErrorKind::RateLimited
            )
        {
            control.cancel();
        }
        result
    }

    pub fn discover_firms(&self, html: &str) -> Vec<(String, String)> {
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

    pub fn has_next_page(html: &str, page: u16) -> bool {
        if html.contains("\"hasPagesToLoad\":true") || html.contains("\\\"hasPagesToLoad\\\":true")
        {
            return true;
        }
        let next = page.saturating_add(1);
        html.contains(&format!("/page/{next}")) || html.contains(&format!("/page/{next}/"))
    }

    /// A stale has-more flag must never keep a redirected/repeated page alive.
    pub fn page_stop_reason(found: usize, new: usize, has_next: bool) -> Option<StopReason> {
        if found == 0 {
            Some(StopReason::EmptyPage)
        } else if new == 0 {
            Some(StopReason::NoNewCandidates)
        } else if !has_next {
            Some(StopReason::LastPage)
        } else {
            None
        }
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
        let mut stop_reason = StopReason::MaxPages;

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
            let html = self.fetch_controlled(&runtime, &control, &url).await?;
            let found = self.discover_firms(&html);
            if found.is_empty() {
                stop_reason = StopReason::EmptyPage;
                break;
            }
            let before = candidates.len();
            let found_count = found.len();
            for candidate in found {
                if seen.insert(candidate.0.clone()) {
                    candidates.push(candidate);
                    if candidates.len() >= config.max_results as usize {
                        break;
                    }
                }
            }
            if candidates.len() >= config.max_results as usize {
                stop_reason = StopReason::MaxResults;
                break;
            }
            let has_next = Self::has_next_page(&html, page);
            if let Some(reason) =
                Self::page_stop_reason(found_count, candidates.len() - before, has_next)
            {
                stop_reason = reason;
                break;
            }
            if page == config.max_pages {
                stop_reason = StopReason::MaxPages;
            }
        }

        candidates.truncate(config.max_results as usize);
        let candidate_count = candidates.len() as u32;
        let total = candidate_count;
        let concurrency = runtime.concurrency();
        let provider = self.clone();
        let runtime_for_stream = runtime.clone();
        let progress_for_stream = progress.clone();
        let control_for_stream = control.clone();

        let results = stream::iter(candidates.into_iter().enumerate())
            .map(move |(index, (url, fallback_name))| {
                let provider = provider.clone();
                let runtime = runtime_for_stream.clone();
                let progress = progress_for_stream.clone();
                let control = control_for_stream.clone();
                async move {
                    if control.is_cancelled() {
                        return Err(AppError::cancelled());
                    }
                    let parsed_url = Url::parse(&url)
                        .map_err(|e| AppError::parse(format!("bad organization URL: {e}")))?;
                    let html = provider
                        .fetch_controlled(&runtime, &control, &parsed_url)
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

        let mut output = ProviderOutput {
            stop_reason: Some(stop_reason),
            candidate_count,
            ..ProviderOutput::default()
        };
        if stop_reason == StopReason::NoNewCandidates {
            output.warnings.push(format!(
                "2GIS: следующая страница повторила уже найденные фирмы; остановлено после {candidate_count} кандидатов. Public HTML может перенаправлять глубокие страницы на начало выдачи."
            ));
        }
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

    #[tokio::test]
    async fn rate_limit_cancels_without_retry() -> Result<(), Box<dyn std::error::Error>> {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = Url::parse(&format!("http://{}/", listener.local_addr()?))?;
        let server = std::thread::spawn(move || {
            if let Ok((mut socket, _)) = listener.accept() {
                let mut request = [0; 4096];
                let _ = socket.read(&mut request);
                let _ = socket.write_all(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 5\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        });
        let control = ProviderControl::new(tokio_util::sync::CancellationToken::new());
        let request = SearchRequest::default();
        let runtime = ProviderRuntime::for_request(
            ProviderPolicy::conservative(0, 1),
            &request,
            SourceKind::TwoGis,
        );
        let result = TwoGisHtmlProvider::new()?
            .fetch_controlled(&runtime, &control, &url)
            .await;
        assert!(matches!(
            result,
            Err(AppError {
                kind: ErrorKind::RateLimited,
                ..
            })
        ));
        assert!(control.is_cancelled());
        assert!(server.join().is_ok());
        Ok(())
    }

    #[test]
    fn discovers_unique_firm_links() -> Result<(), Box<dyn std::error::Error>> {
        let provider = TwoGisHtmlProvider::new()?;
        let html = include_str!("../tests/fixtures/search_page.html");
        let rows = provider.discover_firms(html);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, "Alpha");
        assert!(TwoGisHtmlProvider::has_next_page(html, 1));
        Ok(())
    }

    #[test]
    fn repeated_results_stop_even_with_stale_has_more_flag()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = TwoGisHtmlProvider::new()?;
        let html = include_str!("../tests/fixtures/repeated_page.html");
        let first = provider.discover_firms(html);
        let seen = first
            .iter()
            .map(|row| row.0.clone())
            .collect::<HashSet<_>>();
        let repeated = provider.discover_firms(html);
        let new = repeated.iter().filter(|row| !seen.contains(&row.0)).count();
        assert!(TwoGisHtmlProvider::has_next_page(html, 6));
        assert_eq!(
            TwoGisHtmlProvider::page_stop_reason(repeated.len(), new, true),
            Some(StopReason::NoNewCandidates)
        );
        assert_eq!(
            TwoGisHtmlProvider::page_stop_reason(first.len(), first.len(), true),
            None
        );
        Ok(())
    }

    #[test]
    fn search_url_uses_page_segment() -> Result<(), Box<dyn std::error::Error>> {
        let provider = TwoGisHtmlProvider::new()?;
        let request = SearchRequest {
            region: "moscow".into(),
            query: "автосервис".into(),
            ..SearchRequest::default()
        };
        assert_eq!(
            provider.search_url(&request, 1)?.as_str(),
            "https://2gis.ru/moscow/search/%D0%B0%D0%B2%D1%82%D0%BE%D1%81%D0%B5%D1%80%D0%B2%D0%B8%D1%81"
        );
        assert!(
            provider
                .search_url(&request, 2)?
                .as_str()
                .ends_with("/page/2")
        );
        Ok(())
    }
}
