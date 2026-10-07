#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::{
    collections::HashSet,
    error::Error,
    path::{Path, PathBuf},
    process,
    sync::Arc,
};

use clap::{Parser, Subcommand};
use tokio_util::sync::CancellationToken;
use twogis_domain::{
    CollectionPreset, ProviderSearchConfig, SearchRequest, SourceKind, catalog_cities,
    city_rubric_suggestions, city_rubrics, supported_cities,
};
use twogis_export::export;
use twogis_provider::parselab::ParselabProvider;
use twogis_provider_core::{DirectoryProvider, ProgressSink};
use twogis_storage_sqlite::SqliteStore;

#[derive(Parser)]
#[command(name = "lead-aggregator", about = "Сбор лидов из выгрузок каталогов")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List cities available in the bundled catalog
    Cities {
        /// Filter by name substring
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List rubrics available for a city
    Rubrics {
        /// ParseLab city id (see `cities`)
        #[arg(long)]
        city: String,
        #[arg(long)]
        json: bool,
    },
    /// Verify a license key against the key service
    CheckKey {
        #[arg(long)]
        key: String,
    },
    /// Collect rubrics for cities and store results in SQLite
    Collect {
        /// License key (defaults to PARSELAB_KEY env)
        #[arg(long)]
        key: Option<String>,
        /// City ids; repeat or comma-separate (defaults to all supported cities)
        #[arg(long, value_delimiter = ',')]
        city: Vec<String>,
        /// Rubric ids (defaults to every rubric of each city)
        #[arg(long, value_delimiter = ',')]
        rubric: Vec<String>,
        /// Export collected phone list to this file (.csv/.xlsx)
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = "lead-aggregator.db")]
        db: PathBuf,
    },
    /// Export the phone column of an existing run
    Export {
        #[arg(long)]
        run_id: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "lead-aggregator.db")]
        db: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Cities { filter, json } => cities(filter.as_deref(), json),
        Command::Rubrics { city, json } => rubrics(&city, json),
        Command::CheckKey { key } => check_key(&key).await,
        Command::Collect {
            key,
            city,
            rubric,
            out,
            db,
        } => collect(key, city, rubric, out, db).await,
        Command::Export { run_id, out, db } => export_run(&run_id, &out, &db).await,
    }
}

fn cities(filter: Option<&str>, json: bool) -> Result<(), Box<dyn Error>> {
    let supported: HashSet<String> = supported_cities().into_iter().map(|city| city.id).collect();
    let rows: Vec<_> = catalog_cities()
        .iter()
        .filter(|city| supported.contains(&city.id))
        .filter(|city| {
            filter.is_none_or(|needle| city.name.to_lowercase().contains(&needle.to_lowercase()))
        })
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        for city in rows {
            println!("{}\t{}\t{}", city.id, city.code, city.name);
        }
    }
    Ok(())
}

fn rubrics(city: &str, json: bool) -> Result<(), Box<dyn Error>> {
    let suggestions = city_rubric_suggestions(city);
    if json {
        println!("{}", serde_json::to_string_pretty(&suggestions)?);
    } else {
        for rubric in suggestions {
            println!("{}\t{}", rubric.id, rubric.name);
        }
    }
    Ok(())
}

async fn check_key(key: &str) -> Result<(), Box<dyn Error>> {
    let provider = ParselabProvider::new()?;
    match provider.check_key(key).await? {
        Some(user) => {
            println!("ключ действителен");
            if let Ok(pretty) = serde_json::to_string_pretty(&user) {
                println!("{pretty}");
            }
        }
        None => {
            eprintln!("ключ недействителен");
            process::exit(1);
        }
    }
    Ok(())
}

fn parse_key(explicit: Option<&str>) -> Result<String, Box<dyn Error>> {
    let key = explicit
        .map(str::to_owned)
        .or_else(|| std::env::var("PARSELAB_KEY").ok())
        .ok_or("лицензионный ключ не задан: передайте --key или PARSELAB_KEY")?;
    Ok(key)
}

async fn collect(
    key: Option<String>,
    city_ids: Vec<String>,
    rubric_ids: Vec<String>,
    out: Option<PathBuf>,
    db: PathBuf,
) -> Result<(), Box<dyn Error>> {
    let key = parse_key(key.as_deref())?;
    let provider = Arc::new(ParselabProvider::new()?);
    if provider.check_key(&key).await?.is_none() {
        return Err("ключ недействителен".into());
    }

    let supported: HashSet<String> = supported_cities().into_iter().map(|city| city.id).collect();
    let mut cities: Vec<String> = city_ids
        .iter()
        .flat_map(|value| value.split(',').map(str::trim))
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    if cities.is_empty() {
        cities = supported_cities().iter().map(|c| c.id.clone()).collect();
    } else {
        for city in &cities {
            if !supported.contains(city) {
                return Err(format!("город {city} не имеет выгрузок в каталоге").into());
            }
        }
    }

    let rubric_filter: HashSet<String> = rubric_ids
        .iter()
        .flat_map(|value| value.split(',').map(str::trim))
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();

    let store = SqliteStore::connect(&db).await?;
    let keyed_provider: Arc<dyn DirectoryProvider> = provider;
    let providers: Vec<Arc<dyn DirectoryProvider>> = vec![keyed_provider];
    let service = twogis_application::ApplicationService::new(providers, store);

    let mut collected_any = false;
    for city in &cities {
        let rubrics: Vec<String> = if rubric_filter.is_empty() {
            city_rubrics(city)
        } else {
            city_rubrics(city)
                .into_iter()
                .filter(|id| rubric_filter.contains(id))
                .collect()
        };
        if rubrics.is_empty() {
            eprintln!("город {city}: подходящих рубрик нет, пропускаю");
            continue;
        }
        let request = SearchRequest {
            region: city.clone(),
            query: rubrics.join(","),
            max_results: 50_000,
            max_pages: 1_000,
            concurrency: 1,
            request_delay_ms: 0,
            sources: vec![SourceKind::TwoGis],
            regions: Vec::new(),
            provider_configs: vec![ProviderSearchConfig::recommended(
                SourceKind::TwoGis,
                CollectionPreset::Normal,
            )],
            ..SearchRequest::default()
        };
        let progress: ProgressSink = Arc::new(|_| {});
        let summary = service
            .run_search(request, progress, CancellationToken::new())
            .await?;
        collected_any = true;
        println!(
            "город {city}: организаций {}, исходных записей {}, объединено {}",
            summary.organization_count, summary.raw_records, summary.duplicates_merged
        );
        if let Some(path) = &out {
            let run_store = SqliteStore::connect(&db).await?;
            let rows = run_store.all_results_for_run(&summary.run_id).await?;
            let format = if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("xlsx"))
            {
                twogis_domain::ExportFormat::Xlsx
            } else {
                twogis_domain::ExportFormat::Csv
            };
            let count = export(path, format, &rows)?;
            println!("выгрузка: {} телефонов -> {}", count, path.display());
        }
    }
    if !collected_any {
        return Err("не собрано ни одного города".into());
    }
    Ok(())
}

async fn export_run(run_id: &str, out: &Path, db: &Path) -> Result<(), Box<dyn Error>> {
    let store = SqliteStore::connect(db).await?;
    let rows = store.all_results_for_run(run_id).await?;
    let format = if out
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("xlsx"))
    {
        twogis_domain::ExportFormat::Xlsx
    } else {
        twogis_domain::ExportFormat::Csv
    };
    let count = export(out, format, &rows)?;
    println!("выгрузка: {} телефонов -> {}", count, out.display());
    Ok(())
}
