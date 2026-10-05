use std::collections::HashSet;

use chrono::Utc;
use serde_json::Value;
use twogis_domain::{
    AppError, CategorySuggestion, CollectionPreset, ErrorKind, Organization, ProgressPhase,
    ProviderRunState, ProviderSearchConfig, ScrapeProgress, SearchRequest, SourceAttribution,
    SourceKind,
};
use twogis_provider_core::{
    DirectoryProvider, ProgressSink, ProviderControl, ProviderOutput, ProviderRuntime, StopReason,
};
use url::Url;

use crate::TwoGisHtmlProvider;

const API_BASE: &str = "https://catalog.api.2gis.com";
const PAGE_SIZE: u8 = 10;
const DEMO_MAX_PAGE: u8 = 5;
const MAX_REQUESTS_PER_SEARCH: u16 = 100;

pub(super) async fn search_categories(
    provider: &TwoGisHtmlProvider,
    api_key: &str,
    region: &str,
    query: &str,
) -> Result<Vec<CategorySuggestion>, AppError> {
    let mut api_config =
        ProviderSearchConfig::recommended(SourceKind::TwoGis, CollectionPreset::Gentle);
    api_config.request_delay_ms = 250;
    let search = SearchRequest {
        region: region.to_owned(),
        request_delay_ms: 250,
        provider_configs: vec![api_config],
        ..SearchRequest::default()
    };
    let runtime = ProviderRuntime::for_request(provider.policy(), &search, SourceKind::TwoGis);
    let control = ProviderControl::new(tokio_util::sync::CancellationToken::new());
    let region = lookup_region(provider, &runtime, &control, api_key, region).await?;
    let mut url = Url::parse(&format!("{API_BASE}/2.0/catalog/rubric/search")).map_err(|_| {
        AppError::new(
            ErrorKind::Internal,
            "invalid 2GIS Categories API URL",
            false,
        )
    })?;
    url.query_pairs_mut()
        .append_pair("key", api_key)
        .append_pair("q", query.trim())
        .append_pair("region_id", &region.id)
        .append_pair("page_size", "20")
        .append_pair("locale", "ru_RU");
    let data = request_json(provider, &runtime, &control, url).await?;
    Ok(data["result"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some(CategorySuggestion {
                id: value_as_string(&item["id"])?,
                name: item["name"].as_str()?.to_owned(),
            })
        })
        .collect())
}

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
    let region = lookup_region(provider, &runtime, &control, api_key, &request.region).await?;
    let rubric_id = match request.category.as_deref() {
        Some(category) => Some(
            lookup_category(provider, &runtime, &control, api_key, &region.id, category).await?,
        ),
        None => None,
    };
    // Demo keys allow only pages 1..=5 per geographic query. Each bounded
    // polygon is a separate official Places API query, so max_pages is the
    // total request budget across the region grid.
    let page_limit = config.max_pages.min(MAX_REQUESTS_PER_SEARCH);
    let mut output = ProviderOutput::default();
    let mut seen = HashSet::new();
    let polygons = region
        .bounds
        .as_deref()
        .and_then(polygon_grid)
        .unwrap_or_default();
    let use_region_query = polygons.is_empty();
    if use_region_query {
        output.warnings.push(
            "2GIS не вернул границы региона; использован обычный поиск, демо-ключ ограничивает его 50 карточками".into(),
        );
    }
    let areas: Vec<Option<String>> = if use_region_query {
        vec![None]
    } else {
        polygons.into_iter().map(Some).collect()
    };
    let mut requests = 0_u16;
    'areas: for (area_index, polygon) in areas.iter().enumerate() {
        for page in 1..=DEMO_MAX_PAGE {
            if requests >= page_limit {
                output.stop_reason = Some(StopReason::MaxPages);
                break 'areas;
            }
            if control.is_cancelled() {
                return Err(AppError::cancelled());
            }
            requests += 1;
            progress(ScrapeProgress {
                phase: ProgressPhase::Discovering,
                current: u32::from(requests),
                total: Some(u32::from(page_limit)),
                message: format!("2GIS API: область {}, страница {page}", area_index + 1),
                source: Some(SourceKind::TwoGis),
                region: Some(request.region.clone()),
                state: Some(ProviderRunState::Running),
                retry_after_seconds: None,
            });
            let mut url = Url::parse(&format!("{API_BASE}/3.0/items"))
                .map_err(|_| AppError::new(ErrorKind::Internal, "invalid 2GIS API URL", false))?;
            {
                let mut pairs = url.query_pairs_mut();
                pairs
                    .append_pair("key", api_key)
                    .append_pair("region_id", &region.id)
                    .append_pair("type", "branch")
                    .append_pair("page", &page.to_string())
                    .append_pair("page_size", &PAGE_SIZE.to_string())
                    .append_pair(
                        "fields",
                        "items.point,items.address,items.rubrics,items.reviews,items.contact_groups",
                    );
                if let Some(polygon) = polygon {
                    pairs.append_pair("polygon", polygon);
                }
                if let Some(rubric_id) = &rubric_id {
                    pairs.append_pair("rubric_id", rubric_id);
                } else {
                    pairs.append_pair("q", request.query.trim());
                }
            }
            let data = match request_json(provider, &runtime, &control, url).await {
                Ok(data) => data,
                Err(error) if page > 1 && error.kind == ErrorKind::Validation => {
                    output.warnings.push(error.message);
                    break;
                }
                Err(error) => return Err(error),
            };
            let items = data["result"]["items"]
                .as_array()
                .ok_or_else(|| AppError::parse("2GIS Places API response has no result.items"))?;
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
                }
                if output.organizations.len() >= config.max_results as usize {
                    output.stop_reason = Some(StopReason::MaxResults);
                    break 'areas;
                }
            }
            if items.is_empty()
                || data["result"]["total"]
                    .as_u64()
                    .is_some_and(|total| u64::from(page) * u64::from(PAGE_SIZE) >= total)
            {
                break;
            }
        }
    }
    if requests >= page_limit && output.stop_reason.is_none() {
        output.stop_reason = Some(StopReason::MaxPages);
    }
    output.candidate_count = output.organizations.len().try_into().unwrap_or(u32::MAX);
    Ok(output)
}

async fn lookup_category(
    provider: &TwoGisHtmlProvider,
    runtime: &ProviderRuntime,
    control: &ProviderControl,
    api_key: &str,
    region_id: &str,
    category: &str,
) -> Result<String, AppError> {
    let mut url = Url::parse(&format!("{API_BASE}/2.0/catalog/rubric/search")).map_err(|_| {
        AppError::new(
            ErrorKind::Internal,
            "invalid 2GIS Categories API URL",
            false,
        )
    })?;
    url.query_pairs_mut()
        .append_pair("key", api_key)
        .append_pair("q", category)
        .append_pair("region_id", region_id)
        .append_pair("page_size", "50")
        .append_pair("locale", "ru_RU");
    let data = request_json(provider, runtime, control, url).await?;
    data["result"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(category))
        })
        .and_then(|item| value_as_string(&item["id"]))
        .ok_or_else(|| {
            AppError::validation(format!("Категория 2ГИС «{category}» не найдена в регионе"))
        })
}

async fn lookup_region(
    provider: &TwoGisHtmlProvider,
    runtime: &ProviderRuntime,
    control: &ProviderControl,
    api_key: &str,
    region: &str,
) -> Result<RegionInfo, AppError> {
    let query = region.replace(['-', '_'], " ");
    let mut url = Url::parse(&format!("{API_BASE}/2.0/region/search"))
        .map_err(|_| AppError::new(ErrorKind::Internal, "invalid 2GIS Regions API URL", false))?;
    url.query_pairs_mut()
        .append_pair("key", api_key)
        .append_pair("q", &query)
        .append_pair("locale", "ru_RU")
        .append_pair("fields", "items.bounds");
    let data = request_json(provider, runtime, control, url).await?;
    data["result"]["items"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| {
            Some(RegionInfo {
                id: value_as_string(&item["id"])?,
                bounds: item["bounds"].as_str().map(str::to_owned),
            })
        })
        .filter(|region| !region.id.is_empty())
        .ok_or_else(|| AppError::validation(format!("2GIS API не нашёл регион «{region}»")))
}

struct RegionInfo {
    id: String,
    bounds: Option<String>,
}

fn polygon_grid(bounds: &str) -> Option<Vec<String>> {
    let normalized: String = bounds
        .chars()
        .map(|character| match character {
            '(' | ')' | ',' => ' ',
            other => other,
        })
        .collect();
    let coordinates: Vec<f64> = normalized
        .split_whitespace()
        .filter_map(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .collect();
    let pairs = coordinates.chunks_exact(2);
    let mut min_lon = f64::INFINITY;
    let mut max_lon = f64::NEG_INFINITY;
    let mut min_lat = f64::INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    for pair in pairs {
        let (lon, lat) = (pair[0], pair[1]);
        if !(-180.0..=180.0).contains(&lon) || !(-90.0..=90.0).contains(&lat) {
            continue;
        }
        min_lon = min_lon.min(lon);
        max_lon = max_lon.max(lon);
        min_lat = min_lat.min(lat);
        max_lat = max_lat.max(lat);
    }
    if !min_lon.is_finite() || max_lon - min_lon < 0.001 || max_lat - min_lat < 0.001 {
        return None;
    }

    // 2 km square cells cover ~4 km², below the Places API polygon limit.
    let lat_step = 2.0 / 111.32;
    let middle_lat = (min_lat + max_lat) / 2.0;
    let middle_lon = (min_lon + max_lon) / 2.0;
    let mut cells = Vec::new();
    let mut bottom = min_lat;
    while bottom < max_lat && cells.len() < 20_000 {
        let top = (bottom + lat_step).min(max_lat);
        let center_lat = (bottom + top) / 2.0;
        let lon_step = 2.0 / (111.32 * center_lat.to_radians().cos().abs().max(0.2));
        let mut left = min_lon;
        while left < max_lon && cells.len() < 20_000 {
            let right = (left + lon_step).min(max_lon);
            let center_lon = (left + right) / 2.0;
            let distance = (center_lat - middle_lat).powi(2) + (center_lon - middle_lon).powi(2);
            let polygon = format!(
                "POLYGON(({left} {bottom},{right} {bottom},{right} {top},{left} {top},{left} {bottom}))"
            );
            cells.push((distance, polygon));
            left = right;
        }
        bottom = top;
    }
    cells.sort_by(|left, right| left.0.total_cmp(&right.0));
    Some(cells.into_iter().map(|(_, polygon)| polygon).collect())
}

async fn request_json(
    provider: &TwoGisHtmlProvider,
    runtime: &ProviderRuntime,
    control: &ProviderControl,
    url: Url,
) -> Result<Value, AppError> {
    provider.wait_for_api_slot(control).await?;
    runtime
        .run(control, async {
            let response = provider
                .http
                .client()
                .get(url)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await
                .map_err(|error| {
                    let cause = if error.is_timeout() {
                        "таймаут соединения"
                    } else if error.is_connect() {
                        "не удалось установить соединение"
                    } else if error.is_request() {
                        "ошибка формирования HTTP-запроса"
                    } else {
                        "ошибка HTTP-транспорта"
                    };
                    // reqwest errors normally include the full URL, which has
                    // the API key in its query string. Strip it before
                    // surfacing the underlying transport cause in the UI.
                    let detail = error.without_url();
                    let causes =
                        std::iter::successors(Some(&detail as &dyn std::error::Error), |cause| {
                            cause.source()
                        })
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(": ");
                    AppError::network(format!("2GIS API: {cause} ({causes})"))
                })?;
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
                provider.defer_api_requests(retry_after.unwrap_or(60)).await;
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
    use super::{parse_api_item, polygon_grid};
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

    #[test]
    fn region_bounds_split_into_small_api_polygons_nearest_center_first() {
        let polygons = polygon_grid("POLYGON((37.0 55.0,37.1 55.0,37.1 55.1,37.0 55.1,37.0 55.0))")
            .expect("valid region bounds should produce polygons");
        assert!(polygons.len() > 10);
        assert!(polygons[0].starts_with("POLYGON(("));
        assert!(polygon_grid("POLYGON((37.0 55.0,37.0 55.0,37.0 55.0))").is_none());
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
        config.max_results = 10;
        config.max_pages = 1;
        let request = SearchRequest {
            region: "moscow".into(),
            query: "Кафе".into(),
            category: Some("Кафе".into()),
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
        assert!(output.organizations.len() <= 10);
        assert!(output.organizations.iter().all(|row| {
            !row.name.is_empty()
                && row.source_url.starts_with("https://2gis.ru/firm/")
                && row
                    .category
                    .as_deref()
                    .is_some_and(|value| value.contains("Кафе"))
        }));
        Ok(())
    }

    #[tokio::test]
    #[ignore = "live 2GIS demo API smoke test; set DGIS_TEST_API_KEY locally"]
    async fn live_demo_categories_api_returns_rubrics() -> Result<(), Box<dyn std::error::Error>> {
        let key = std::env::var("DGIS_TEST_API_KEY")?;
        let provider =
            crate::TwoGisHtmlProvider::new()?.with_api_key_state(Arc::new(RwLock::new(Some(key))));
        let categories = provider.search_categories("moscow", "кафе").await?;
        assert!(!categories.is_empty());
        assert!(
            categories
                .iter()
                .all(|item| !item.id.is_empty() && !item.name.is_empty())
        );
        Ok(())
    }
}
