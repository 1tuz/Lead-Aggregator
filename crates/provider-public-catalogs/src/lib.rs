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
    ProviderPolicy, ProviderRuntime,
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

    fn search_url(&self, request: &SearchRequest, page: u16) -> Result<Url, AppError> {
        let mut url = self
            .base
            .join(&format!("{}/top/", request.region))
            .map_err(|e| AppError::parse(format!("bad Yell URL: {e}")))?;
        url.query_pairs_mut()
            .append_pair("text", request.query.trim())
            .append_pair("page", &page.to_string());
        Ok(url)
    }

    fn discover(&self, request: &SearchRequest, html: &str) -> Vec<Url> {
        discover_links(&self.base, html, |url| {
            url.host_str() == Some("www.yell.ru")
                && url.path().contains(&format!("/{}/com/", request.region))
        })
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
        let candidates = collect_candidates(
            self,
            request,
            progress.clone(),
            control.clone(),
            runtime.clone(),
            |provider, page| provider.search_url(request, page),
            |provider, html| provider.discover(request, html),
        )
        .await?;
        enrich_candidates(self, request, candidates, progress, control, runtime).await
    }
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

    fn search_url(&self, request: &SearchRequest, page: u16) -> Result<Url, AppError> {
        let mut url = self
            .base
            .join("search/")
            .map_err(|e| AppError::parse(format!("bad Zoon URL: {e}")))?;
        url.query_pairs_mut()
            .append_pair("city", &request.region)
            .append_pair("query", request.query.trim())
            .append_pair("page", &page.to_string());
        Ok(url)
    }

    fn discover(&self, html: &str) -> Vec<Url> {
        discover_links(&self.base, html, |url| {
            if url.host_str() != Some("zoon.ru") {
                return false;
            }
            let segments = url
                .path_segments()
                .map(|segments| segments.filter(|s| !s.is_empty()).collect::<Vec<_>>())
                .unwrap_or_default();
            if segments.len() != 3 {
                return false;
            }
            !matches!(segments[0], "search" | "article" | "promo")
                && !matches!(segments[2], "reviews" | "price" | "type" | "award")
        })
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
        let candidates = collect_candidates(
            self,
            request,
            progress.clone(),
            control.clone(),
            runtime.clone(),
            |provider, page| provider.search_url(request, page),
            |provider, html| provider.discover(html),
        )
        .await?;
        enrich_candidates(self, request, candidates, progress, control, runtime).await
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

    fn search_url(&self, request: &SearchRequest, page: u16) -> Result<Url, AppError> {
        let mut url = self
            .base
            .join("search")
            .map_err(|e| AppError::parse(format!("bad Rusprofile URL: {e}")))?;
        url.query_pairs_mut()
            .append_pair("query", request.query.trim())
            .append_pair("page", &page.to_string());
        Ok(url)
    }

    fn discover(&self, html: &str) -> Vec<Url> {
        discover_links(&self.base, html, |url| {
            url.host_str() == Some("www.rusprofile.ru")
                && Regex::new(r"^/id/\d+/?$")
                    .ok()
                    .is_some_and(|re| re.is_match(url.path()))
        })
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
        let candidates = collect_candidates(
            self,
            request,
            progress.clone(),
            control.clone(),
            runtime.clone(),
            |provider, page| provider.search_url(request, page),
            |provider, html| provider.discover(html),
        )
        .await?;
        enrich_candidates(self, request, candidates, progress, control, runtime).await
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

async fn collect_candidates<P, SearchUrl, Discover>(
    provider: &P,
    request: &SearchRequest,
    progress: ProgressSink,
    control: ProviderControl,
    runtime: ProviderRuntime,
    search_url: SearchUrl,
    discover: Discover,
) -> Result<Vec<Url>, AppError>
where
    P: PublicCatalog,
    SearchUrl: Fn(&P, u16) -> Result<Url, AppError>,
    Discover: Fn(&P, &str) -> Vec<Url>,
{
    let config = request.config_for(provider.source());
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

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
        let before = candidates.len();
        for url in found {
            if seen.insert(url.to_string()) {
                candidates.push(url);
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
    Ok(candidates)
}

async fn enrich_candidates<P: PublicCatalog>(
    provider: &P,
    request: &SearchRequest,
    candidates: Vec<Url>,
    progress: ProgressSink,
    control: ProviderControl,
    runtime: ProviderRuntime,
) -> Result<ProviderOutput, AppError> {
    let total = candidates.len() as u32;
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

    let mut output = ProviderOutput::default();
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
    fn search_urls_are_source_specific_and_bounded_to_one_page()
    -> Result<(), Box<dyn std::error::Error>> {
        let request = SearchRequest {
            region: "moscow".into(),
            query: "стоматолог".into(),
            ..SearchRequest::default()
        };
        let yell = YellHtmlProvider::new()?;
        let zoon = ZoonHtmlProvider::new()?;
        let rusprofile = RusprofileHtmlProvider::new()?;
        assert_eq!(
            yell.search_url(&request, 1)?.as_str(),
            "https://www.yell.ru/moscow/top/?text=%D1%81%D1%82%D0%BE%D0%BC%D0%B0%D1%82%D0%BE%D0%BB%D0%BE%D0%B3&page=1"
        );
        assert_eq!(
            zoon.search_url(&request, 1)?
                .query_pairs()
                .find(|(key, _)| key == "page")
                .map(|(_, value)| value.into_owned())
                .as_deref(),
            Some("1")
        );
        assert_eq!(
            rusprofile
                .search_url(&request, 1)?
                .query_pairs()
                .find(|(key, _)| key == "query")
                .map(|(_, value)| value.into_owned())
                .as_deref(),
            Some("стоматолог")
        );
        Ok(())
    }

    #[test]
    fn company_discovery_skips_articles_categories_and_reviews()
    -> Result<(), Box<dyn std::error::Error>> {
        let request = SearchRequest {
            region: "moscow".into(),
            ..SearchRequest::default()
        };
        let yell = YellHtmlProvider::new()?;
        let zoon = ZoonHtmlProvider::new()?;
        let rusprofile = RusprofileHtmlProvider::new()?;
        let yell_html = r#"<a href="/moscow/com/clinic_1/">Clinic</a><a href="/moscow/blog/article/">Article</a><a href="/moscow/top/">Category</a>"#;
        assert_eq!(yell.discover(&request, yell_html).len(), 1);
        let zoon_html = r#"<a href="/moscow/beauty/clinic/">Clinic</a><a href="/article/123/news/">Article</a><a href="/moscow/clinic/reviews/">Reviews</a><a href="/search/category/page/">Category</a>"#;
        assert_eq!(zoon.discover(zoon_html).len(), 1);
        let rusprofile_html = r#"<a href="/id/12345">Company</a><a href="/search/company">Search</a><a href="/articles/12345">Article</a>"#;
        assert_eq!(rusprofile.discover(rusprofile_html).len(), 1);
        Ok(())
    }

    #[test]
    fn parses_public_profile_and_requisites() -> Result<(), Box<dyn std::error::Error>> {
        let html = r#"
          <html><body>
            <h1>ООО Ромашка</h1>
            <div>Россия, г Москва, ул Тестовая, д 1</div>
            <a href="tel:+79991234567">+7 999 123-45-67</a>
            <a href="https://romashka.example">romashka.example</a>
            <div>ИНН: 7701234567</div><div>ОГРН: 1027700123456</div>
          </body></html>
        "#;
        let url = Url::parse("https://www.yell.ru/moscow/com/romashka_1/")?;
        let row = parse_public_profile(SourceKind::Yell, &url, html)?;
        assert_eq!(row.name, "ООО Ромашка");
        assert_eq!(row.inn.as_deref(), Some("7701234567"));
        assert_eq!(row.ogrn.as_deref(), Some("1027700123456"));
        assert_eq!(row.sources[0].source, SourceKind::Yell);
        Ok(())
    }
}
