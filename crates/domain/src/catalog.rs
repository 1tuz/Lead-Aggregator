//! Bundled city/rubric catalog seeded from the ParseLab parser distribution.
//!
//! The catalog describes which `db` export files exist per city. It carries no
//! secrets: city codes, rubric ids and display names only.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCity {
    pub id: String,
    pub code: String,
    pub name: String,
    pub country_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRubric {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCategory {
    pub id: String,
    pub name: String,
    pub rubrics: Vec<CatalogRubric>,
}

fn cities_json() -> &'static str {
    include_str!("data/cities.json")
}

fn categories_json() -> &'static str {
    include_str!("data/categories.json")
}

fn rubrics_json(city_id: &str) -> Option<&'static str> {
    static RUBRICS: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    let files = RUBRICS.get_or_init(|| {
        HashMap::from([
            ("1".to_owned(), include_str!("data/rubrics/1.json")),
            ("2".to_owned(), include_str!("data/rubrics/2.json")),
            ("3".to_owned(), include_str!("data/rubrics/3.json")),
            ("4".to_owned(), include_str!("data/rubrics/4.json")),
            ("5".to_owned(), include_str!("data/rubrics/5.json")),
            ("6".to_owned(), include_str!("data/rubrics/6.json")),
            ("7".to_owned(), include_str!("data/rubrics/7.json")),
            ("8".to_owned(), include_str!("data/rubrics/8.json")),
            ("9".to_owned(), include_str!("data/rubrics/9.json")),
            ("10".to_owned(), include_str!("data/rubrics/10.json")),
            ("11".to_owned(), include_str!("data/rubrics/11.json")),
            ("12".to_owned(), include_str!("data/rubrics/12.json")),
            ("13".to_owned(), include_str!("data/rubrics/13.json")),
            ("15".to_owned(), include_str!("data/rubrics/15.json")),
            ("16".to_owned(), include_str!("data/rubrics/16.json")),
            ("17".to_owned(), include_str!("data/rubrics/17.json")),
            ("18".to_owned(), include_str!("data/rubrics/18.json")),
            ("19".to_owned(), include_str!("data/rubrics/19.json")),
            ("20".to_owned(), include_str!("data/rubrics/20.json")),
            ("21".to_owned(), include_str!("data/rubrics/21.json")),
            ("22".to_owned(), include_str!("data/rubrics/22.json")),
            ("23".to_owned(), include_str!("data/rubrics/23.json")),
            ("24".to_owned(), include_str!("data/rubrics/24.json")),
            ("25".to_owned(), include_str!("data/rubrics/25.json")),
            ("26".to_owned(), include_str!("data/rubrics/26.json")),
            ("27".to_owned(), include_str!("data/rubrics/27.json")),
            ("28".to_owned(), include_str!("data/rubrics/28.json")),
            ("29".to_owned(), include_str!("data/rubrics/29.json")),
            ("30".to_owned(), include_str!("data/rubrics/30.json")),
            ("31".to_owned(), include_str!("data/rubrics/31.json")),
            ("32".to_owned(), include_str!("data/rubrics/32.json")),
            ("33".to_owned(), include_str!("data/rubrics/33.json")),
            ("34".to_owned(), include_str!("data/rubrics/34.json")),
            ("35".to_owned(), include_str!("data/rubrics/35.json")),
            ("36".to_owned(), include_str!("data/rubrics/36.json")),
            ("37".to_owned(), include_str!("data/rubrics/37.json")),
            ("38".to_owned(), include_str!("data/rubrics/38.json")),
            ("39".to_owned(), include_str!("data/rubrics/39.json")),
            ("40".to_owned(), include_str!("data/rubrics/40.json")),
            ("41".to_owned(), include_str!("data/rubrics/41.json")),
            ("42".to_owned(), include_str!("data/rubrics/42.json")),
            ("43".to_owned(), include_str!("data/rubrics/43.json")),
            ("44".to_owned(), include_str!("data/rubrics/44.json")),
            ("45".to_owned(), include_str!("data/rubrics/45.json")),
            ("46".to_owned(), include_str!("data/rubrics/46.json")),
            ("47".to_owned(), include_str!("data/rubrics/47.json")),
            ("48".to_owned(), include_str!("data/rubrics/48.json")),
            ("49".to_owned(), include_str!("data/rubrics/49.json")),
            ("50".to_owned(), include_str!("data/rubrics/50.json")),
            ("51".to_owned(), include_str!("data/rubrics/51.json")),
            ("52".to_owned(), include_str!("data/rubrics/52.json")),
            ("53".to_owned(), include_str!("data/rubrics/53.json")),
            ("54".to_owned(), include_str!("data/rubrics/54.json")),
            ("55".to_owned(), include_str!("data/rubrics/55.json")),
            ("56".to_owned(), include_str!("data/rubrics/56.json")),
            ("57".to_owned(), include_str!("data/rubrics/57.json")),
            ("58".to_owned(), include_str!("data/rubrics/58.json")),
            ("59".to_owned(), include_str!("data/rubrics/59.json")),
            ("60".to_owned(), include_str!("data/rubrics/60.json")),
            ("61".to_owned(), include_str!("data/rubrics/61.json")),
            ("62".to_owned(), include_str!("data/rubrics/62.json")),
            ("63".to_owned(), include_str!("data/rubrics/63.json")),
            ("64".to_owned(), include_str!("data/rubrics/64.json")),
            ("65".to_owned(), include_str!("data/rubrics/65.json")),
            ("66".to_owned(), include_str!("data/rubrics/66.json")),
            ("67".to_owned(), include_str!("data/rubrics/67.json")),
            ("68".to_owned(), include_str!("data/rubrics/68.json")),
            ("69".to_owned(), include_str!("data/rubrics/69.json")),
            ("70".to_owned(), include_str!("data/rubrics/70.json")),
            ("71".to_owned(), include_str!("data/rubrics/71.json")),
            ("72".to_owned(), include_str!("data/rubrics/72.json")),
            ("73".to_owned(), include_str!("data/rubrics/73.json")),
            ("74".to_owned(), include_str!("data/rubrics/74.json")),
            ("76".to_owned(), include_str!("data/rubrics/76.json")),
            ("77".to_owned(), include_str!("data/rubrics/77.json")),
            ("78".to_owned(), include_str!("data/rubrics/78.json")),
            ("80".to_owned(), include_str!("data/rubrics/80.json")),
            ("81".to_owned(), include_str!("data/rubrics/81.json")),
            ("82".to_owned(), include_str!("data/rubrics/82.json")),
            ("83".to_owned(), include_str!("data/rubrics/83.json")),
            ("84".to_owned(), include_str!("data/rubrics/84.json")),
            ("85".to_owned(), include_str!("data/rubrics/85.json")),
            ("86".to_owned(), include_str!("data/rubrics/86.json")),
            ("87".to_owned(), include_str!("data/rubrics/87.json")),
            ("88".to_owned(), include_str!("data/rubrics/88.json")),
            ("89".to_owned(), include_str!("data/rubrics/89.json")),
            ("90".to_owned(), include_str!("data/rubrics/90.json")),
            ("91".to_owned(), include_str!("data/rubrics/91.json")),
            ("92".to_owned(), include_str!("data/rubrics/92.json")),
            ("94".to_owned(), include_str!("data/rubrics/94.json")),
            ("95".to_owned(), include_str!("data/rubrics/95.json")),
            ("96".to_owned(), include_str!("data/rubrics/96.json")),
            ("97".to_owned(), include_str!("data/rubrics/97.json")),
            ("101".to_owned(), include_str!("data/rubrics/101.json")),
            ("103".to_owned(), include_str!("data/rubrics/103.json")),
            ("106".to_owned(), include_str!("data/rubrics/106.json")),
            ("108".to_owned(), include_str!("data/rubrics/108.json")),
            ("109".to_owned(), include_str!("data/rubrics/109.json")),
            ("111".to_owned(), include_str!("data/rubrics/111.json")),
            ("112".to_owned(), include_str!("data/rubrics/112.json")),
            ("113".to_owned(), include_str!("data/rubrics/113.json")),
            ("114".to_owned(), include_str!("data/rubrics/114.json")),
            ("115".to_owned(), include_str!("data/rubrics/115.json")),
            ("116".to_owned(), include_str!("data/rubrics/116.json")),
            ("118".to_owned(), include_str!("data/rubrics/118.json")),
            ("121".to_owned(), include_str!("data/rubrics/121.json")),
            ("122".to_owned(), include_str!("data/rubrics/122.json")),
            ("124".to_owned(), include_str!("data/rubrics/124.json")),
            ("126".to_owned(), include_str!("data/rubrics/126.json")),
            ("127".to_owned(), include_str!("data/rubrics/127.json")),
            ("129".to_owned(), include_str!("data/rubrics/129.json")),
            ("131".to_owned(), include_str!("data/rubrics/131.json")),
            ("132".to_owned(), include_str!("data/rubrics/132.json")),
            ("134".to_owned(), include_str!("data/rubrics/134.json")),
            ("135".to_owned(), include_str!("data/rubrics/135.json")),
            ("137".to_owned(), include_str!("data/rubrics/137.json")),
            ("138".to_owned(), include_str!("data/rubrics/138.json")),
            ("139".to_owned(), include_str!("data/rubrics/139.json")),
            ("142".to_owned(), include_str!("data/rubrics/142.json")),
            ("143".to_owned(), include_str!("data/rubrics/143.json")),
            ("144".to_owned(), include_str!("data/rubrics/144.json")),
            ("150".to_owned(), include_str!("data/rubrics/150.json")),
            ("151".to_owned(), include_str!("data/rubrics/151.json")),
            ("152".to_owned(), include_str!("data/rubrics/152.json")),
            ("153".to_owned(), include_str!("data/rubrics/153.json")),
            ("154".to_owned(), include_str!("data/rubrics/154.json")),
            ("155".to_owned(), include_str!("data/rubrics/155.json")),
            ("158".to_owned(), include_str!("data/rubrics/158.json")),
            ("159".to_owned(), include_str!("data/rubrics/159.json")),
            ("160".to_owned(), include_str!("data/rubrics/160.json")),
            ("161".to_owned(), include_str!("data/rubrics/161.json")),
            ("162".to_owned(), include_str!("data/rubrics/162.json")),
            ("166".to_owned(), include_str!("data/rubrics/166.json")),
            ("167".to_owned(), include_str!("data/rubrics/167.json")),
            ("168".to_owned(), include_str!("data/rubrics/168.json")),
            ("169".to_owned(), include_str!("data/rubrics/169.json")),
            ("170".to_owned(), include_str!("data/rubrics/170.json")),
            ("174".to_owned(), include_str!("data/rubrics/174.json")),
            ("176".to_owned(), include_str!("data/rubrics/176.json")),
            ("177".to_owned(), include_str!("data/rubrics/177.json")),
            ("178".to_owned(), include_str!("data/rubrics/178.json")),
            ("179".to_owned(), include_str!("data/rubrics/179.json")),
            ("180".to_owned(), include_str!("data/rubrics/180.json")),
            ("181".to_owned(), include_str!("data/rubrics/181.json")),
            ("182".to_owned(), include_str!("data/rubrics/182.json")),
            ("183".to_owned(), include_str!("data/rubrics/183.json")),
            ("184".to_owned(), include_str!("data/rubrics/184.json")),
            ("185".to_owned(), include_str!("data/rubrics/185.json")),
            ("186".to_owned(), include_str!("data/rubrics/186.json")),
            ("187".to_owned(), include_str!("data/rubrics/187.json")),
            ("188".to_owned(), include_str!("data/rubrics/188.json")),
            ("189".to_owned(), include_str!("data/rubrics/189.json")),
            ("190".to_owned(), include_str!("data/rubrics/190.json")),
            ("191".to_owned(), include_str!("data/rubrics/191.json")),
            ("192".to_owned(), include_str!("data/rubrics/192.json")),
            ("193".to_owned(), include_str!("data/rubrics/193.json")),
            ("196".to_owned(), include_str!("data/rubrics/196.json")),
            ("201".to_owned(), include_str!("data/rubrics/201.json")),
            ("202".to_owned(), include_str!("data/rubrics/202.json")),
            ("203".to_owned(), include_str!("data/rubrics/203.json")),
            ("204".to_owned(), include_str!("data/rubrics/204.json")),
            ("205".to_owned(), include_str!("data/rubrics/205.json")),
            ("206".to_owned(), include_str!("data/rubrics/206.json")),
            ("207".to_owned(), include_str!("data/rubrics/207.json")),
            ("208".to_owned(), include_str!("data/rubrics/208.json")),
            ("209".to_owned(), include_str!("data/rubrics/209.json")),
            ("210".to_owned(), include_str!("data/rubrics/210.json")),
            ("213".to_owned(), include_str!("data/rubrics/213.json")),
            ("214".to_owned(), include_str!("data/rubrics/214.json")),
            ("215".to_owned(), include_str!("data/rubrics/215.json")),
            ("217".to_owned(), include_str!("data/rubrics/217.json")),
            ("218".to_owned(), include_str!("data/rubrics/218.json")),
            ("219".to_owned(), include_str!("data/rubrics/219.json")),
            ("220".to_owned(), include_str!("data/rubrics/220.json")),
            ("221".to_owned(), include_str!("data/rubrics/221.json")),
            ("222".to_owned(), include_str!("data/rubrics/222.json")),
            ("223".to_owned(), include_str!("data/rubrics/223.json")),
            ("224".to_owned(), include_str!("data/rubrics/224.json")),
            ("225".to_owned(), include_str!("data/rubrics/225.json")),
            ("226".to_owned(), include_str!("data/rubrics/226.json")),
            ("227".to_owned(), include_str!("data/rubrics/227.json")),
            ("228".to_owned(), include_str!("data/rubrics/228.json")),
            ("229".to_owned(), include_str!("data/rubrics/229.json")),
            ("230".to_owned(), include_str!("data/rubrics/230.json")),
            ("232".to_owned(), include_str!("data/rubrics/232.json")),
            ("233".to_owned(), include_str!("data/rubrics/233.json")),
            ("234".to_owned(), include_str!("data/rubrics/234.json")),
            ("235".to_owned(), include_str!("data/rubrics/235.json")),
            ("240".to_owned(), include_str!("data/rubrics/240.json")),
            ("241".to_owned(), include_str!("data/rubrics/241.json")),
            ("242".to_owned(), include_str!("data/rubrics/242.json")),
            ("243".to_owned(), include_str!("data/rubrics/243.json")),
            ("245".to_owned(), include_str!("data/rubrics/245.json")),
            ("247".to_owned(), include_str!("data/rubrics/247.json")),
            ("248".to_owned(), include_str!("data/rubrics/248.json")),
            ("250".to_owned(), include_str!("data/rubrics/250.json")),
            ("251".to_owned(), include_str!("data/rubrics/251.json")),
            ("252".to_owned(), include_str!("data/rubrics/252.json")),
            ("253".to_owned(), include_str!("data/rubrics/253.json")),
            ("255".to_owned(), include_str!("data/rubrics/255.json")),
        ])
    });
    files.get(city_id).copied()
}

fn parse_cities() -> Vec<CatalogCity> {
    #[derive(Deserialize)]
    struct Row {
        id: String,
        code: String,
        name: String,
        country_code: String,
    }
    serde_json::from_str::<Vec<Row>>(cities_json())
        .unwrap_or_default()
        .into_iter()
        .map(|row| CatalogCity {
            id: row.id,
            code: row.code,
            name: row.name,
            country_code: row.country_code,
        })
        .collect()
}

fn parse_categories() -> Vec<CatalogCategory> {
    #[derive(Deserialize)]
    struct Row {
        id: String,
        name: String,
        children: Vec<Child>,
    }
    #[derive(Deserialize)]
    struct Child {
        id: String,
        name: String,
        #[serde(default)]
        parent_id: Option<String>,
    }
    serde_json::from_str::<Vec<Row>>(categories_json())
        .unwrap_or_default()
        .into_iter()
        .map(|row| CatalogCategory {
            id: row.id,
            name: row.name,
            rubrics: row
                .children
                .into_iter()
                .map(|child| CatalogRubric {
                    id: child.id,
                    name: child.name,
                    parent_id: child.parent_id,
                })
                .collect(),
        })
        .collect()
}

/// All cities in the bundled catalog (id = ParseLab city id, e.g. "69" = Абакан).
pub fn catalog_cities() -> &'static [CatalogCity] {
    static CITIES: OnceLock<Vec<CatalogCity>> = OnceLock::new();
    CITIES.get_or_init(parse_cities)
}

/// Top-level categories with child rubrics from the bundled catalog.
pub fn catalog_categories() -> &'static [CatalogCategory] {
    static CATEGORIES: OnceLock<Vec<CatalogCategory>> = OnceLock::new();
    CATEGORIES.get_or_init(parse_categories)
}

/// Rubric ids available for a specific city (from `dat/rubrics/<id>.json`).
pub fn city_rubrics(city_id: &str) -> Vec<String> {
    rubrics_json(city_id)
        .and_then(|body| serde_json::from_str::<Vec<String>>(body).ok())
        .unwrap_or_default()
}

/// Cities that actually have at least one rubric export on the db service.
pub fn supported_cities() -> Vec<CatalogCity> {
    catalog_cities()
        .iter()
        .filter(|city| !city_rubrics(&city.id).is_empty())
        .cloned()
        .collect()
}

/// Rubric ids for a city intersected with the app's category tree,
/// paired with their display names for UI pickers.
pub fn city_rubric_suggestions(city_id: &str) -> Vec<CatalogRubric> {
    let available: std::collections::HashSet<String> = city_rubrics(city_id).into_iter().collect();
    catalog_categories()
        .iter()
        .flat_map(|category| category.rubrics.iter())
        .filter(|rubric| available.contains(rubric.id.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_abakan_demo_city() {
        let city = catalog_cities()
            .iter()
            .find(|city| city.id == "69")
            .expect("Abakan must exist in bundled cities");
        assert_eq!(city.code, "abakan");
        assert_eq!(city.name, "Абакан");
    }

    #[test]
    fn supported_cities_have_rubric_exports() {
        let supported = supported_cities();
        assert!(!supported.is_empty());
        let abakan = supported
            .iter()
            .find(|city| city.id == "69")
            .expect("Abakan must have rubric exports");
        assert!(!city_rubrics(&abakan.id).is_empty());
    }

    #[test]
    fn categories_parse_with_named_children() {
        let categories = catalog_categories();
        assert!(!categories.is_empty());
        assert!(categories.iter().all(|category| !category.name.is_empty()));
    }

    #[test]
    fn unknown_city_has_no_rubrics() {
        assert!(city_rubrics("999999999").is_empty());
        assert!(city_rubric_suggestions("999999999").is_empty());
    }
}
