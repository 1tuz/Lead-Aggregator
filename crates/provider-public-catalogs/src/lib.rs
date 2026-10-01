use std::{collections::HashSet, time::Duration};

use async_trait::async_trait;
use chrono::Utc;
use regex::Regex;
use reqwest::{Client, StatusCode};
use scraper::{Html, Selector};
use tokio_util::sync::CancellationToken;
use twogis_domain::{
    AppError, ErrorKind, Organization, ProgressPhase, ScrapeProgress, SearchRequest, SourceKind,
};
use twogis_provider_core::{DirectoryProvider, ProgressSink, ProviderOutput, ProviderPolicy};
use url::Url;

#[derive(Clone)]
struct SafeHttp {
    client: Client,
}

impl SafeHttp {
    fn new() -> Result<Self, AppError> {
        let client = Client::builder()
            .user_agent("lead-aggregator/0.2 (+local desktop app; public HTML only)")
            .timeout(Duration::from_secs(25))
            .build()
            .map_err(|e| AppError::network(format!("failed to create HTTP client: {e}")))?;
        Ok(Self { client })
    }

    async fn fetch(&self, source: SourceKind, url: &Url) -> Result<String, AppError> {
        let response = self
            .client
            .get(url.clone())
            .header("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.5")
            .send()
            .await
            .map_err(|e| {
                AppError::network(format!("{} request failed for {url}: {e}", source.label()))
            })?;

        match response.status() {
            StatusCode::TOO_MANY_REQUESTS => {
                return Err(AppError::new(
                    ErrorKind::RateLimited,
                    format!(
                        "{} returned HTTP 429; provider paused without bypass attempts",
                        source.label()
                    ),
                    true,
                ));
            }
            StatusCode::FORBIDDEN => {
                return Err(AppError::new(
                    ErrorKind::Blocked,
                    format!(
                        "{} returned HTTP 403; no anti-bot bypass is attempted",
                        source.label()
                    ),
                    true,
                ));
            }
            status if !status.is_success() => {
                return Err(AppError::network(format!(
                    "{} returned HTTP {status} for {url}",
                    source.label()
                )));
            }
            _ => {}
        }

        let body = response
            .text()
            .await
            .map_err(|e| AppError::network(format!("failed to read {url}: {e}")))?;
        let lower = body.to_lowercase();
        if lower.contains("captcha")
            || lower.contains("подтвердите, что вы не робот")
            || lower.contains("проверка безопасности")
        {
            return Err(AppError::new(
                ErrorKind::Blocked,
                format!(
                    "{} requested an anti-bot check; provider stopped",
                    source.label()
                ),
                true,
            ));
        }
        Ok(body)
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
        ProviderPolicy::conservative(1_000, 2)
    }

    async fn search(
        &self,
        request: &SearchRequest,
        progress: ProgressSink,
        cancel: CancellationToken,
    ) -> Result<ProviderOutput, AppError> {
        let candidates = collect_candidates(
            self,
            request,
            progress.clone(),
            cancel.clone(),
            |provider, page| provider.search_url(request, page),
            |provider, html| provider.discover(request, html),
        )
        .await?;
        enrich_candidates(self, request, candidates, progress, cancel).await
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
        ProviderPolicy::conservative(1_500, 1)
    }

    async fn search(
        &self,
        request: &SearchRequest,
        progress: ProgressSink,
        cancel: CancellationToken,
    ) -> Result<ProviderOutput, AppError> {
        let candidates = collect_candidates(
            self,
            request,
            progress.clone(),
            cancel.clone(),
            |provider, page| provider.search_url(request, page),
            |provider, html| provider.discover(html),
        )
        .await?;
        enrich_candidates(self, request, candidates, progress, cancel).await
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
        ProviderPolicy::conservative(2_500, 1)
    }

    async fn search(
        &self,
        request: &SearchRequest,
        progress: ProgressSink,
        cancel: CancellationToken,
    ) -> Result<ProviderOutput, AppError> {
        let candidates = collect_candidates(
            self,
            request,
            progress.clone(),
            cancel.clone(),
            |provider, page| provider.search_url(request, page),
            |provider, html| provider.discover(html),
        )
        .await?;
        enrich_candidates(self, request, candidates, progress, cancel).await
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
    cancel: CancellationToken,
    search_url: SearchUrl,
    discover: Discover,
) -> Result<Vec<Url>, AppError>
where
    P: PublicCatalog,
    SearchUrl: Fn(&P, u16) -> Result<Url, AppError>,
    Discover: Fn(&P, &str) -> Vec<Url>,
{
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    let delay = request.request_delay_ms.max(provider.policy().min_delay_ms);

    for page in 1..=request.max_pages {
        if cancel.is_cancelled() {
            return Err(AppError::cancelled());
        }
        (progress)(ScrapeProgress {
            phase: ProgressPhase::Discovering,
            current: u32::from(page),
            total: Some(u32::from(request.max_pages)),
            message: format!("{}: страница {page}", provider.source().label()),
        });
        let url = search_url(provider, page)?;
        let html = provider.http().fetch(provider.source(), &url).await?;
        let found = discover(provider, &html);
        let before = candidates.len();
        for url in found {
            if seen.insert(url.to_string()) {
                candidates.push(url);
                if candidates.len() >= request.max_results as usize {
                    break;
                }
            }
        }
        if candidates.len() >= request.max_results as usize || candidates.len() == before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(u64::from(delay))).await;
    }
    candidates.truncate(request.max_results as usize);
    Ok(candidates)
}

async fn enrich_candidates<P: PublicCatalog>(
    provider: &P,
    request: &SearchRequest,
    candidates: Vec<Url>,
    progress: ProgressSink,
    cancel: CancellationToken,
) -> Result<ProviderOutput, AppError> {
    let total = candidates.len() as u32;
    let delay = request.request_delay_ms.max(provider.policy().min_delay_ms);
    let mut output = ProviderOutput::default();

    for (index, url) in candidates.into_iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(AppError::cancelled());
        }
        tokio::time::sleep(Duration::from_millis(u64::from(delay))).await;
        match provider.http().fetch(provider.source(), &url).await {
            Ok(html) => match parse_public_profile(provider.source(), &url, &html) {
                Ok(row) => {
                    (progress)(ScrapeProgress {
                        phase: ProgressPhase::Enriching,
                        current: index as u32 + 1,
                        total: Some(total),
                        message: format!("{}: {}", provider.source().label(), row.name),
                    });
                    output.organizations.push(row);
                }
                Err(err) => output.warnings.push(err.message),
            },
            Err(err) if matches!(err.kind, ErrorKind::RateLimited | ErrorKind::Blocked) => {
                return Err(err);
            }
            Err(err) => output.warnings.push(err.message),
        }
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
