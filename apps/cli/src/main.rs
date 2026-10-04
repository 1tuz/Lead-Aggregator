#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::{
    collections::HashSet, env, error::Error, fs, io, path::PathBuf, sync::Arc, time::Duration,
};

use serde::Serialize;
use tokio_util::sync::CancellationToken;
use twogis_domain::{CollectionPreset, ErrorKind, ProviderSearchConfig, SearchRequest, SourceKind};
use twogis_provider::TwoGisHtmlProvider;
use twogis_provider_core::{
    CatalogHttpClient, DESKTOP_USER_AGENT, DirectoryProvider, ProgressSink, ProviderControl,
    StopReason,
};
use twogis_provider_public_catalogs::{
    RusprofileHtmlProvider, YellHtmlProvider, ZoonHtmlProvider, ZoonListing,
};
use url::Url;

#[derive(Debug, Clone)]
struct CliArgs {
    sources: Vec<SourceKind>,
    region: String,
    query: String,
    pages: u16,
    max_results: u32,
    json: bool,
    enrich: bool,
    save_html_dir: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditPage {
    page: u16,
    url: String,
    http_status: u16,
    final_url: String,
    challenge: Option<String>,
    links_found: u32,
    new_candidates: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceAudit {
    source: String,
    region: String,
    query: String,
    status: String,
    pages: Vec<AuditPage>,
    candidate_count: u32,
    profiles_loaded: u32,
    parse_errors: u32,
    raw_records: u32,
    unique_records: u32,
    duplicates_merged: u32,
    stop_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    let mut reports = Vec::new();
    for source in &args.sources {
        reports.push(audit_source(*source, &args).await);
    }

    if args.json {
        if reports.len() == 1 {
            println!("{}", serde_json::to_string_pretty(&reports[0])?);
        } else {
            println!("{}", serde_json::to_string_pretty(&reports)?);
        }
    } else {
        for report in &reports {
            println!(
                "{} status={} candidates={} unique={} stop={}",
                report.source,
                report.status,
                report.candidate_count,
                report.unique_records,
                report.stop_reason
            );
            for page in &report.pages {
                println!(
                    "  p{} http={} links={} new={} {}",
                    page.page, page.http_status, page.links_found, page.new_candidates, page.url
                );
            }
            if let Some(error) = &report.error {
                println!("  error: {error}");
            }
        }
    }
    Ok(())
}

async fn audit_source(source: SourceKind, args: &CliArgs) -> SourceAudit {
    let mut config = ProviderSearchConfig::recommended(source, CollectionPreset::Gentle);
    config.enabled = true;
    config.max_pages = args.pages;
    config.max_results = args.max_results;
    config.concurrency = 1;

    let request = SearchRequest {
        region: args.region.clone(),
        query: args.query.clone(),
        max_results: args.max_results,
        max_pages: args.pages,
        concurrency: 1,
        request_delay_ms: config.request_delay_ms,
        sources: vec![source],
        regions: Vec::new(),
        provider_configs: vec![config.clone()],
    };

    let discovery = match source {
        SourceKind::TwoGis => discover_twogis(&request, args).await,
        SourceKind::Yell => discover_yell(&request, args).await,
        SourceKind::Zoon => discover_zoon(&request, args).await,
        SourceKind::Rusprofile => discover_rusprofile(&request, args).await,
    };

    match discovery {
        Ok((pages, candidate_count, stop_reason, early_status, early_error)) => {
            if let Some(status) = early_status {
                return SourceAudit {
                    source: source.id().into(),
                    region: request.region.clone(),
                    query: request.query.clone(),
                    status,
                    pages,
                    candidate_count,
                    profiles_loaded: 0,
                    parse_errors: 0,
                    raw_records: 0,
                    unique_records: 0,
                    duplicates_merged: 0,
                    stop_reason: stop_reason.as_str().into(),
                    error: early_error,
                };
            }

            if !args.enrich {
                return SourceAudit {
                    source: source.id().into(),
                    region: request.region.clone(),
                    query: request.query.clone(),
                    status: status_from_pages(&pages, stop_reason),
                    pages,
                    candidate_count,
                    profiles_loaded: 0,
                    parse_errors: 0,
                    raw_records: 0,
                    unique_records: 0,
                    duplicates_merged: 0,
                    stop_reason: stop_reason.as_str().into(),
                    error: None,
                };
            }

            enrich_via_provider(source, request, pages, candidate_count, stop_reason).await
        }
        Err(message) => SourceAudit {
            source: source.id().into(),
            region: args.region.clone(),
            query: args.query.clone(),
            status: "other".into(),
            pages: Vec::new(),
            candidate_count: 0,
            profiles_loaded: 0,
            parse_errors: 0,
            raw_records: 0,
            unique_records: 0,
            duplicates_merged: 0,
            stop_reason: StopReason::EmptyPage.as_str().into(),
            error: Some(message),
        },
    }
}

async fn enrich_via_provider(
    source: SourceKind,
    request: SearchRequest,
    pages: Vec<AuditPage>,
    candidate_count: u32,
    stop_reason: StopReason,
) -> SourceAudit {
    let progress: ProgressSink = Arc::new(|_| {});
    let control = ProviderControl::new(CancellationToken::new());
    let result = match source {
        SourceKind::TwoGis => {
            let Ok(provider) = TwoGisHtmlProvider::new() else {
                return err_report(source, &request, pages, "provider init failed");
            };
            provider.search(&request, progress, control).await
        }
        SourceKind::Yell => {
            let Ok(provider) = YellHtmlProvider::new() else {
                return err_report(source, &request, pages, "provider init failed");
            };
            provider.search(&request, progress, control).await
        }
        SourceKind::Zoon => {
            let Ok(provider) = ZoonHtmlProvider::new() else {
                return err_report(source, &request, pages, "provider init failed");
            };
            provider.search(&request, progress, control).await
        }
        SourceKind::Rusprofile => {
            let Ok(provider) = RusprofileHtmlProvider::new() else {
                return err_report(source, &request, pages, "provider init failed");
            };
            provider.search(&request, progress, control).await
        }
    };

    match result {
        Ok(output) => {
            let parse_errors = output.warnings.len() as u32;
            let raw_records = output.organizations.len() as u32;
            let dedupe = twogis_dedupe::deduplicate(output.organizations);
            SourceAudit {
                source: source.id().into(),
                region: request.region,
                query: request.query,
                status: status_from_pages(&pages, stop_reason),
                pages,
                candidate_count: candidate_count.max(output.candidate_count),
                profiles_loaded: raw_records,
                parse_errors,
                raw_records,
                unique_records: dedupe.organizations.len() as u32,
                duplicates_merged: dedupe.merged_count,
                stop_reason: output.stop_reason.unwrap_or(stop_reason).as_str().into(),
                error: None,
            }
        }
        Err(err) => SourceAudit {
            source: source.id().into(),
            region: request.region,
            query: request.query,
            status: status_for_error(err.kind),
            pages,
            candidate_count,
            profiles_loaded: 0,
            parse_errors: 0,
            raw_records: 0,
            unique_records: 0,
            duplicates_merged: 0,
            stop_reason: stop_for_error(err.kind).as_str().into(),
            error: Some(err.message),
        },
    }
}

fn err_report(
    source: SourceKind,
    request: &SearchRequest,
    pages: Vec<AuditPage>,
    message: &str,
) -> SourceAudit {
    SourceAudit {
        source: source.id().into(),
        region: request.region.clone(),
        query: request.query.clone(),
        status: "other".into(),
        pages,
        candidate_count: 0,
        profiles_loaded: 0,
        parse_errors: 0,
        raw_records: 0,
        unique_records: 0,
        duplicates_merged: 0,
        stop_reason: StopReason::EmptyPage.as_str().into(),
        error: Some(message.into()),
    }
}

type Discovery = Result<
    (
        Vec<AuditPage>,
        u32,
        StopReason,
        Option<String>,
        Option<String>,
    ),
    String,
>;

async fn discover_twogis(request: &SearchRequest, args: &CliArgs) -> Discovery {
    let provider = TwoGisHtmlProvider::new().map_err(|e| e.message)?;
    let http = CatalogHttpClient::new().map_err(|e| e.message)?;
    let mut pages = Vec::new();
    let mut seen = HashSet::new();
    let mut candidates = 0u32;
    let mut stop_reason = StopReason::MaxPages;
    let delay = Duration::from_millis(u64::from(
        request.config_for(SourceKind::TwoGis).request_delay_ms,
    ));

    for page in 1..=args.pages {
        let url = provider.search_url(request, page).map_err(|e| e.message)?;
        tokio::time::sleep(delay).await;
        let report = http
            .fetch_html_report(SourceKind::TwoGis, &url)
            .await
            .map_err(|e| e.message)?;
        maybe_save_html(args, SourceKind::TwoGis, page, &report.body);
        let found = provider.discover_firms(&report.body);
        let mut new = 0u32;
        for (link, _) in &found {
            if seen.insert(link.clone()) {
                candidates += 1;
                new += 1;
            }
        }
        pages.push(page_row(page, &report, found.len() as u32, new));
        if let Some(early) = early_stop(&report) {
            return Ok((pages, candidates, early.0, Some(early.1), early.2));
        }
        if found.is_empty() {
            stop_reason = StopReason::EmptyPage;
            break;
        }
        let has_next = TwoGisHtmlProvider::has_next_page(&report.body, page);
        if new == 0 && !has_next {
            stop_reason = StopReason::NoNewCandidates;
            break;
        }
        if !has_next {
            stop_reason = StopReason::LastPage;
            break;
        }
    }
    Ok((pages, candidates, stop_reason, None, None))
}

async fn discover_yell(request: &SearchRequest, args: &CliArgs) -> Discovery {
    let provider = YellHtmlProvider::new().map_err(|e| e.message)?;
    let http = CatalogHttpClient::new().map_err(|e| e.message)?;
    let mut pages = Vec::new();
    let mut seen = HashSet::new();
    let mut candidates = 0u32;
    let mut stop_reason = StopReason::MaxPages;
    let delay = Duration::from_millis(u64::from(
        request.config_for(SourceKind::Yell).request_delay_ms,
    ));

    for page in 1..=args.pages {
        let url = provider.search_url(request, page).map_err(|e| e.message)?;
        tokio::time::sleep(delay).await;
        let report = http
            .fetch_html_report(SourceKind::Yell, &url)
            .await
            .map_err(|e| e.message)?;
        maybe_save_html(args, SourceKind::Yell, page, &report.body);
        let found = provider.discover(request, &report.body);
        let mut new = 0u32;
        for link in &found {
            if seen.insert(link.to_string()) {
                candidates += 1;
                new += 1;
            }
        }
        pages.push(page_row(page, &report, found.len() as u32, new));
        if let Some(early) = early_stop(&report) {
            return Ok((pages, candidates, early.0, Some(early.1), early.2));
        }
        if found.is_empty() {
            stop_reason = StopReason::EmptyPage;
            break;
        }
        let has_next = YellHtmlProvider::has_next_page(&report.body, page);
        if new == 0 && !has_next {
            stop_reason = StopReason::NoNewCandidates;
            break;
        }
        if !has_next {
            stop_reason = StopReason::LastPage;
            break;
        }
    }
    Ok((pages, candidates, stop_reason, None, None))
}

async fn discover_zoon(request: &SearchRequest, args: &CliArgs) -> Discovery {
    let provider = ZoonHtmlProvider::new().map_err(|e| e.message)?;
    let http = CatalogHttpClient::new().map_err(|e| e.message)?;
    let delay = Duration::from_millis(u64::from(
        request.config_for(SourceKind::Zoon).request_delay_ms,
    ));

    tokio::time::sleep(delay).await;
    let probe_url = provider.search_probe_url(request).map_err(|e| e.message)?;
    let probe = http
        .fetch_html_report(SourceKind::Zoon, &probe_url)
        .await
        .map_err(|e| e.message)?;
    maybe_save_html(args, SourceKind::Zoon, 0, &probe.body);
    let listing = provider.detect_listing(request, &probe.body);

    let mut pages = Vec::new();
    let mut seen = HashSet::new();
    let mut candidates = 0u32;
    let mut stop_reason = StopReason::MaxPages;

    for page in 1..=args.pages {
        let url = provider
            .listing_url(&listing, request, page)
            .map_err(|e| e.message)?;
        tokio::time::sleep(delay).await;
        let report = http
            .fetch_html_report(SourceKind::Zoon, &url)
            .await
            .map_err(|e| e.message)?;
        maybe_save_html(args, SourceKind::Zoon, page, &report.body);
        let found = provider.discover(request, &report.body);
        let mut new = 0u32;
        for link in &found {
            if seen.insert(link.to_string()) {
                candidates += 1;
                new += 1;
            }
        }
        pages.push(page_row(page, &report, found.len() as u32, new));
        if let Some(early) = early_stop(&report) {
            return Ok((pages, candidates, early.0, Some(early.1), early.2));
        }
        if found.is_empty() {
            stop_reason = StopReason::EmptyPage;
            break;
        }
        let has_next = ZoonHtmlProvider::has_next_page(&listing, &report.body, page);
        if matches!(listing, ZoonListing::Search) {
            stop_reason = StopReason::LastPage;
            break;
        }
        if new == 0 && !has_next {
            stop_reason = StopReason::NoNewCandidates;
            break;
        }
        if !has_next {
            stop_reason = StopReason::LastPage;
            break;
        }
    }
    Ok((pages, candidates, stop_reason, None, None))
}

async fn discover_rusprofile(request: &SearchRequest, args: &CliArgs) -> Discovery {
    let provider = RusprofileHtmlProvider::new().map_err(|e| e.message)?;
    let http = CatalogHttpClient::new().map_err(|e| e.message)?;
    let mut pages = Vec::new();
    let mut seen = HashSet::new();
    let mut candidates = 0u32;
    let mut stop_reason = StopReason::MaxPages;
    let delay = Duration::from_millis(u64::from(
        request.config_for(SourceKind::Rusprofile).request_delay_ms,
    ));

    for page in 1..=args.pages {
        let url = provider.search_url(request, page).map_err(|e| e.message)?;
        tokio::time::sleep(delay).await;
        let report = http
            .fetch_html_report(SourceKind::Rusprofile, &url)
            .await
            .map_err(|e| e.message)?;
        maybe_save_html(args, SourceKind::Rusprofile, page, &report.body);
        let found = provider.discover(&report.body);
        let mut new = 0u32;
        for link in &found {
            if seen.insert(link.to_string()) {
                candidates += 1;
                new += 1;
            }
        }
        pages.push(page_row(page, &report, found.len() as u32, new));
        if report.http_status == 404 {
            return Ok((
                pages,
                0,
                StopReason::EmptyPage,
                Some("wrong_url".into()),
                Some(
                    "Rusprofile public /search returns HTTP 404; keyword mapped listings use /codes/{code}"
                        .into(),
                ),
            ));
        }
        if let Some(early) = early_stop(&report) {
            return Ok((pages, candidates, early.0, Some(early.1), early.2));
        }
        if found.is_empty() {
            stop_reason = StopReason::EmptyPage;
            break;
        }
        let has_next = RusprofileHtmlProvider::has_next_page(&report.body, page);
        if new == 0 && !has_next {
            stop_reason = StopReason::NoNewCandidates;
            break;
        }
        if !has_next {
            stop_reason = StopReason::LastPage;
            break;
        }
    }
    Ok((pages, candidates, stop_reason, None, None))
}

fn page_row(
    page: u16,
    report: &twogis_provider_core::HtmlFetchReport,
    links_found: u32,
    new_candidates: u32,
) -> AuditPage {
    AuditPage {
        page,
        url: report.request_url.clone(),
        http_status: report.http_status,
        final_url: report.final_url.clone(),
        challenge: challenge_label(&report.challenge),
        links_found,
        new_candidates,
    }
}

fn early_stop(
    report: &twogis_provider_core::HtmlFetchReport,
) -> Option<(StopReason, String, Option<String>)> {
    if report.http_status == 403 {
        return Some((StopReason::Blocked, "403".into(), None));
    }
    if report.http_status == 429 {
        return Some((StopReason::RateLimited, "429".into(), None));
    }
    if report.challenge.error_kind() == Some(ErrorKind::CaptchaRequired) {
        return Some((
            StopReason::Captcha,
            "captcha".into(),
            Some(report.challenge.reason.clone()),
        ));
    }
    if report.challenge.error_kind() == Some(ErrorKind::ChallengeRequired) {
        return Some((
            StopReason::Blocked,
            "anti_bot".into(),
            Some(report.challenge.reason.clone()),
        ));
    }
    None
}

fn status_from_pages(pages: &[AuditPage], stop_reason: StopReason) -> String {
    if pages
        .iter()
        .any(|page| page.challenge.as_deref() == Some("captcha"))
    {
        return "captcha".into();
    }
    if pages.iter().any(|page| page.http_status == 403) {
        return "403".into();
    }
    if pages.iter().any(|page| page.http_status == 429) {
        return "429".into();
    }
    if pages.iter().any(|page| page.http_status == 404) {
        return "wrong_url".into();
    }
    if pages.iter().map(|page| page.new_candidates).sum::<u32>() == 0 {
        return "html_missing_results".into();
    }
    match stop_reason {
        StopReason::NoNewCandidates if pages.len() > 1 => "duplicate_pages".into(),
        _ => "working".into(),
    }
}

fn status_for_error(kind: ErrorKind) -> String {
    match kind {
        ErrorKind::CaptchaRequired => "captcha".into(),
        ErrorKind::Blocked => "403".into(),
        ErrorKind::ChallengeRequired => "anti_bot".into(),
        ErrorKind::RateLimited => "429".into(),
        _ => "other".into(),
    }
}

fn stop_for_error(kind: ErrorKind) -> StopReason {
    match kind {
        ErrorKind::CaptchaRequired => StopReason::Captcha,
        ErrorKind::Blocked | ErrorKind::ChallengeRequired => StopReason::Blocked,
        ErrorKind::RateLimited => StopReason::RateLimited,
        ErrorKind::Cancelled => StopReason::Cancelled,
        _ => StopReason::EmptyPage,
    }
}

fn challenge_label(detection: &twogis_provider_core::ChallengeDetection) -> Option<String> {
    detection.error_kind().map(|kind| match kind {
        ErrorKind::CaptchaRequired => "captcha".into(),
        ErrorKind::ChallengeRequired => "anti_bot".into(),
        _ => detection.reason.clone(),
    })
}

fn maybe_save_html(args: &CliArgs, source: SourceKind, page: u16, body: &str) {
    let Some(dir) = &args.save_html_dir else {
        return;
    };
    let _ = fs::create_dir_all(dir);
    let path = dir.join(format!("{}-p{page}.html", source.id()));
    let _ = fs::write(path, body);
}

fn parse_args() -> Result<CliArgs, Box<dyn Error>> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some("audit") {
        return Err(invalid(
            "usage: lead-aggregator audit --source|--sources ... --region moscow --query ... --pages 3 --json [--save-html DIR] [--enrich]",
        ));
    }
    args.remove(0);

    let mut source = None::<String>;
    let mut sources = None::<String>;
    let mut region = "moscow".to_owned();
    let mut query = "автосервис".to_owned();
    let mut pages = 3u16;
    let mut max_results = 40u32;
    let mut json = false;
    let mut enrich = false;
    let mut save_html_dir = None::<PathBuf>;

    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "--source" => {
                source = Some(required(&args, idx + 1, "--source")?);
                idx += 2;
            }
            "--sources" => {
                sources = Some(required(&args, idx + 1, "--sources")?);
                idx += 2;
            }
            "--region" => {
                region = required(&args, idx + 1, "--region")?;
                idx += 2;
            }
            "--query" => {
                query = required(&args, idx + 1, "--query")?;
                idx += 2;
            }
            "--pages" => {
                pages = required(&args, idx + 1, "--pages")?.parse()?;
                idx += 2;
            }
            "--max-results" => {
                max_results = required(&args, idx + 1, "--max-results")?.parse()?;
                idx += 2;
            }
            "--json" => {
                json = true;
                idx += 1;
            }
            "--enrich" => {
                enrich = true;
                idx += 1;
            }
            "--save-html" => {
                save_html_dir = Some(PathBuf::from(required(&args, idx + 1, "--save-html")?));
                idx += 2;
            }
            other => return Err(invalid(&format!("unknown argument: {other}"))),
        }
    }

    let selected = sources.or(source).unwrap_or_else(|| "all".into());
    eprintln!("user-agent: {DESKTOP_USER_AGENT}");
    let _ = Url::parse("https://example.test/");
    Ok(CliArgs {
        sources: parse_sources(&selected)?,
        region,
        query,
        pages,
        max_results,
        json,
        enrich,
        save_html_dir,
    })
}

fn parse_sources(value: &str) -> Result<Vec<SourceKind>, Box<dyn Error>> {
    if value == "all" {
        return Ok(vec![
            SourceKind::TwoGis,
            SourceKind::Yell,
            SourceKind::Zoon,
            SourceKind::Rusprofile,
        ]);
    }
    value
        .split(',')
        .map(|part| match part.trim().to_ascii_lowercase().as_str() {
            "2gis" | "twogis" => Ok(SourceKind::TwoGis),
            "yell" => Ok(SourceKind::Yell),
            "zoon" => Ok(SourceKind::Zoon),
            "rusprofile" => Ok(SourceKind::Rusprofile),
            other => Err(invalid(&format!("unknown source: {other}"))),
        })
        .collect()
}

fn required(args: &[String], index: usize, flag: &str) -> Result<String, Box<dyn Error>> {
    args.get(index)
        .cloned()
        .ok_or_else(|| invalid(&format!("missing value for {flag}")))
}

fn invalid(message: &str) -> Box<dyn Error> {
    Box::new(io::Error::new(
        io::ErrorKind::InvalidInput,
        message.to_owned(),
    ))
}
