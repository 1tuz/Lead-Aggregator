use std::collections::HashMap;

use twogis_domain::{DedupeInfo, Organization};
use url::Url;

#[derive(Debug, Default)]
pub struct DedupeResult {
    pub organizations: Vec<Organization>,
    pub merged_count: u32,
}

pub fn deduplicate(rows: Vec<Organization>) -> DedupeResult {
    let raw_count = rows.len() as u32;
    let mut canonical = Vec::<Organization>::new();
    let mut by_inn = HashMap::<String, usize>::new();
    let mut by_ogrn = HashMap::<String, usize>::new();
    let mut by_phone = HashMap::<String, usize>::new();
    let mut by_domain = HashMap::<String, usize>::new();
    let mut by_name_address = HashMap::<String, usize>::new();

    for mut row in rows {
        normalize_row(&mut row);
        let match_index = find_match(
            &row,
            &by_inn,
            &by_ogrn,
            &by_phone,
            &by_domain,
            &by_name_address,
        );

        let index = if let Some(index) = match_index {
            merge_into(&mut canonical[index], row);
            index
        } else {
            row.dedupe = DedupeInfo {
                merged_records: 1,
                fingerprint: fingerprint(&row),
                possible_duplicate: false,
            };
            canonical.push(row);
            canonical.len() - 1
        };
        index_row(
            &canonical[index],
            index,
            &mut by_inn,
            &mut by_ogrn,
            &mut by_phone,
            &mut by_domain,
            &mut by_name_address,
        );
    }

    mark_possible_duplicates(&mut canonical);
    canonical.sort_by_cached_key(|row| row.name.to_lowercase());
    let merged_count = raw_count.saturating_sub(canonical.len() as u32);

    DedupeResult {
        organizations: canonical,
        merged_count,
    }
}

fn find_match(
    row: &Organization,
    by_inn: &HashMap<String, usize>,
    by_ogrn: &HashMap<String, usize>,
    by_phone: &HashMap<String, usize>,
    by_domain: &HashMap<String, usize>,
    by_name_address: &HashMap<String, usize>,
) -> Option<usize> {
    row.inn
        .as_deref()
        .and_then(normalize_digits)
        .and_then(|value| by_inn.get(&value).copied())
        .or_else(|| {
            row.ogrn
                .as_deref()
                .and_then(normalize_digits)
                .and_then(|value| by_ogrn.get(&value).copied())
        })
        .or_else(|| {
            row.phones
                .iter()
                .filter_map(|phone| normalize_phone(phone))
                .find_map(|phone| by_phone.get(&phone).copied())
        })
        .or_else(|| {
            row.website
                .as_deref()
                .and_then(dedupe_domain)
                .and_then(|domain| by_domain.get(&domain).copied())
        })
        .or_else(|| {
            let key = name_address_key(row)?;
            by_name_address.get(&key).copied()
        })
}

fn index_row(
    row: &Organization,
    index: usize,
    by_inn: &mut HashMap<String, usize>,
    by_ogrn: &mut HashMap<String, usize>,
    by_phone: &mut HashMap<String, usize>,
    by_domain: &mut HashMap<String, usize>,
    by_name_address: &mut HashMap<String, usize>,
) {
    if let Some(value) = row.inn.as_deref().and_then(normalize_digits) {
        by_inn.insert(value, index);
    }
    if let Some(value) = row.ogrn.as_deref().and_then(normalize_digits) {
        by_ogrn.insert(value, index);
    }
    for phone in row.phones.iter().filter_map(|phone| normalize_phone(phone)) {
        by_phone.insert(phone, index);
    }
    if let Some(domain) = row.website.as_deref().and_then(dedupe_domain) {
        by_domain.insert(domain, index);
    }
    if let Some(key) = name_address_key(row) {
        by_name_address.insert(key, index);
    }
}

fn normalize_row(row: &mut Organization) {
    let mut normalized_phones = Vec::new();
    for phone in &row.phones {
        if let Some(phone) = normalize_phone(phone) {
            push_unique(&mut normalized_phones, phone);
        }
    }
    row.phones = normalized_phones;
    row.inn = row.inn.as_deref().and_then(normalize_digits);
    row.ogrn = row.ogrn.as_deref().and_then(normalize_digits);
    if row.dedupe.merged_records == 0 {
        row.dedupe.merged_records = 1;
    }
}

fn merge_into(target: &mut Organization, incoming: Organization) {
    target.category = target.category.take().or(incoming.category);
    target.address = target.address.take().or(incoming.address);
    target.email = target.email.take().or(incoming.email);
    target.website = target.website.take().or(incoming.website);
    target.opening_status = target.opening_status.take().or(incoming.opening_status);
    target.latitude = target.latitude.or(incoming.latitude);
    target.longitude = target.longitude.or(incoming.longitude);
    target.inn = target.inn.take().or(incoming.inn);
    target.ogrn = target.ogrn.take().or(incoming.ogrn);

    if incoming.review_count.unwrap_or(0) > target.review_count.unwrap_or(0) {
        target.rating = incoming.rating;
        target.review_count = incoming.review_count;
    }

    for phone in incoming.phones {
        push_unique(&mut target.phones, phone);
    }
    for social in incoming.socials {
        push_unique(&mut target.socials, social);
    }
    for source in incoming.sources {
        if !target.sources.contains(&source) {
            target.sources.push(source);
        }
    }
    for tag in incoming.tags {
        push_unique(&mut target.tags, tag);
    }
    for branch in incoming.branches {
        if !target.branches.contains(&branch) {
            target.branches.push(branch);
        }
    }

    target.dedupe.merged_records = target
        .dedupe
        .merged_records
        .saturating_add(incoming.dedupe.merged_records.max(1));
    target.dedupe.fingerprint = fingerprint(target);
    if target.dedupe.merged_records > 1 {
        push_unique(&mut target.tags, "Объединённый лид".to_owned());
    }
}

fn mark_possible_duplicates(rows: &mut [Organization]) {
    let mut groups = HashMap::<String, Vec<usize>>::new();
    for (index, row) in rows.iter().enumerate() {
        let name = normalize_name(&row.name);
        if name.len() >= 4 {
            groups.entry(name).or_default().push(index);
        }
    }
    for indexes in groups.values().filter(|indexes| indexes.len() > 1) {
        for index in indexes {
            rows[*index].dedupe.possible_duplicate = true;
            push_unique(&mut rows[*index].tags, "Возможный дубль".to_owned());
        }
    }
}

fn fingerprint(row: &Organization) -> String {
    if let Some(inn) = row.inn.as_deref().and_then(normalize_digits) {
        return format!("inn:{inn}");
    }
    if let Some(ogrn) = row.ogrn.as_deref().and_then(normalize_digits) {
        return format!("ogrn:{ogrn}");
    }
    if let Some(phone) = row.phones.iter().find_map(|phone| normalize_phone(phone)) {
        return format!("phone:{phone}");
    }
    if let Some(domain) = row.website.as_deref().and_then(dedupe_domain) {
        return format!("domain:{domain}");
    }
    if let Some(key) = name_address_key(row) {
        return format!("name-address:{key}");
    }
    row.sources
        .first()
        .map(|source| format!("source:{}:{}", source.source.id(), source.source_id))
        .unwrap_or_else(|| format!("source:unknown:{}", row.id))
}

fn name_address_key(row: &Organization) -> Option<String> {
    let name = normalize_name(&row.name);
    let address = normalize_text(row.address.as_deref()?);
    if name.is_empty() || address.is_empty() {
        return None;
    }
    Some(format!("{name}|{address}"))
}

fn normalize_name(value: &str) -> String {
    const LEGAL_FORMS: &[&str] = &["ооо", "ао", "пао", "ип", "зао", "оао"];
    normalize_text(value)
        .split_whitespace()
        .filter(|token| !LEGAL_FORMS.contains(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_text(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| {
            if ch.is_alphanumeric() {
                Some(ch.to_lowercase().next().unwrap_or(ch))
            } else if ch.is_whitespace() {
                Some(' ')
            } else {
                None
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_digits(value: &str) -> Option<String> {
    let digits = value
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>();
    (!digits.is_empty()).then_some(digits)
}

pub fn normalize_phone(value: &str) -> Option<String> {
    let mut digits = value
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>();
    if digits.len() == 11 && digits.starts_with('8') {
        digits.replace_range(0..1, "7");
    } else if digits.len() == 10 {
        digits.insert(0, '7');
    }
    (digits.len() >= 10).then(|| format!("+{digits}"))
}

pub fn normalize_domain(value: &str) -> Option<String> {
    let parsed = Url::parse(value)
        .or_else(|_| Url::parse(&format!("https://{value}")))
        .ok()?;
    let host = parsed.host_str()?.trim_start_matches("www.").to_lowercase();
    (!host.is_empty()).then_some(host)
}

fn dedupe_domain(value: &str) -> Option<String> {
    let host = normalize_domain(value)?;
    const SHARED_DOMAINS: &[&str] = &[
        "2gis.ru",
        "yell.ru",
        "zoon.ru",
        "rusprofile.ru",
        "vk.com",
        "t.me",
        "taplink.cc",
        "linktr.ee",
        "business.site",
    ];
    let shared = SHARED_DOMAINS.iter().any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    });
    (!shared).then_some(host)
}

fn push_unique<T: PartialEq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use twogis_domain::{SourceAttribution, SourceKind};

    fn row(id: &str, name: &str, phone: &str, source: SourceKind) -> Organization {
        Organization {
            id: id.into(),
            name: name.into(),
            phones: vec![phone.into()],
            source_url: format!("https://example.test/{id}"),
            collected_at: "2026-10-01T00:00:00Z".into(),
            sources: vec![SourceAttribution {
                source,
                source_id: id.into(),
                source_url: format!("https://example.test/{id}"),
                collected_at: "2026-10-01T00:00:00Z".into(),
            }],
            tags: vec![source.label().into()],
            ..Organization::default()
        }
    }

    #[test]
    fn merges_same_russian_phone_across_sources() {
        let result = deduplicate(vec![
            row("a", "Ромашка", "8 (999) 123-45-67", SourceKind::TwoGis),
            row("b", "ООО Ромашка", "+7 999 123 45 67", SourceKind::Yell),
        ]);
        assert_eq!(result.organizations.len(), 1);
        assert_eq!(result.merged_count, 1);
        assert_eq!(result.organizations[0].sources.len(), 2);
        assert_eq!(result.organizations[0].phones, vec!["+79991234567"]);
    }

    #[test]
    fn does_not_merge_same_name_with_different_addresses_without_strong_key() {
        let mut a = row("a", "Сеть", "", SourceKind::TwoGis);
        a.phones.clear();
        a.address = Some("ул. Первая, 1".into());
        let mut b = row("b", "Сеть", "", SourceKind::Yell);
        b.phones.clear();
        b.address = Some("ул. Вторая, 2".into());
        let result = deduplicate(vec![a, b]);
        assert_eq!(result.organizations.len(), 2);
        assert!(
            result
                .organizations
                .iter()
                .all(|row| row.dedupe.possible_duplicate)
        );
    }

    #[test]
    fn preserves_legal_form_substrings_inside_real_words() {
        assert_eq!(normalize_name("Кипарис"), "кипарис");
        assert_eq!(normalize_name("ООО Кипарис"), "кипарис");
    }

    #[test]
    fn does_not_merge_shared_profile_domains() {
        let mut a = row("shared-a", "Альфа", "", SourceKind::TwoGis);
        a.phones.clear();
        a.website = Some("https://taplink.cc/alpha".into());
        let mut b = row("shared-b", "Бета", "", SourceKind::Yell);
        b.phones.clear();
        b.website = Some("https://taplink.cc/beta".into());

        let result = deduplicate(vec![a, b]);
        assert_eq!(result.organizations.len(), 2);
    }

    #[test]
    fn merges_same_inn_and_same_website_domain() {
        let mut by_inn_a = row("a", "Альфа", "", SourceKind::TwoGis);
        by_inn_a.phones.clear();
        by_inn_a.inn = Some("7701234567".into());
        let mut by_inn_b = row("b", "Альфа ООО", "", SourceKind::Rusprofile);
        by_inn_b.phones.clear();
        by_inn_b.inn = Some("77-01234567".into());
        let inn_result = deduplicate(vec![by_inn_a, by_inn_b]);
        assert_eq!(inn_result.organizations.len(), 1);
        assert_eq!(inn_result.organizations[0].sources.len(), 2);

        let mut by_ogrn_a = row("e", "Гамма", "", SourceKind::TwoGis);
        by_ogrn_a.phones.clear();
        by_ogrn_a.ogrn = Some("1027700123456".into());
        let mut by_ogrn_b = row("f", "Гамма ООО", "", SourceKind::Rusprofile);
        by_ogrn_b.phones.clear();
        by_ogrn_b.ogrn = Some("1027700123456".into());
        assert_eq!(
            deduplicate(vec![by_ogrn_a, by_ogrn_b]).organizations.len(),
            1
        );

        let mut by_domain_a = row("c", "Бета", "", SourceKind::TwoGis);
        by_domain_a.phones.clear();
        by_domain_a.website = Some("https://www.beta.example/catalog".into());
        let mut by_domain_b = row("d", "Другая компания", "", SourceKind::Yell);
        by_domain_b.phones.clear();
        by_domain_b.website = Some("http://beta.example/contact".into());
        let domain_result = deduplicate(vec![by_domain_a, by_domain_b]);
        assert_eq!(domain_result.organizations.len(), 1);
        assert_eq!(domain_result.organizations[0].sources.len(), 2);
    }
}
