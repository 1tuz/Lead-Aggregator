use twogis_domain::SourceKind;

/// Map the shared UI/region slug to the slug expected by a concrete catalog.
///
/// Live checks (2026-10-04): Zoon treats `city=moscow` as Cherkessk and needs `msk`.
pub fn source_region_slug(source: SourceKind, region: &str) -> String {
    let region = region.trim().to_ascii_lowercase();
    match source {
        SourceKind::Zoon => zoon_city_slug(&region).to_owned(),
        SourceKind::TwoGis | SourceKind::Yell | SourceKind::Rusprofile => region,
    }
}

fn zoon_city_slug(region: &str) -> &str {
    match region {
        "moscow" | "moskva" => "msk",
        "spb" | "saint-petersburg" | "st-petersburg" => "spb",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoon_maps_moscow_to_msk() {
        assert_eq!(source_region_slug(SourceKind::Zoon, "moscow"), "msk");
        assert_eq!(source_region_slug(SourceKind::Yell, "moscow"), "moscow");
        assert_eq!(source_region_slug(SourceKind::TwoGis, "moscow"), "moscow");
    }
}
