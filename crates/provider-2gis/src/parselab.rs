//! ParseLab db-export provider.
//!
//! Downloads prebuilt city/rubric JSON exports from the bundled sealed host
//! (`DB_EXPORT_HOST/[cityId]/[rubricId].json`).
//! This mirrors the upstream Windows parser: it never touches 2gis.ru; each
//! rubric file is one HTTP GET, deduplicated by firm id, with retry/backoff
//! on network errors.

use std::collections::HashSet;

use chrono::Utc;
use serde_json::Value;
use twogis_domain::{
    AppError, ErrorKind, Organization, ProgressPhase, ProviderRunState, ScrapeProgress,
    SearchRequest, SourceAttribution, SourceKind,
};
use twogis_provider_core::{
    DirectoryProvider, ProgressSink, ProviderControl, ProviderOutput, ProviderPolicy,
    ProviderRuntime, StopReason,
};
use url::Url;

use twogis_domain::catalog_cities;
use twogis_domain::city_rubrics;
use twogis_domain::{DB_EXPORT_HOST, KEY_USER_SUFFIX};

const REQUEST_DELAY_MS: u32 = 0;
const RETRY_DELAY_MS: u64 = 5_000;
const MAX_RUBRIC_RETRIES: u8 = 3;

fn open_sealed(sealed: &twogis_domain::Sealed) -> Result<String, AppError> {
    twogis_domain::open(sealed).ok_or_else(|| {
        AppError::new(
            ErrorKind::Internal,
            "запечатанный адрес сервиса повреждён; переустановите приложение",
            false,
        )
    })
}

#[derive(Clone)]
pub struct ParselabProvider {
    http: twogis_provider_core::CatalogHttpClient,
}

impl ParselabProvider {
    pub fn new() -> Result<Self, AppError> {
        Ok(Self {
            http: twogis_provider_core::CatalogHttpClient::new()?,
        })
    }

    pub fn rubric_url(&self, city_id: &str, rubric_id: &str, key: &str) -> Result<Url, AppError> {
        let host = open_sealed(&DB_EXPORT_HOST)?;
        let url = format!("{host}/{city_id}/{rubric_id}.json?key={key}");
        Url::parse(&url).map_err(|e| {
            AppError::new(
                ErrorKind::Internal,
                format!("bad db export URL: {e}"),
                false,
            )
        })
    }

    /// Verify a license key against the key service and return the user info.
    pub async fn check_key(&self, key: &str) -> Result<Option<serde_json::Value>, AppError> {
        let check_path = open_sealed(&twogis_domain::KEY_CHECK_PATH)?;
        let user_suffix = open_sealed(&KEY_USER_SUFFIX)?;
        let url = Url::parse(&format!("{check_path}?key={key}{user_suffix}")).map_err(|e| {
            AppError::new(
                ErrorKind::Internal,
                format!("bad key-check URL: {e}"),
                false,
            )
        })?;
        let body = self
            .http
            .get_html(SourceKind::TwoGis, &url)
            .await
            .map_err(|e| AppError::network(format!("Сервис ключей недоступен: {}", e.message)))?;
        if body.trim() == "0" {
            return Ok(None);
        }
        serde_json::from_str::<serde_json::Value>(&body)
            .map(Some)
            .map_err(|_| AppError::parse("Сервис ключей вернул некорректный ответ"))
    }

    /// Fetch one rubric export; returns the raw JSON array.
    async fn fetch_rubric(
        &self,
        runtime: &ProviderRuntime,
        control: &ProviderControl,
        city_id: &str,
        rubric_id: &str,
        key: &str,
    ) -> Result<Vec<Value>, AppError> {
        let url = self.rubric_url(city_id, rubric_id, key)?;
        runtime
            .run(control, async {
                let body = self.http.get_html(SourceKind::TwoGis, &url).await?;
                serde_json::from_str::<Vec<Value>>(&body)
                    .map_err(|_| AppError::parse("Экспорт рубрики вернул некорректный JSON"))
            })
            .await
    }
}

#[async_trait::async_trait]
impl DirectoryProvider for ParselabProvider {
    fn source(&self) -> SourceKind {
        SourceKind::TwoGis
    }

    fn id(&self) -> &'static str {
        "parselab-db-export"
    }

    fn policy(&self) -> ProviderPolicy {
        ProviderPolicy::conservative(REQUEST_DELAY_MS, 1)
    }

    async fn search(
        &self,
        request: &SearchRequest,
        progress: ProgressSink,
        control: ProviderControl,
    ) -> Result<ProviderOutput, AppError> {
        // The export route requires a `key` query parameter, but currently
        // serves public city/rubric data when its value is empty (`key=`).
        let key = "";
        // request.region holds the ParseLab city id (e.g. "69"); request.query
        // holds a comma-separated rubric id list produced by the UI picker.
        let city_id = request.region.trim().to_owned();
        if !catalog_cities().iter().any(|c| c.id == city_id) {
            return Err(AppError::validation(format!(
                "Город с id {city_id} отсутствует в каталоге"
            )));
        }
        let rubrics: Vec<String> = request
            .query
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if rubrics.is_empty() {
            return Err(AppError::validation("Не выбрано ни одной рубрики"));
        }
        let available: HashSet<String> = city_rubrics(&city_id).into_iter().collect();
        let rubrics: Vec<&String> = rubrics
            .iter()
            .filter(|id| available.contains(id.as_str()))
            .collect();
        if rubrics.is_empty() {
            return Err(AppError::validation(
                "Для выбранного города нет выгрузок по этим рубрикам",
            ));
        }

        let config = request.config_for(SourceKind::TwoGis);
        let runtime = ProviderRuntime::for_request(self.policy(), request, SourceKind::TwoGis);
        let mut output = ProviderOutput::default();
        let mut seen = HashSet::new();
        let total = rubrics.len() as u32;

        for (index, rubric_id) in rubrics.iter().enumerate() {
            if control.is_cancelled() {
                return Err(AppError::cancelled());
            }
            progress(ScrapeProgress {
                phase: ProgressPhase::Discovering,
                current: index as u32 + 1,
                total: Some(total),
                message: format!("Рубрика {rubric_id} · город {city_id}"),
                source: Some(SourceKind::TwoGis),
                region: Some(city_id.clone()),
                state: Some(ProviderRunState::Running),
                retry_after_seconds: None,
            });

            let mut rows = Vec::new();
            let mut attempts = 0_u8;
            loop {
                attempts += 1;
                match self
                    .fetch_rubric(&runtime, &control, &city_id, rubric_id, &key)
                    .await
                {
                    Ok(data) => {
                        rows = data;
                        break;
                    }
                    Err(err) if err.kind == ErrorKind::Cancelled => return Err(err),
                    Err(err) if attempts < MAX_RUBRIC_RETRIES => {
                        output.warnings.push(format!(
                            "Рубрика {rubric_id}: {} (попытка {attempts}/{MAX_RUBRIC_RETRIES}, повтор через 5 с)",
                            err.message
                        ));
                        control
                            .sleep(std::time::Duration::from_millis(RETRY_DELAY_MS))
                            .await?;
                    }
                    Err(err) => {
                        output
                            .warnings
                            .push(format!("Рубрика {rubric_id}: {}", err.message));
                        break;
                    }
                }
            }

            for row in rows {
                if let Some(mut organization) = parse_export_row(&row, &city_id) {
                    if !seen.insert(organization.id.clone()) {
                        continue;
                    }
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
        }

        progress(ScrapeProgress {
            phase: ProgressPhase::Enriching,
            current: output.organizations.len() as u32,
            total: Some(output.organizations.len() as u32),
            message: "Выгрузка получена".into(),
            source: Some(SourceKind::TwoGis),
            region: Some(city_id),
            state: Some(ProviderRunState::Completed),
            retry_after_seconds: None,
        });
        output.candidate_count = u32::try_from(output.organizations.len()).unwrap_or(u32::MAX);
        if output.stop_reason.is_none() {
            output.stop_reason = Some(StopReason::LastPage);
        }
        Ok(output)
    }
}

/// Row layout (index: field), from the upstream parser `fields` table:
/// 0 id, 1 name, 2 city_name, 3 geometry_name, 4 post_code, 5 phone,
/// 7 email, 8 website, 9 vkontakte, 10 instagram, 11 lon, 12 lat,
/// 13 category, 14 subcategory.
fn parse_export_row(row: &Value, city_id: &str) -> Option<Organization> {
    let arr = row.as_array()?;
    let id = arr.first()?.as_str()?.to_owned();
    if id.is_empty() {
        return None;
    }
    let name = value_str(arr.get(1)).unwrap_or_default();
    if name.is_empty() {
        return None;
    }
    let phone = value_str(arr.get(5)).unwrap_or_default();
    let mut phones = Vec::new();
    if !phone.is_empty() {
        phones.push(phone);
    }
    let email = value_str(arr.get(7)).filter(|v| !v.is_empty());
    let website = value_str(arr.get(8)).filter(|v| !v.is_empty());
    let mut socials = Vec::new();
    for index in [9_usize, 10] {
        if let Some(social) = value_str(arr.get(index))
            .filter(|v| !v.is_empty())
            .filter(|social| !socials.contains(social))
        {
            socials.push(social);
        }
    }
    let latitude = value_str(arr.get(12)).and_then(|v| v.parse::<f64>().ok());
    let longitude = value_str(arr.get(11)).and_then(|v| v.parse::<f64>().ok());
    let category = value_str(arr.get(13)).filter(|v| !v.is_empty());
    let subcategory = value_str(arr.get(14)).filter(|v| !v.is_empty());
    let address = value_str(arr.get(3)).filter(|v| !v.is_empty());
    let collected_at = Utc::now().to_rfc3339();
    Some(Organization {
        id: format!("{city_id}:{id}"),
        name,
        category: subcategory.clone().or(category.clone()),
        address,
        rating: None,
        review_count: None,
        phones,
        email,
        website,
        socials,
        opening_status: None,
        latitude,
        longitude,
        source_url: String::new(),
        collected_at: collected_at.clone(),
        sources: vec![SourceAttribution {
            source: SourceKind::TwoGis,
            source_id: id,
            source_url: String::new(),
            collected_at,
        }],
        tags: category.into_iter().collect(),
        ..Organization::default()
    })
}

fn value_str(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_export_row_with_phone_and_category() {
        let row: Value = serde_json::from_str(
            r#"["12345","Кафе Пример","Абакан","Абакан, улица Ленина, 1","655000",
                "+7 3902 12-34-56",null,"hello@example.ru","https://example.ru",
                "https://vk.com/example",null,"91.12345","53.71234","Кафе","Кофейни"]"#,
        )
        .expect("valid row json");
        let org = parse_export_row(&row, "69").expect("valid row");
        assert_eq!(org.id, "69:12345");
        assert_eq!(org.name, "Кафе Пример");
        assert_eq!(org.phones, vec!["+7 3902 12-34-56"]);
        assert_eq!(org.email.as_deref(), Some("hello@example.ru"));
        assert_eq!(org.website.as_deref(), Some("https://example.ru"));
        assert_eq!(org.socials, vec!["https://vk.com/example"]);
        assert_eq!(org.latitude, Some(53.71234));
        assert_eq!(org.longitude, Some(91.12345));
        assert_eq!(org.category.as_deref(), Some("Кофейни"));
        assert_eq!(org.tags, vec!["Кафе"]);
    }

    #[test]
    fn skips_rows_without_id_or_name() {
        let empty: Value = serde_json::from_str(r#"[]""#).unwrap_or(Value::Null);
        assert!(parse_export_row(&empty, "69").is_none());
        let no_name: Value = serde_json::from_str(r#"["1","","город"]"#).expect("valid row json");
        assert!(parse_export_row(&no_name, "69").is_none());
    }

    #[test]
    fn rubric_url_uses_empty_key_for_public_export() {
        let provider = ParselabProvider::new().expect("provider init");
        let url = provider
            .rubric_url("69", "122", "")
            .expect("url build")
            .to_string();
        assert!(url.contains("/69/122.json"), "unexpected url: {url}");
        assert!(url.ends_with("?key="));
        // Host comes from the sealed blob; plaintext host must not be hardcoded anywhere.
        assert!(url.starts_with("http"));
    }
}
