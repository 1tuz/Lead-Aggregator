use std::path::Path;

use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use twogis_domain::{AppError, DedupeInfo, Organization};

#[derive(Clone)]
pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    pub async fn connect(path: &Path) -> Result<Self, AppError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .map_err(|e| AppError::storage(format!("failed to open SQLite database: {e}")))?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&self) -> Result<(), AppError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS organizations (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL,
              category TEXT,
              address TEXT,
              rating REAL,
              review_count INTEGER,
              phones_json TEXT NOT NULL,
              email TEXT,
              website TEXT,
              socials_json TEXT NOT NULL,
              opening_status TEXT,
              latitude REAL,
              longitude REAL,
              source_url TEXT NOT NULL,
              collected_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::storage(format!("failed to migrate database: {e}")))?;

        for (name, definition) in [
            ("inn", "TEXT"),
            ("ogrn", "TEXT"),
            ("sources_json", "TEXT NOT NULL DEFAULT '[]'"),
            ("tags_json", "TEXT NOT NULL DEFAULT '[]'"),
            ("branches_json", "TEXT NOT NULL DEFAULT '[]'"),
            ("dedupe_json", "TEXT NOT NULL DEFAULT '{}'"),
        ] {
            self.ensure_column(name, definition).await?;
        }

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS source_records (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              source TEXT NOT NULL,
              source_id TEXT NOT NULL,
              source_url TEXT NOT NULL,
              payload_json TEXT NOT NULL,
              collected_at TEXT NOT NULL,
              UNIQUE(source, source_id, source_url)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::storage(format!("failed to create source_records: {e}")))?;

        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        sqlx::query(
            r#"
            UPDATE organizations SET
              sources_json = json_array(json_object(
                'source', 'twoGis', 'sourceId', id, 'sourceUrl', source_url, 'collectedAt', collected_at
              )),
              tags_json = '["2GIS"]',
              branches_json = CASE WHEN address IS NULL THEN '[]' ELSE json_array(json_object(
                'address', address, 'latitude', latitude, 'longitude', longitude, 'source', 'twoGis'
              )) END,
              dedupe_json = json_object(
                'mergedRecords', 1, 'fingerprint', 'source:' || id, 'possibleDuplicate', json('false')
              )
            WHERE sources_json = '[]' AND source_url LIKE '%2gis.ru/%'
            "#,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::storage(format!("failed to attribute legacy 2GIS records: {e}")))?;
        sqlx::query(
            r#"
            INSERT INTO source_records (source, source_id, source_url, payload_json, collected_at)
            SELECT '2gis', id, source_url,
              json_object(
                'id', id, 'name', name, 'category', category, 'address', address,
                'rating', rating, 'reviewCount', review_count, 'phones', json(phones_json),
                'email', email, 'website', website, 'socials', json(socials_json),
                'openingStatus', opening_status, 'latitude', latitude, 'longitude', longitude,
                'sourceUrl', source_url, 'collectedAt', collected_at, 'inn', inn, 'ogrn', ogrn,
                'sources', json(sources_json), 'tags', json(tags_json), 'branches', json(branches_json),
                'dedupe', json(dedupe_json)
              ), collected_at
            FROM organizations
            WHERE source_url LIKE '%2gis.ru/%'
              AND NOT EXISTS (
                SELECT 1 FROM source_records records WHERE records.source='2gis' AND records.source_id=organizations.id
              )
            "#,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::storage(format!("failed to backfill legacy source records: {e}")))?;
        tx.commit().await.map_err(|e| {
            AppError::storage(format!("failed to commit legacy source migration: {e}"))
        })?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_organizations_inn ON organizations(inn)")
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::storage(format!("failed to create INN index: {e}")))?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_organizations_ogrn ON organizations(ogrn)")
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::storage(format!("failed to create OGRN index: {e}")))?;
        Ok(())
    }

    async fn ensure_column(&self, name: &str, definition: &str) -> Result<(), AppError> {
        let columns = sqlx::query("PRAGMA table_info(organizations)")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::storage(format!("failed to inspect database schema: {e}")))?;
        let exists = columns.iter().any(|row| {
            row.try_get::<String, _>("name")
                .is_ok_and(|column| column == name)
        });
        if exists {
            return Ok(());
        }
        let sql = format!("ALTER TABLE organizations ADD COLUMN {name} {definition}");
        sqlx::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::storage(format!("failed to add column {name}: {e}")))?;
        Ok(())
    }

    pub async fn record_source_many(&self, rows: &[Organization]) -> Result<(), AppError> {
        let mut tx =
            self.pool.begin().await.map_err(|e| {
                AppError::storage(format!("failed to start source transaction: {e}"))
            })?;
        for row in rows {
            let payload = serde_json::to_string(row)
                .map_err(|e| AppError::storage(format!("failed to encode source row: {e}")))?;
            for source in &row.sources {
                sqlx::query(
                    r#"
                    INSERT INTO source_records (source, source_id, source_url, payload_json, collected_at)
                    VALUES (?, ?, ?, ?, ?)
                    ON CONFLICT(source, source_id, source_url) DO UPDATE SET
                      payload_json=excluded.payload_json,
                      collected_at=excluded.collected_at
                    "#,
                )
                .bind(source.source.id())
                .bind(&source.source_id)
                .bind(&source.source_url)
                .bind(&payload)
                .bind(&source.collected_at)
                .execute(&mut *tx)
                .await
                .map_err(|e| AppError::storage(format!("failed to persist source record: {e}")))?;
            }
        }
        tx.commit()
            .await
            .map_err(|e| AppError::storage(format!("failed to commit source records: {e}")))?;
        Ok(())
    }

    pub async fn upsert_many(&self, rows: &[Organization]) -> Result<(), AppError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::storage(format!("failed to start transaction: {e}")))?;

        for row in rows {
            let phones = encode(&row.phones, "phones")?;
            let socials = encode(&row.socials, "socials")?;
            let sources = encode(&row.sources, "sources")?;
            let tags = encode(&row.tags, "tags")?;
            let branches = encode(&row.branches, "branches")?;
            let dedupe = encode(&row.dedupe, "dedupe info")?;
            sqlx::query(
                r#"
                INSERT INTO organizations (
                  id, name, category, address, rating, review_count,
                  phones_json, email, website, socials_json, opening_status,
                  latitude, longitude, source_url, collected_at, inn, ogrn,
                  sources_json, tags_json, branches_json, dedupe_json
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(id) DO UPDATE SET
                  name=excluded.name,
                  category=excluded.category,
                  address=excluded.address,
                  rating=excluded.rating,
                  review_count=excluded.review_count,
                  phones_json=excluded.phones_json,
                  email=excluded.email,
                  website=excluded.website,
                  socials_json=excluded.socials_json,
                  opening_status=excluded.opening_status,
                  latitude=excluded.latitude,
                  longitude=excluded.longitude,
                  source_url=excluded.source_url,
                  collected_at=excluded.collected_at,
                  inn=excluded.inn,
                  ogrn=excluded.ogrn,
                  sources_json=excluded.sources_json,
                  tags_json=excluded.tags_json,
                  branches_json=excluded.branches_json,
                  dedupe_json=excluded.dedupe_json
                "#,
            )
            .bind(&row.id)
            .bind(&row.name)
            .bind(&row.category)
            .bind(&row.address)
            .bind(row.rating)
            .bind(row.review_count.map(i64::from))
            .bind(phones)
            .bind(&row.email)
            .bind(&row.website)
            .bind(socials)
            .bind(&row.opening_status)
            .bind(row.latitude)
            .bind(row.longitude)
            .bind(&row.source_url)
            .bind(&row.collected_at)
            .bind(&row.inn)
            .bind(&row.ogrn)
            .bind(sources)
            .bind(tags)
            .bind(branches)
            .bind(dedupe)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::storage(format!("failed to persist {}: {e}", row.name)))?;
        }

        tx.commit()
            .await
            .map_err(|e| AppError::storage(format!("failed to commit results: {e}")))?;
        Ok(())
    }

    pub async fn recent(&self, limit: u32) -> Result<Vec<Organization>, AppError> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, category, address, rating, review_count,
                   phones_json, email, website, socials_json, opening_status,
                   latitude, longitude, source_url, collected_at, inn, ogrn,
                   sources_json, tags_json, branches_json, dedupe_json
            FROM organizations
            ORDER BY collected_at DESC
            LIMIT ?
            "#,
        )
        .bind(i64::from(limit.min(5_000)))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::storage(format!("failed to load results: {e}")))?;

        rows.into_iter().map(decode_row).collect()
    }
}

fn encode<T: serde::Serialize>(value: &T, label: &str) -> Result<String, AppError> {
    serde_json::to_string(value)
        .map_err(|error| AppError::storage(format!("failed to encode {label}: {error}")))
}

fn storage_error(error: sqlx::Error) -> AppError {
    AppError::storage(error.to_string())
}

fn decode_row(row: sqlx::sqlite::SqliteRow) -> Result<Organization, AppError> {
    let phones_json: String = row
        .try_get("phones_json")
        .map_err(|e| AppError::storage(e.to_string()))?;
    let socials_json: String = row
        .try_get("socials_json")
        .map_err(|e| AppError::storage(e.to_string()))?;
    let collected_at: String = row
        .try_get("collected_at")
        .map_err(|e| AppError::storage(e.to_string()))?;
    let sources_json: String = row.try_get("sources_json").unwrap_or_else(|_| "[]".into());
    let tags_json: String = row.try_get("tags_json").unwrap_or_else(|_| "[]".into());
    let branches_json: String = row.try_get("branches_json").unwrap_or_else(|_| "[]".into());
    let dedupe_json: String = row.try_get("dedupe_json").unwrap_or_else(|_| "{}".into());

    Ok(Organization {
        id: row
            .try_get("id")
            .map_err(|e| AppError::storage(e.to_string()))?,
        name: row
            .try_get("name")
            .map_err(|e| AppError::storage(e.to_string()))?,
        category: row
            .try_get("category")
            .map_err(|e| AppError::storage(e.to_string()))?,
        address: row
            .try_get("address")
            .map_err(|e| AppError::storage(e.to_string()))?,
        rating: row
            .try_get("rating")
            .map_err(|e| AppError::storage(e.to_string()))?,
        review_count: row
            .try_get::<Option<i64>, _>("review_count")
            .map_err(|e| AppError::storage(e.to_string()))?
            .and_then(|value| u32::try_from(value).ok()),
        phones: serde_json::from_str(&phones_json).unwrap_or_default(),
        email: row
            .try_get("email")
            .map_err(|e| AppError::storage(e.to_string()))?,
        website: row
            .try_get("website")
            .map_err(|e| AppError::storage(e.to_string()))?,
        socials: serde_json::from_str(&socials_json).unwrap_or_default(),
        opening_status: row
            .try_get("opening_status")
            .map_err(|e| AppError::storage(e.to_string()))?,
        latitude: row
            .try_get("latitude")
            .map_err(|e| AppError::storage(e.to_string()))?,
        longitude: row
            .try_get("longitude")
            .map_err(|e| AppError::storage(e.to_string()))?,
        source_url: row
            .try_get("source_url")
            .map_err(|e| AppError::storage(e.to_string()))?,
        collected_at,
        inn: row.try_get("inn").unwrap_or(None),
        ogrn: row.try_get("ogrn").unwrap_or(None),
        sources: serde_json::from_str(&sources_json).unwrap_or_default(),
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        branches: serde_json::from_str(&branches_json).unwrap_or_default(),
        dedupe: serde_json::from_str(&dedupe_json).unwrap_or_else(|_| DedupeInfo::default()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use twogis_domain::{SourceAttribution, SourceKind};

    #[tokio::test]
    async fn migrates_v01_database_and_keeps_source_records()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::temp_dir().join(format!(
            "twogis-migration-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true),
            )
            .await?;
        sqlx::query(
            "CREATE TABLE organizations (id TEXT PRIMARY KEY, name TEXT NOT NULL, category TEXT, address TEXT, rating REAL, review_count INTEGER, phones_json TEXT NOT NULL, email TEXT, website TEXT, socials_json TEXT NOT NULL, opening_status TEXT, latitude REAL, longitude REAL, source_url TEXT NOT NULL, collected_at TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        ?;
        sqlx::query("INSERT INTO organizations (id, name, phones_json, socials_json, source_url, collected_at) VALUES ('old', 'Old lead', '[]', '[]', 'https://2gis.ru/moscow/firm/old', '2026-10-01T00:00:00Z')")
            .execute(&pool)
            .await
            ?;
        pool.close().await;

        let store = SqliteStore::connect(&path).await?;
        let old = store.recent(10).await?;
        assert_eq!(old.len(), 1);
        assert_eq!(old[0].name, "Old lead");
        assert_eq!(old[0].sources[0].source, SourceKind::TwoGis);
        assert_eq!(old[0].inn, None);
        let legacy_source_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM source_records")
            .fetch_one(&store.pool)
            .await?;
        assert_eq!(legacy_source_count, 1);

        let mut source = Organization {
            id: "new".into(),
            name: "New lead".into(),
            source_url: "https://example.test/new".into(),
            collected_at: "2026-10-01T00:00:00Z".into(),
            ..Organization::default()
        };
        source.sources.push(SourceAttribution {
            source: SourceKind::Yell,
            source_id: "new".into(),
            source_url: source.source_url.clone(),
            collected_at: source.collected_at.clone(),
        });
        store.record_source_many(&[source]).await?;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM source_records")
            .fetch_one(&store.pool)
            .await?;
        assert_eq!(count, 2);
        drop(store);

        let migrated_again = SqliteStore::connect(&path).await?;
        assert_eq!(migrated_again.recent(10).await?.len(), 1);
        let source_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM source_records")
            .fetch_one(&migrated_again.pool)
            .await?;
        assert_eq!(source_count, 2);
        drop(migrated_again);
        let _ = std::fs::remove_file(path);
        Ok(())
    }
}
