use std::{collections::HashSet, time::Duration};

use async_trait::async_trait;
use chrono::Utc;
use futures::{StreamExt, stream};
use regex::Regex;
use scraper::{Html, Selector};
use twogis_domain::{
    AppError, ErrorKind, Organization, ProgressPhase, ProviderRunState, ScrapeProgress,
    SearchRequest, SourceKind,
};
use twogis_provider_core::{
    CatalogHttpClient, DirectoryProvider, ProgressSink, ProviderControl, ProviderOutput,
    ProviderPolicy, ProviderRuntime, StopReason, source_region_slug,
};
use url::Url;

#[derive(Clone)]
struct SafeHttp {
    client: CatalogHttpClient,
}

impl SafeHttp {
    fn new() -> Result<Self, AppError> {
        Ok(Self {
            client: CatalogHttpClient::new()?,
        })
    }

    async fn fetch(&self, source: SourceKind, url: &Url) -> Result<String, AppError> {
        self.client.get_html(source, url).await
    }

    async fn fetch_with_backoff(
        &self,
        source: SourceKind,
        request: &SearchRequest,
        runtime: &ProviderRuntime,
        control: &ProviderControl,
        progress: &ProgressSink,
        url: &Url,
    ) -> Result<String, AppError> {
        let config = request.config_for(source);
        for attempt in 0..=config.max_retries {
            match runtime.run(control, self.fetch(source, url)).await {
                Ok(body) => return Ok(body),
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
                        message: format!("{} rate limit: retry in {delay}s", source.label()),
                        source: Some(source),
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
        Err(AppError::network(format!(
            "{} retry loop exhausted",
            source.label()
        )))
    }
}

#[derive(Clone)]
pub struct YellHtmlProvider {
    http: SafeHttp,
    base: Url,
}

impl YellHtmlProvider {
    pub fn new() -> Result<Self, AppError> {
        Ok(Self {
            http: SafeHttp::new()?,
            base: Url::parse("https://www.yell.ru/")
                .map_err(|e| AppError::new(ErrorKind::Internal, e.to_string(), false))?,
        })
    }

    pub fn search_url(&self, request: &SearchRequest, page: u16) -> Result<Url, AppError> {
        let region = source_region_slug(SourceKind::Yell, &request.region);
        let mut url = self
            .base
            .join(&format!("{region}/top/"))
            .map_err(|e| AppError::parse(format!("bad Yell URL: {e}")))?;
        url.query_pairs_mut()
            .append_pair("text", request.query.trim())
            .append_pair("page", &page.to_string());
        Ok(url)
    }

    pub fn discover(&self, request: &SearchRequest, html: &str) -> Vec<Url> {
        let region = source_region_slug(SourceKind::Yell, &request.region);
        let pattern = format!(r"^/{region}/com/[^/]+/?$");
        let Ok(re) = Regex::new(&pattern) else {
            return Vec::new();
        };
        discover_links(&self.base, html, |url| {
            url.host_str() == Some("www.yell.ru") && re.is_match(url.path())
        })
    }

    pub fn has_next_page(html: &str, page: u16) -> bool {
        let next = page.saturating_add(1);
        html.contains(&format!("page={next}")) || html.contains(&format!("page={next}&"))
    }
}

#[async_trait]
impl DirectoryProvider for YellHtmlProvider {
    fn source(&self) -> SourceKind {
        SourceKind::Yell
    }

    fn id(&self) -> &'static str {
        "yell-public-html"
    }

    fn policy(&self) -> ProviderPolicy {
        ProviderPolicy::conservative(2_000, 1)
    }

    async fn search(
        &self,
        request: &SearchRequest,
        progress: ProgressSink,
        control: ProviderControl,
    ) -> Result<ProviderOutput, AppError> {
        let runtime = ProviderRuntime::for_request(self.policy(), request, SourceKind::Yell);
        let (candidates, stop_reason) = collect_candidates(
            self,
            request,
            progress.clone(),
            control.clone(),
            runtime.clone(),
            |provider, page| provider.search_url(request, page),
            |provider, html| provider.discover(request, html),
            |html, page| YellHtmlProvider::has_next_page(html, page),
        )
        .await?;
        enrich_candidates(
            self,
            request,
            candidates,
            stop_reason,
            progress,
            control,
            runtime,
        )
        .await
    }
}

#[derive(Clone, Debug)]
pub enum ZoonListing {
    Category { city: String, category: String },
    Search,
}

#[derive(Clone)]
pub struct ZoonHtmlProvider {
    http: SafeHttp,
    base: Url,
}

impl ZoonHtmlProvider {
    pub fn new() -> Result<Self, AppError> {
        Ok(Self {
            http: SafeHttp::new()?,
            base: Url::parse("https://zoon.ru/")
                .map_err(|e| AppError::new(ErrorKind::Internal, e.to_string(), false))?,
        })
    }

    pub fn city_slug(request: &SearchRequest) -> String {
        source_region_slug(SourceKind::Zoon, &request.region)
    }

    pub fn search_probe_url(&self, request: &SearchRequest) -> Result<Url, AppError> {
        let mut url = self
            .base
            .join("search/")
            .map_err(|e| AppError::parse(format!("bad Zoon URL: {e}")))?;
        url.query_pairs_mut()
            .append_pair("city", &Self::city_slug(request))
            .append_pair("query", request.query.trim())
            .append_pair("page", "1");
        Ok(url)
    }

    pub fn listing_url(
        &self,
        listing: &ZoonListing,
        request: &SearchRequest,
        page: u16,
    ) -> Result<Url, AppError> {
        match listing {
            ZoonListing::Search => {
                let mut url = self.search_probe_url(request)?;
                // Live check: page>1 on /search/ returns the same HTML. Keep page param only for page 1.
                if page > 1 {
                    url.query_pairs_mut().clear();
                    url.query_pairs_mut()
                        .append_pair("city", &Self::city_slug(request))
                        .append_pair("query", request.query.trim())
                        .append_pair("page", &page.to_string());
                }
                Ok(url)
            }
            ZoonListing::Category { city, category } => {
                let path = if page <= 1 {
                    format!("{city}/{category}/")
                } else {
                    format!("{city}/{category}/page-{page}/")
                };
                self.base
                    .join(&path)
                    .map_err(|e| AppError::parse(format!("bad Zoon category URL: {e}")))
            }
        }
    }

    pub fn detect_listing(&self, request: &SearchRequest, html: &str) -> ZoonListing {
        let city = Self::city_slug(request);
        let document = Html::parse_document(html);
        let Ok(selector) = Selector::parse("a[href]") else {
            return ZoonListing::Search;
        };
        let mut counts = std::collections::HashMap::<String, usize>::new();
        for anchor in document.select(&selector) {
            let Some(href) = anchor.value().attr("href") else {
                continue;
            };
            let Ok(url) = self.base.join(href) else {
                continue;
            };
            let segments = url
                .path_segments()
                .map(|segments| segments.filter(|s| !s.is_empty()).collect::<Vec<_>>())
                .unwrap_or_default();
            if segments.len() >= 2 && segments[0] == city {
                let category = segments[1];
                if matches!(
                    category,
                    "search" | "article" | "promo" | "pages" | "award" | "user"
                ) {
                    continue;
                }
                *counts.entry(category.to_owned()).or_default() += 1;
            }
        }
        counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .filter(|(_, count)| *count >= 3)
            .map(|(category, _)| ZoonListing::Category { city, category })
            .unwrap_or(ZoonListing::Search)
    }

    pub fn discover(&self, request: &SearchRequest, html: &str) -> Vec<Url> {
        let city = Self::city_slug(request);
        discover_links(&self.base, html, |url| {
            if url.host_str() != Some("zoon.ru") {
                return false;
            }
            let segments = url
                .path_segments()
                .map(|segments| segments.filter(|s| !s.is_empty()).collect::<Vec<_>>())
                .unwrap_or_default();
            if segments.len() != 3 || segments[0] != city {
                return false;
            }
            if segments[2].starts_with("page-") {
                return false;
            }
            !matches!(
                segments[1],
                "search" | "article" | "promo" | "build" | "pages"
            ) && !matches!(segments[2], "reviews" | "price" | "type" | "award")
        })
    }

    pub fn has_next_page(listing: &ZoonListing, html: &str, page: u16) -> bool {
        match listing {
            ZoonListing::Search => false,
            ZoonListing::Category { city, category } => {
                let next = page.saturating_add(1);
                html.contains(&format!("/{city}/{category}/page-{next}/"))
                    || html.contains(&format!("/{city}/{category}/page-{next}\""))
            }
        }
    }
}

#[async_trait]
impl DirectoryProvider for ZoonHtmlProvider {
    fn source(&self) -> SourceKind {
        SourceKind::Zoon
    }

    fn id(&self) -> &'static str {
        "zoon-public-html"
    }

    fn policy(&self) -> ProviderPolicy {
        ProviderPolicy::conservative(3_000, 1)
    }

    async fn search(
        &self,
        request: &SearchRequest,
        progress: ProgressSink,
        control: ProviderControl,
    ) -> Result<ProviderOutput, AppError> {
        let runtime = ProviderRuntime::for_request(self.policy(), request, SourceKind::Zoon);
        let config = request.config_for(SourceKind::Zoon);

        (progress)(ScrapeProgress {
            phase: ProgressPhase::Discovering,
            current: 0,
            total: Some(u32::from(config.max_pages)),
            message: "Zoon: resolve listing".into(),
            source: Some(SourceKind::Zoon),
            region: Some(request.region.clone()),
            state: Some(ProviderRunState::Running),
            retry_after_seconds: None,
        });

        let probe_url = self.search_probe_url(request)?;
        let probe_html = self
            .http
            .fetch_with_backoff(
                SourceKind::Zoon,
                request,
                &runtime,
                &control,
                &progress,
                &probe_url,
            )
            .await?;
        let listing = self.detect_listing(request, &probe_html);

        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        let mut stop_reason = StopReason::MaxPages;

        // Seed candidates from the probe page when listing stays on search.
        if matches!(listing, ZoonListing::Search) {
            for url in self.discover(request, &probe_html) {
                if seen.insert(url.to_string()) {
                    candidates.push(url);
                }
            }
            stop_reason = if candidates.is_empty() {
                StopReason::EmptyPage
            } else {
                // Live check: /search/?page=N does not advance results.
                StopReason::LastPage
            };
        } else {
            for page in 1..=config.max_pages {
                if control.is_cancelled() {
                    return Err(AppError::cancelled());
                }
                (progress)(ScrapeProgress {
                    phase: ProgressPhase::Discovering,
                    current: u32::from(page),
                    total: Some(u32::from(config.max_pages)),
                    message: format!("Zoon: страница {page}"),
                    source: Some(SourceKind::Zoon),
                    region: Some(request.region.clone()),
                    state: Some(ProviderRunState::Running),
                    retry_after_seconds: None,
                });
                let url = self.listing_url(&listing, request, page)?;
                let html = if page == 1 {
                    // Category page 1 differs from search probe; fetch it.
                    self.http
                        .fetch_with_backoff(
                            SourceKind::Zoon,
                            request,
                            &runtime,
                            &control,
                            &progress,
                            &url,
                        )
                        .await?
                } else {
                    self.http
                        .fetch_with_backoff(
                            SourceKind::Zoon,
                            request,
                            &runtime,
                            &control,
                            &progress,
                            &url,
                        )
                        .await?
                };
                let found = self.discover(request, &html);
                if found.is_empty() {
                    stop_reason = StopReason::EmptyPage;
                    break;
                }
                let before = candidates.len();
                for item in found {
                    if seen.insert(item.to_string()) {
                        candidates.push(item);
                        if candidates.len() >= config.max_results as usize {
                            break;
                        }
                    }
                }
                if candidates.len() >= config.max_results as usize {
                    stop_reason = StopReason::MaxResults;
                    break;
                }
                let added = candidates.len() > before;
                let has_next = Self::has_next_page(&listing, &html, page);
                if !added && !has_next {
                    stop_reason = StopReason::NoNewCandidates;
                    break;
                }
                if !has_next {
                    stop_reason = StopReason::LastPage;
                    break;
                }
                if page == config.max_pages {
                    stop_reason = StopReason::MaxPages;
                }
            }
        }

        candidates.truncate(config.max_results as usize);
        enrich_candidates(
            self,
            request,
            candidates,
            stop_reason,
            progress,
            control,
            runtime,
        )
        .await
    }
}

#[derive(Clone)]
pub struct RusprofileHtmlProvider {
    http: SafeHttp,
    base: Url,
}

impl RusprofileHtmlProvider {
    pub fn new() -> Result<Self, AppError> {
        Ok(Self {
            http: SafeHttp::new()?,
            base: Url::parse("https://www.rusprofile.ru/")
                .map_err(|e| AppError::new(ErrorKind::Internal, e.to_string(), false))?,
        })
    }

    /// Live check (2026-10-04): `/search?query=` returns HTTP 404 for text and INN.
    /// Public HTML listings still work for ТН ВЭД/code pages: `/codes/{code}` and `/codes/{code}/{page}`.
    pub fn search_url(&self, request: &SearchRequest, page: u16) -> Result<Url, AppError> {
        if let Some(code) = normalize_rusprofile_code(request.query.trim()) {
            let path = if page <= 1 {
                format!("codes/{code}")
            } else {
                format!("codes/{code}/{page}")
            };
            return self
                .base
                .join(&path)
                .map_err(|e| AppError::parse(format!("bad Rusprofile codes URL: {e}")));
        }
        let mut url = self
            .base
            .join("search")
            .map_err(|e| AppError::parse(format!("bad Rusprofile URL: {e}")))?;
        url.query_pairs_mut()
            .append_pair("query", request.query.trim());
        if page > 1 {
            url.query_pairs_mut().append_pair("page", &page.to_string());
        }
        Ok(url)
    }

    pub fn discover(&self, html: &str) -> Vec<Url> {
        discover_links(&self.base, html, |url| {
            url.host_str() == Some("www.rusprofile.ru")
                && Regex::new(r"^/id/\d+/?$")
                    .ok()
                    .is_some_and(|re| re.is_match(url.path()))
        })
    }

    pub fn has_next_page(html: &str, page: u16) -> bool {
        let next = page.saturating_add(1);
        html.contains(&format!("/{next}\""))
            || html.contains(&format!("/{next}'"))
            || html.contains(&format!("?page={next}"))
            || html.contains(&format!("rel=\"next\""))
            || html.contains("rel='next'")
    }
}

fn normalize_rusprofile_code(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if Regex::new(r"^\d{4,10}$").ok()?.is_match(trimmed) {
        return Some(trimmed.to_owned());
    }
    // Common keyword → verified public code listing (ТН ВЭД 45.2 /codes/45200000).
    let key = trimmed.to_lowercase().replace('ё', "е");
    match key.as_str() {
        "автосервис" | "автосервисы" | "сто" | "автотехцентр" | "автотехцентры" => {
            Some("45200000".into())
        }
        _ => None,
    }
}

#[async_trait]
impl DirectoryProvider for RusprofileHtmlProvider {
    fn source(&self) -> SourceKind {
        SourceKind::Rusprofile
    }

    fn id(&self) -> &'static str {
        "rusprofile-public-html"
    }

    fn policy(&self) -> ProviderPolicy {
        ProviderPolicy::conservative(4_500, 1)
    }

    async fn search(
        &self,
        request: &SearchRequest,
        progress: ProgressSink,
        control: ProviderControl,
    ) -> Result<ProviderOutput, AppError> {
        let runtime = ProviderRuntime::for_request(self.policy(), request, SourceKind::Rusprofile);
        let uses_codes = normalize_rusprofile_code(request.query.trim()).is_some();
        let (candidates, stop_reason) = collect_candidates(
            self,
            request,
            progress.clone(),
            control.clone(),
            runtime.clone(),
            |provider, page| provider.search_url(request, page),
            |provider, html| provider.discover(html),
            |html, page| {
                if uses_codes {
                    RusprofileHtmlProvider::has_next_page(html, page)
                } else {
                    // Text search endpoint is dead; never pretend page params work.
                    false
                }
            },
        )
        .await?;
        enrich_candidates(
            self,
            request,
            candidates,
            stop_reason,
            progress,
            control,
            runtime,
        )
        .await
    }
}

trait PublicCatalog: DirectoryProvider + Clone {
    fn http(&self) -> &SafeHttp;
}

impl PublicCatalog for YellHtmlProvider {
    fn http(&self) -> &SafeHttp {
        &self.http
    }
}
impl PublicCatalog for ZoonHtmlProvider {
    fn http(&self) -> &SafeHttp {
        &self.http
    }
}
impl PublicCatalog for RusprofileHtmlProvider {
    fn http(&self) -> &SafeHttp {
        &self.http
    }
}

async fn collect_candidates<P, SearchUrl, Discover, HasNext>(
    provider: &P,
    request: &SearchRequest,
    progress: ProgressSink,
    control: ProviderControl,
    runtime: ProviderRuntime,
    search_url: SearchUrl,
    discover: Discover,
    has_next: HasNext,
) -> Result<(Vec<Url>, StopReason), AppError>
where
    P: PublicCatalog,
    SearchUrl: Fn(&P, u16) -> Result<Url, AppError>,
    Discover: Fn(&P, &str) -> Vec<Url>,
    HasNext: Fn(&str, u16) -> bool,
{
    let config = request.config_for(provider.source());
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    let mut stop_reason = StopReason::MaxPages;

    for page in 1..=config.max_pages {
        if control.is_cancelled() {
            return Err(AppError::cancelled());
        }
        (progress)(ScrapeProgress {
            phase: ProgressPhase::Discovering,
            current: u32::from(page),
            total: Some(u32::from(config.max_pages)),
            message: format!("{}: страница {page}", provider.source().label()),
            source: Some(provider.source()),
            region: Some(request.region.clone()),
            state: Some(ProviderRunState::Running),
            retry_after_seconds: None,
        });
        let url = search_url(provider, page)?;
        let html = provider
            .http()
            .fetch_with_backoff(
                provider.source(),
                request,
                &runtime,
                &control,
                &progress,
                &url,
            )
            .await?;
        let found = discover(provider, &html);
        if found.is_empty() {
            stop_reason = StopReason::EmptyPage;
            break;
        }
        let before = candidates.len();
        for url in found {
            if seen.insert(url.to_string()) {
                candidates.push(url);
                if candidates.len() >= config.max_results as usize {
                    break;
                }
            }
        }
        if candidates.len() >= config.max_results as usize {
            stop_reason = StopReason::MaxResults;
            break;
        }
        let added = candidates.len() > before;
        let next = has_next(&html, page);
        if !added && !next {
            stop_reason = StopReason::NoNewCandidates;
            break;
        }
        if !next {
            stop_reason = StopReason::LastPage;
            break;
        }
        if page == config.max_pages {
            stop_reason = StopReason::MaxPages;
        }
    }
    candidates.truncate(config.max_results as usize);
    Ok((candidates, stop_reason))
}

async fn enrich_candidates<P: PublicCatalog>(
    provider: &P,
    request: &SearchRequest,
    candidates: Vec<Url>,
    stop_reason: StopReason,
    progress: ProgressSink,
    control: ProviderControl,
    runtime: ProviderRuntime,
) -> Result<ProviderOutput, AppError> {
    let candidate_count = candidates.len() as u32;
    let total = candidate_count;
    let concurrency = runtime.concurrency();
    let provider = provider.clone();
    let runtime_for_stream = runtime.clone();
    let progress_for_stream = progress.clone();
    let control_for_stream = control.clone();
    let request_for_stream = request.clone();

    let results = stream::iter(candidates.into_iter().enumerate())
        .map(move |(index, url)| {
            let provider = provider.clone();
            let runtime = runtime_for_stream.clone();
            let progress = progress_for_stream.clone();
            let control = control_for_stream.clone();
            let request = request_for_stream.clone();
            async move {
                if control.is_cancelled() {
                    return Err(AppError::cancelled());
                }
                let source = provider.source();
                let html = provider
                    .http()
                    .fetch_with_backoff(source, &request, &runtime, &control, &progress, &url)
                    .await?;
                let row = parse_public_profile(source, &url, &html)?;
                (progress)(ScrapeProgress {
                    phase: ProgressPhase::Enriching,
                    current: index as u32 + 1,
                    total: Some(total),
                    message: format!("{}: {}", source.label(), row.name),
                    source: Some(source),
                    region: Some(request.region.clone()),
                    state: Some(ProviderRunState::Running),
                    retry_after_seconds: None,
                });
                Ok::<_, AppError>(row)
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
    let mut terminal_error = None;
    for result in results {
        match result {
            Ok(row) => output.organizations.push(row),
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
    Ok(output)
}

fn discover_links(base: &Url, html: &str, predicate: impl Fn(&Url) -> bool) -> Vec<Url> {
    let document = Html::parse_document(html);
    let Ok(selector) = Selector::parse("a[href]") else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for anchor in document.select(&selector) {
        let Some(href) = anchor.value().attr("href") else {
            continue;
        };
        let Ok(mut url) = base.join(href) else {
            continue;
        };
        url.set_query(None);
        url.set_fragment(None);
        if predicate(&url) && seen.insert(url.to_string()) {
            out.push(url);
        }
    }
    out
}

fn parse_public_profile(
    source: SourceKind,
    url: &Url,
    html: &str,
) -> Result<Organization, AppError> {
    let document = Html::parse_document(html);
    let name = first_text(&document, "h1").unwrap_or_default();
    if name.is_empty() {
        return Err(AppError::parse(format!(
            "{} profile has no h1: {url}",
            source.label()
        )));
    }

    let anchors = all_anchors(&document);
    let mut phones = Vec::new();
    let mut socials = Vec::new();
    let mut email = None;
    let mut website = None;

    for anchor in anchors {
        let href = anchor.value().attr("href").unwrap_or_default().trim();
        if let Some(phone) = href.strip_prefix("tel:") {
            push_unique(&mut phones, phone.trim().to_owned());
        } else if let Some(value) = href.strip_prefix("mailto:") {
            email.get_or_insert_with(|| value.trim().to_owned());
        } else if is_social(href) {
            push_unique(&mut socials, href.to_owned());
        } else if website.is_none() && is_external_website(source, href) {
            website = Some(href.to_owned());
        }
    }

    let plain = document
        .root_element()
        .text()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let inn = capture_digits(&plain, r"(?i)ИНН\s*:?\s*(\d{10,12})");
    let ogrn = capture_digits(&plain, r"(?i)ОГРН\s*:?\s*(\d{13,15})");
    let address = first_attr(&document, "[itemprop='streetAddress']", "content")
        .or_else(|| first_text(&document, "[itemprop='streetAddress']"))
        .or_else(|| find_address_line(&plain));
    let rating = first_attr(&document, "meta[itemprop='ratingValue']", "content")
        .and_then(|value| value.replace(',', ".").parse::<f64>().ok());
    let review_count = first_attr(&document, "meta[itemprop='reviewCount']", "content")
        .and_then(|value| value.parse::<u32>().ok());
    let id = url
        .path_segments()
        .and_then(|mut parts| parts.rfind(|part| !part.is_empty()))
        .unwrap_or(url.as_str())
        .to_owned();

    let mut row = Organization {
        id,
        name,
        address,
        rating,
        review_count,
        phones,
        email,
        website,
        socials,
        inn,
        ogrn,
        source_url: url.to_string(),
        collected_at: Utc::now().to_rfc3339(),
        ..Organization::default()
    };
    row.attach_source(source);
    Ok(row)
}

fn first_text(document: &Html, selector: &str) -> Option<String> {
    let selector = Selector::parse(selector).ok()?;
    document
        .select(&selector)
        .map(|node| {
            node.text()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .find(|text| !text.is_empty())
}

fn first_attr(document: &Html, selector: &str, attr: &str) -> Option<String> {
    let selector = Selector::parse(selector).ok()?;
    document
        .select(&selector)
        .filter_map(|node| node.value().attr(attr))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_owned)
}

fn all_anchors(document: &Html) -> Vec<scraper::ElementRef<'_>> {
    let Ok(selector) = Selector::parse("a[href]") else {
        return Vec::new();
    };
    document.select(&selector).collect()
}

fn capture_digits(text: &str, pattern: &str) -> Option<String> {
    Regex::new(pattern)
        .ok()?
        .captures(text)?
        .get(1)
        .map(|m| m.as_str().to_owned())
}

fn find_address_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| {
            line.len() >= 8
                && line.len() <= 220
                && (line.starts_with("г ")
                    || line.starts_with("Россия, г ")
                    || line.contains(" ул ")
                    || line.contains(" улица "))
        })
        .map(str::to_owned)
}

fn is_social(href: &str) -> bool {
    [
        "vk.com",
        "t.me",
        "telegram.me",
        "ok.ru",
        "youtube.com",
        "rutube.ru",
    ]
    .iter()
    .any(|host| href.contains(host))
}

fn is_external_website(source: SourceKind, href: &str) -> bool {
    if !(href.starts_with("https://") || href.starts_with("http://")) {
        return false;
    }
    let own = match source {
        SourceKind::TwoGis => "2gis.ru",
        SourceKind::Yell => "yell.ru",
        SourceKind::Zoon => "zoon.ru",
        SourceKind::Rusprofile => "rusprofile.ru",
    };
    !href.contains(own) && !is_social(href)
}

fn push_unique<T: PartialEq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yell_search_url_and_strict_company_links() -> Result<(), Box<dyn std::error::Error>> {
        let request = SearchRequest {
            region: "moscow".into(),
            query: "автосервис".into(),
            ..SearchRequest::default()
        };
        let yell = YellHtmlProvider::new()?;
        assert_eq!(
            yell.search_url(&request, 2)?.as_str(),
            "https://www.yell.ru/moscow/top/?text=%D0%B0%D0%B2%D1%82%D0%BE%D1%81%D0%B5%D1%80%D0%B2%D0%B8%D1%81&page=2"
        );
        let html = include_str!("../tests/fixtures/yell_search.html");
        let found = yell.discover(&request, html);
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|url| !url.path().contains("/reviews")));
        assert!(YellHtmlProvider::has_next_page(html, 1));
        Ok(())
    }

    #[test]
    fn zoon_maps_moscow_and_detects_category_listing() -> Result<(), Box<dyn std::error::Error>> {
        let request = SearchRequest {
            region: "moscow".into(),
            query: "автосервис".into(),
            ..SearchRequest::default()
        };
        let zoon = ZoonHtmlProvider::new()?;
        let probe = zoon.search_probe_url(&request)?;
        assert!(probe.as_str().contains("city=msk"));
        assert!(!probe.as_str().contains("city=moscow"));

        let search_html = include_str!("../tests/fixtures/zoon_search.html");
        let listing = zoon.detect_listing(&request, search_html);
        assert!(matches!(
            listing,
            ZoonListing::Category { ref city, ref category }
                if city == "msk" && category == "autoservice"
        ));
        assert_eq!(zoon.discover(&request, search_html).len(), 2);

        let category_html = include_str!("../tests/fixtures/zoon_category.html");
        assert_eq!(zoon.discover(&request, category_html).len(), 2);
        assert!(ZoonHtmlProvider::has_next_page(&listing, category_html, 1));
        assert_eq!(
            zoon.listing_url(&listing, &request, 2)?.as_str(),
            "https://zoon.ru/msk/autoservice/page-2/"
        );
        Ok(())
    }

    #[test]
    fn rusprofile_uses_codes_for_autoservice_and_discovers_ids()
    -> Result<(), Box<dyn std::error::Error>> {
        let request = SearchRequest {
            region: "moscow".into(),
            query: "автосервис".into(),
            ..SearchRequest::default()
        };
        let rusprofile = RusprofileHtmlProvider::new()?;
        assert_eq!(
            rusprofile.search_url(&request, 1)?.as_str(),
            "https://www.rusprofile.ru/codes/45200000"
        );
        assert_eq!(
            rusprofile.search_url(&request, 2)?.as_str(),
            "https://www.rusprofile.ru/codes/45200000/2"
        );
        let html = include_str!("../tests/fixtures/rusprofile_codes.html");
        assert_eq!(rusprofile.discover(html).len(), 2);
        assert!(RusprofileHtmlProvider::has_next_page(html, 1));
        Ok(())
    }

    #[test]
    fn parses_public_profile_and_requisites() -> Result<(), Box<dyn std::error::Error>> {
        let html = include_str!("../tests/fixtures/profile.html");
        let url = Url::parse("https://www.yell.ru/moscow/com/romashka_1/")?;
        let row = parse_public_profile(SourceKind::Yell, &url, html)?;
        assert_eq!(row.name, "ООО Ромашка");
        assert_eq!(row.inn.as_deref(), Some("7701234567"));
        assert_eq!(row.ogrn.as_deref(), Some("1027700123456"));
        assert_eq!(row.sources[0].source, SourceKind::Yell);
        Ok(())
    }
}
