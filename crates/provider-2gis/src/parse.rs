use chrono::Utc;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use twogis_domain::{AppError, Organization};

pub fn parse_firm_page(source_url: &str, html: &str) -> Result<Organization, AppError> {
    let document = Html::parse_document(html);
    let name = first_text(&document, "h1").unwrap_or_default();
    let id = extract_firm_id(source_url).unwrap_or_else(|| source_url.to_owned());
    let category = first_matching_anchor_text(&document, |href| href.contains("rubricId"));
    let address = first_matching_anchor_text(&document, |href| href.contains("/geo/"))
        .or_else(|| first_attr(&document, "meta[property='og:street-address']", "content"));

    let anchors = all_anchors(&document);
    let mut phones = Vec::new();
    let mut email = None;
    let mut website = None;
    let mut socials = Vec::new();

    for anchor in anchors {
        let href = anchor.value().attr("href").unwrap_or_default().trim();
        let label = clean_text(anchor.text());
        if let Some(phone) = href.strip_prefix("tel:") {
            push_unique(&mut phones, phone.trim().to_owned());
            continue;
        }
        if let Some(value) = href.strip_prefix("mailto:") {
            if !value.trim().is_empty() {
                email.get_or_insert_with(|| value.trim().to_owned());
            }
            continue;
        }
        if is_social(href) {
            push_unique(&mut socials, href.to_owned());
            continue;
        }
        if website.is_none() && is_website_candidate(href, &label) {
            website = Some(if href.contains("link.2gis.ru") && !label.is_empty() {
                label
            } else {
                href.to_owned()
            });
        }
    }

    if email.is_none() {
        email = extract_email(html);
    }
    let (rating, review_count) = extract_rating_and_reviews(&document);
    let opening_status = extract_opening_status(&document);
    let (latitude, longitude) = extract_coordinates(html);

    Ok(Organization {
        id,
        name,
        category,
        address,
        rating,
        review_count,
        phones,
        email,
        website,
        socials,
        opening_status,
        latitude,
        longitude,
        source_url: source_url.to_owned(),
        collected_at: Utc::now().to_rfc3339(),
        ..Organization::default()
    })
}

fn first_text(document: &Html, selector: &str) -> Option<String> {
    let selector = Selector::parse(selector).ok()?;
    document
        .select(&selector)
        .map(|node| clean_text(node.text()))
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

fn all_anchors(document: &Html) -> Vec<ElementRef<'_>> {
    let Ok(selector) = Selector::parse("a[href]") else {
        return Vec::new();
    };
    document.select(&selector).collect()
}

fn first_matching_anchor_text(document: &Html, predicate: impl Fn(&str) -> bool) -> Option<String> {
    all_anchors(document).into_iter().find_map(|anchor| {
        let href = anchor.value().attr("href")?;
        if !predicate(href) {
            return None;
        }
        let text = clean_text(anchor.text());
        (!text.is_empty()).then_some(text)
    })
}

fn clean_text<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    parts
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.is_empty() && !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn extract_firm_id(url: &str) -> Option<String> {
    let regex = Regex::new(r"/firm/(\d+)").ok()?;
    regex
        .captures(url)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().to_owned())
}

fn extract_email(html: &str) -> Option<String> {
    let regex = Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b").ok()?;
    regex.find(html).map(|m| m.as_str().to_owned())
}

fn is_social(href: &str) -> bool {
    [
        "vk.com/",
        "t.me/",
        "telegram.me/",
        "wa.me/",
        "whatsapp.com/",
        "ok.ru/",
        "youtube.com/",
        "instagram.com/",
    ]
    .iter()
    .any(|host| href.contains(host))
}

fn is_website_candidate(href: &str, label: &str) -> bool {
    if !href.starts_with("http") || is_social(href) {
        return false;
    }
    if href.contains("link.2gis.ru") {
        return label.contains('.');
    }
    if href.contains("2gis.ru") || href.contains("2gis.com") || href.contains("redirect.2gis") {
        return false;
    }
    label.contains('.') || href.starts_with("https://") || href.starts_with("http://")
}

fn extract_rating_and_reviews(document: &Html) -> (Option<f64>, Option<u32>) {
    let texts = document
        .root_element()
        .text()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    for window in texts.windows(2) {
        let rating = window[0].replace(',', ".").parse::<f64>().ok();
        if !(rating.is_some_and(|value| (0.0..=5.0).contains(&value))) {
            continue;
        }
        if !(window[1].contains("оцен") || window[1].contains("отзыв")) {
            continue;
        }
        let reviews = window[1]
            .split_whitespace()
            .find_map(|part| part.parse::<u32>().ok());
        return (rating, reviews);
    }
    (None, None)
}

fn extract_opening_status(document: &Html) -> Option<String> {
    document
        .root_element()
        .text()
        .map(str::trim)
        .find(|text| {
            matches!(*text, "Открыто" | "Закрыто")
                || text.starts_with("Открыто до")
                || text.starts_with("Закрыто до")
        })
        .map(str::to_owned)
}

fn extract_coordinates(html: &str) -> (Option<f64>, Option<f64>) {
    let lat_re = Regex::new(r#"[\"'](?:lat|latitude)[\"']\s*:\s*(-?\d{1,3}(?:\.\d+)?)"#).ok();
    let lon_re = Regex::new(r#"[\"'](?:lon|longitude)[\"']\s*:\s*(-?\d{1,3}(?:\.\d+)?)"#).ok();
    let lat = lat_re
        .and_then(|re| re.captures(html))
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<f64>().ok());
    let lon = lon_re
        .and_then(|re| re.captures(html))
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<f64>().ok());
    (lat, lon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_semantic_firm_page_without_css_hashes() -> Result<(), Box<dyn std::error::Error>> {
        let html = r#"
          <html><body>
            <h1>Тестовый сервис</h1>
            <a href="/moscow/search/service/rubricId/123">Автосервис</a>
            <div>4.8</div><div>53 оценки</div>
            <a href="/moscow/geo/123">2-я улица, 27</a>
            <div>Открыто до 21:00</div>
            <a href="tel:+74951234567">+7 495 123-45-67</a>
            <a href="mailto:hello@example.test">hello@example.test</a>
            <a href="https://example.test">example.test</a>
            <a href="https://vk.com/example">ВКонтакте</a>
            <script>{"lat":55.7,"lon":37.6}</script>
          </body></html>
        "#;
        let row = parse_firm_page("https://2gis.ru/moscow/firm/7000000001", html)?;
        assert_eq!(row.id, "7000000001");
        assert_eq!(row.name, "Тестовый сервис");
        assert_eq!(row.category.as_deref(), Some("Автосервис"));
        assert_eq!(row.address.as_deref(), Some("2-я улица, 27"));
        assert_eq!(row.rating, Some(4.8));
        assert_eq!(row.review_count, Some(53));
        assert_eq!(row.phones, vec!["+74951234567"]);
        assert_eq!(row.email.as_deref(), Some("hello@example.test"));
        assert_eq!(row.latitude, Some(55.7));
        assert_eq!(row.longitude, Some(37.6));
        Ok(())
    }
}
