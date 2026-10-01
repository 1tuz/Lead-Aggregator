use std::{fs::File, io::BufWriter, path::Path};

use rust_xlsxwriter::Workbook;
use twogis_domain::{AppError, ExportFormat, Organization};

pub fn export(path: &Path, format: ExportFormat, rows: &[Organization]) -> Result<(), AppError> {
    match format {
        ExportFormat::Csv => export_csv(path, rows),
        ExportFormat::Json => export_json(path, rows),
        ExportFormat::Xlsx => export_xlsx(path, rows),
    }
}

fn source_labels(row: &Organization) -> String {
    row.sources
        .iter()
        .map(|source| source.source.label())
        .collect::<Vec<_>>()
        .join(" | ")
}

fn export_csv(path: &Path, rows: &[Organization]) -> Result<(), AppError> {
    let mut writer = csv::Writer::from_path(path)
        .map_err(|e| AppError::export(format!("failed to create CSV: {e}")))?;
    writer
        .write_record([
            "id",
            "name",
            "category",
            "address",
            "rating",
            "review_count",
            "phones",
            "email",
            "website",
            "socials",
            "inn",
            "ogrn",
            "sources",
            "tags",
            "merged_records",
            "possible_duplicate",
            "opening_status",
            "latitude",
            "longitude",
            "source_url",
            "collected_at",
        ])
        .map_err(|e| AppError::export(e.to_string()))?;

    for row in rows {
        let rating = row.rating.map(|v| v.to_string()).unwrap_or_default();
        let reviews = row.review_count.map(|v| v.to_string()).unwrap_or_default();
        let phones = row.phones.join(" | ");
        let socials = row.socials.join(" | ");
        let sources = source_labels(row);
        let tags = row.tags.join(" | ");
        let merged = row.dedupe.merged_records.to_string();
        let possible = row.dedupe.possible_duplicate.to_string();
        let latitude = row.latitude.map(|v| v.to_string()).unwrap_or_default();
        let longitude = row.longitude.map(|v| v.to_string()).unwrap_or_default();
        writer
            .write_record([
                row.id.as_str(),
                row.name.as_str(),
                row.category.as_deref().unwrap_or_default(),
                row.address.as_deref().unwrap_or_default(),
                rating.as_str(),
                reviews.as_str(),
                phones.as_str(),
                row.email.as_deref().unwrap_or_default(),
                row.website.as_deref().unwrap_or_default(),
                socials.as_str(),
                row.inn.as_deref().unwrap_or_default(),
                row.ogrn.as_deref().unwrap_or_default(),
                sources.as_str(),
                tags.as_str(),
                merged.as_str(),
                possible.as_str(),
                row.opening_status.as_deref().unwrap_or_default(),
                latitude.as_str(),
                longitude.as_str(),
                row.source_url.as_str(),
                row.collected_at.as_str(),
            ])
            .map_err(|e| AppError::export(e.to_string()))?;
    }
    writer
        .flush()
        .map_err(|e| AppError::export(format!("failed to flush CSV: {e}")))
}

fn export_json(path: &Path, rows: &[Organization]) -> Result<(), AppError> {
    let file =
        File::create(path).map_err(|e| AppError::export(format!("failed to create JSON: {e}")))?;
    serde_json::to_writer_pretty(BufWriter::new(file), rows)
        .map_err(|e| AppError::export(format!("failed to write JSON: {e}")))
}

fn export_xlsx(path: &Path, rows: &[Organization]) -> Result<(), AppError> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    let headers = [
        "ID",
        "Name",
        "Category",
        "Address",
        "Rating",
        "Reviews",
        "Phones",
        "Email",
        "Website",
        "Socials",
        "INN",
        "OGRN",
        "Sources",
        "Tags",
        "Merged",
        "Possible duplicate",
        "Status",
        "Latitude",
        "Longitude",
        "2GIS URL",
        "Collected at",
    ];
    for (column, header) in headers.iter().enumerate() {
        sheet
            .write_string(0, column as u16, *header)
            .map_err(|e| AppError::export(e.to_string()))?;
    }
    for (index, row) in rows.iter().enumerate() {
        let r = index as u32 + 1;
        let cells = [
            row.id.clone(),
            row.name.clone(),
            row.category.clone().unwrap_or_default(),
            row.address.clone().unwrap_or_default(),
            row.rating.map(|v| v.to_string()).unwrap_or_default(),
            row.review_count.map(|v| v.to_string()).unwrap_or_default(),
            row.phones.join(" | "),
            row.email.clone().unwrap_or_default(),
            row.website.clone().unwrap_or_default(),
            row.socials.join(" | "),
            row.inn.clone().unwrap_or_default(),
            row.ogrn.clone().unwrap_or_default(),
            source_labels(row),
            row.tags.join(" | "),
            row.dedupe.merged_records.to_string(),
            row.dedupe.possible_duplicate.to_string(),
            row.opening_status.clone().unwrap_or_default(),
            row.latitude.map(|v| v.to_string()).unwrap_or_default(),
            row.longitude.map(|v| v.to_string()).unwrap_or_default(),
            row.source_url.clone(),
            row.collected_at.clone(),
        ];
        for (column, value) in cells.iter().enumerate() {
            sheet
                .write_string(r, column as u16, value)
                .map_err(|e| AppError::export(e.to_string()))?;
        }
    }
    sheet.autofit();
    workbook
        .save(path)
        .map_err(|e| AppError::export(format!("failed to save XLSX: {e}")))
}
