use std::collections::HashSet;

use chrono::Utc;
use serde_json::Value;
use twogis_domain::{
    AppError, ErrorKind, Organization, ProgressPhase, ProviderRunState, ScrapeProgress,
    SearchRequest, SourceAttribution, SourceKind,
};
use twogis_provider_core::{
    DirectoryProvider, ProgressSink, ProviderControl, ProviderOutput, ProviderRuntime, StopReason,
};
use url::Url;

use crate::TwoGisHtmlProvider;

const API_BASE: &str = "https://catalog.api.2gis.com";
const PAGE_SIZE: u8 = 10;

pub(super) async fn search(
    provider: &TwoGisHtmlProvider,
    api_key: &str,
    request: &SearchRequest,
    progress: ProgressSink,
    control: ProviderControl,
) -> Result<ProviderOutput, AppError> {
    request.validate()?;
    let config = request.config_for(SourceKind::TwoGis);
    let runtime = ProviderRuntime::for_request(provider.policy(), request, SourceKind::TwoGis);
    let region_id = lookup_region(provider, &runtime, &control, api_key, &request.region).await?;
    let page_limit = config.max_pages;
    let mut output = ProviderOutput::default();
    let mut seen = HashSet::new();

    for page in 1..=page_limit {
        if control.is_cancelled() {
            return Err(AppError::cancelled());
        }
        progress(ScrapeProgress {
            phase: ProgressPhase::Discovering,
            current: u32::from(page),
            total: Some(u32::from(page_limit)),
            message: format!("2GIS API: страница {page}"),
            source: Some(SourceKind::TwoGis),
            region: Some(request.region.clone()),
            state: Some(ProviderRunState::Running),
            retry_after_seconds: None,
        });
        let mut url = Url::parse(&format!("{API_BASE}/3.0/items"))
            .map_err(|_| AppError::new(ErrorKind::Internal, "invalid 2GIS API URL", false))?;
        url.query_pairs_mut()
            .append_pair("key", api_key)
            .append_pair("q", request.query.trim())
            .append_pair("region_id", &region_id)
            .append_pair("type", "branch")
            .append_pair("page", &page.to_string())
            .append_pair("page_size", &PAGE_SIZE.to_string())
            .append_pair(
                "fields",
                "items.point,items.address,items.rubrics,items.reviews,items.contact_groups",
            );
        let data = match request_json(provider, &runtime, &control, url).await {
            Ok(data) => data,
            Err(error) if page > 1 && error.kind == ErrorKind::Validation => {
                output.warnings.push(error.message);
                output.stop_reason = Some(StopReason::MaxPages);
                break;
            }
            Err(error) => return Err(error),
        };
        let items = data["result"]["items"]
            .as_array()
            .ok_or_else(|| AppError::parse("2GIS Places API response has no result.items"))?;
        let before = output.organizations.len();
        for item in items {
            let Some(id) = value_as_string(&item["id"]) else {
                continue;
            };
            if id.is_empty() || !seen.insert(id.clone()) {
                continue;
            }
            let source_url = format!("https://2gis.ru/firm/{id}");
            if let Some(mut organization) =
                parse_api_item(item, &source_url, &Utc::now().to_rfc3339())
            {
                organization.attach_source(SourceKind::TwoGis);
                output.organizations.push(organization);
                if output.organizations.len() >= config.max_results as usize {
                    break;
                }
            }
        }
        if output.organizations.len() >= config.max_results as usize {
            output.stop_reason = Some(StopReason::MaxResults);
            break;
        }
        if items.is_empty() {
            output.stop_reason = Some(StopReason::EmptyPage);
            break;
        }
        if data["result"]["total"]
            .as_u64()
            .is_some_and(|total| u64::from(page) * u64::from(PAGE_SIZE) >= total)
        {
            output.stop_reason = Some(StopReason::LastPage);
            break;
        }
        if output.organizations.len() == before {
            output.stop_reason = Some(StopReason::NoNewCandidates);
            break;
        }
        if page == page_limit {
            output.stop_reason = Some(StopReason::MaxPages);
        }
    }
    output.candidate_count = output.organizations.len().try_into().unwrap_or(u32::MAX);
    Ok(output)
}

async fn lookup_region(
    provider: &TwoGisHtmlProvider,
    runtime: &ProviderRuntime,
    control: &ProviderControl,
    api_key: &str,
    region: &str,
) -> Result<String, AppError> {
    let query = region.replace(['-', '_'], " ");
    let mut url = Url::parse(&format!("{API_BASE}/2.0/region/search"))
        .map_err(|_| AppError::new(ErrorKind::Internal, "invalid 2GIS Regions API URL", false))?;
    url.query_pairs_mut()
        .append_pair("key", api_key)
        .append_pair("q", &query)
        .append_pair("locale", "ru_RU");
    let data = request_json(provider, runtime, control, url).await?;
    data["result"]["items"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| value_as_string(&item["id"]))
        .filter(|id| !id.is_empty())
        .ok_or_else(|| AppError::validation(format!("2GIS API не нашёл регион «{region}»")))
}

async fn request_json(
    provider: &TwoGisHtmlProvider,
    runtime: &ProviderRuntime,
    control: &ProviderControl,
    url: Url,
) -> Result<Value, AppError> {
    runtime
        .run(control, async {
            let response = provider
                .http
                .client()
                .get(url)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await
                .map_err(|_| AppError::network("2GIS API HTTP-запрос завершился ошибкой"))?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            let body = response
                .text()
                .await
                .map_err(|_| AppError::network("Не удалось прочитать ответ 2GIS API"))?;
            let data: Value = serde_json::from_str(&body)
                .map_err(|_| AppError::parse("2GIS API вернул некорректный JSON"))?;
            let code = data["meta"]["code"]
                .as_u64()
                .unwrap_or(u64::from(status.as_u16())) as u16;
            if code == 429 {
                return Err(AppError::new(
                    ErrorKind::RateLimited,
                    "2GIS API ограничил частоту запросов (429)",
                    true,
                )
                .with_retry_after(retry_after));
            }
            if code == 403 {
                return Err(AppError::new(
                    ErrorKind::Blocked,
                    "2GIS API отклонил ключ или доступ (403)",
                    false,
                ));
            }
            if code == 400 {
                let detail = data["meta"]["error"]["message"]
                    .as_str()
                    .unwrap_or_default();
                if detail.starts_with("Length of parameter 'page' should be from 1 to ") {
                    return Err(AppError::validation(
                        "2GIS API: достигнут доступный предел страниц для этого ключа",
                    ));
                }
                return Err(AppError::network(
                    "2GIS API отклонил параметры запроса (400)",
                ));
            }
            if !(200..300).contains(&code) {
                return Err(AppError::network(format!(
                    "2GIS API вернул HTTP/API {code}"
                )));
            }
            Ok(data)
        })
        .await
}

fn value_as_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|number| number.to_string()))
}

fn parse_api_item(item: &Value, source_url: &str, collected_at: &str) -> Option<Organization> {
    let id = item["id"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| item["id"].as_u64().map(|value| value.to_string()))?;
    let name = item["name"].as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let mut phones = Vec::new();
    let mut email = None;
    let mut website = None;
    let mut socials = Vec::new();
    for contact in item["contact_groups"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| group["contacts"].as_array().into_iter().flatten())
    {
        let Some(value) = contact["value"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let website_address = contact["url"].as_str().unwrap_or(value).trim();
        match contact["type"].as_str() {
            Some("phone") => {
                if !phones.iter().any(|phone| phone == value) {
                    phones.push(value.to_owned());
                }
            }
            Some("email") => {
                email.get_or_insert_with(|| value.to_owned());
            }
            Some("website")
                if website.is_none()
                    && !website_address.is_empty()
                    && !website_address.contains("link.2gis.ru") =>
            {
                website = Some(website_address.to_owned());
            }
            Some("website") => {}
            Some("vkontakte" | "telegram" | "whatsapp" | "youtube" | "instagram" | "facebook")
                if !socials.iter().any(|social| social == value) =>
            {
                socials.push(value.to_owned());
            }
            Some("vkontakte" | "telegram" | "whatsapp" | "youtube" | "instagram" | "facebook") => {}
            _ => {}
        }
    }
    let address = item["address_name"]
        .as_str()
        .or_else(|| item["address"]["name"].as_str())
        .map(str::to_owned);
    let latitude = item["point"]["lat"].as_f64();
    let longitude = item["point"]["lon"].as_f64();
    let category = item["rubrics"]
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row["name"].as_str())
        .map(str::to_owned);
    let rating = item["reviews"]["general_rating"].as_f64();
    let review_count = item["reviews"]["general_review_count"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok());
    Some(Organization {
        id: id.clone(),
        name: name.to_owned(),
        category,
        address,
        rating,
        review_count,
        phones,
        email,
        website,
        socials,
        latitude,
        longitude,
        source_url: source_url.to_owned(),
        collected_at: collected_at.to_owned(),
        sources: vec![SourceAttribution {
            source: SourceKind::TwoGis,
            source_id: id,
            source_url: source_url.to_owned(),
            collected_at: collected_at.to_owned(),
        }],
        ..Organization::default()
    })
}

#[cfg(test)]
mod tests {
    use super::parse_api_item;
    use std::sync::{Arc, RwLock};
    use tokio_util::sync::CancellationToken;
    use twogis_domain::{CollectionPreset, ProviderSearchConfig, SearchRequest, SourceKind};
    use twogis_provider_core::{DirectoryProvider, ProgressSink, ProviderControl};

    #[test]
    fn parses_official_places_api_item_into_organization() {
        let item = serde_json::json!({
            "id": "70000001099900001",
            "type": "branch",
            "name": "Кафе Пример",
            "address_name": "Москва, Тверская улица, 1",
            "point": { "lat": 55.75, "lon": 37.61 },
            "rubrics": [{ "name": "Кафе" }],
            "reviews": { "general_rating": 4.7, "general_review_count": 123 },
            "contact_groups": [{ "contacts": [
                { "type": "phone", "value": "+74951234567" },
                { "type": "website", "value": "https://example.ru" },
                { "type": "email", "value": "hello@example.ru" }
            ] }]
        });

        let organization = parse_api_item(
            &item,
            "https://2gis.ru/firm/70000001099900001",
            "2026-10-05T00:00:00Z",
        )
        .expect("valid Places API item should parse");
        assert_eq!(organization.id, "70000001099900001");
        assert_eq!(organization.name, "Кафе Пример");
        assert_eq!(
            organization.address.as_deref(),
            Some("Москва, Тверская улица, 1")
        );
        assert_eq!(organization.category.as_deref(), Some("Кафе"));
        assert_eq!(organization.rating, Some(4.7));
        assert_eq!(organization.review_count, Some(123));
        assert_eq!(organization.phones, vec!["+74951234567"]);
        assert_eq!(organization.website.as_deref(), Some("https://example.ru"));
        assert_eq!(organization.email.as_deref(), Some("hello@example.ru"));
        assert_eq!(organization.latitude, Some(55.75));
        assert_eq!(organization.longitude, Some(37.61));
        assert_eq!(organization.sources[0].source, SourceKind::TwoGis);
    }

    #[tokio::test]
    #[ignore = "live 2GIS demo API smoke test; set DGIS_TEST_API_KEY locally"]
    async fn live_demo_places_api_search_returns_parsed_organizations()
    -> Result<(), Box<dyn std::error::Error>> {
        let key = std::env::var("DGIS_TEST_API_KEY")?;
        let provider =
            crate::TwoGisHtmlProvider::new()?.with_api_key_state(Arc::new(RwLock::new(Some(key))));
        let mut config =
            ProviderSearchConfig::recommended(SourceKind::TwoGis, CollectionPreset::Gentle);
        config.max_results = 60;
        config.max_pages = 6;
        let request = SearchRequest {
            region: "moscow".into(),
            query: "кафе".into(),
            sources: vec![SourceKind::TwoGis],
            provider_configs: vec![config],
            ..SearchRequest::default()
        };
        let progress: ProgressSink = Arc::new(|_| {});
        let output = provider
            .search(
                &request,
                progress,
                ProviderControl::new(CancellationToken::new()),
            )
            .await?;
        assert!(!output.organizations.is_empty());
        assert_eq!(output.organizations.len(), 50);
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning.contains("предел страниц"))
        );
        assert!(output.organizations.iter().all(|row| {
            !row.name.is_empty() && row.source_url.starts_with("https://2gis.ru/firm/")
        }));
        Ok(())
    }
}
