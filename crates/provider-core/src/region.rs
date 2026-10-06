use twogis_domain::SourceKind;

/// Map the shared UI/region slug to the slug expected by a concrete catalog.
pub fn source_region_slug(source: SourceKind, region: &str) -> String {
    let _ = source;
    region.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_passes_through() {
        assert_eq!(source_region_slug(SourceKind::TwoGis, "moscow"), "moscow");
    }
}
