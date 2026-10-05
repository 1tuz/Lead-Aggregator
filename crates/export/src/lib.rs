use std::{collections::HashSet, path::Path};

use rust_xlsxwriter::Workbook;
use twogis_domain::{AppError, ExportFormat, Organization};

pub fn export(path: &Path, format: ExportFormat, rows: &[Organization]) -> Result<u32, AppError> {
    match format {
        ExportFormat::Csv => export_phone_csv(path, rows)?,
        ExportFormat::Xlsx => export_phone_xlsx(path, rows)?,
    }
    Ok(clean_phone_list(rows).len() as u32)
}

fn clean_phone_list(rows: &[Organization]) -> Vec<String> {
    let mut seen = HashSet::new();
    rows.iter()
        .flat_map(|row| &row.phones)
        .filter_map(|phone| normalize_phone(phone))
        .filter(|phone| !is_excluded_phone(phone))
        .filter(|phone| seen.insert(phone.clone()))
        .collect()
}

fn normalize_phone(value: &str) -> Option<String> {
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

fn is_excluded_phone(phone: &str) -> bool {
    let digits = phone.strip_prefix('+').unwrap_or(phone);
    let national = if digits.len() == 11 && digits.starts_with('7') {
        &digits[1..]
    } else {
        digits
    };
    if national.len() < 3 {
        return false;
    }
    let code = &national[..3];
    if code == "800" || code == "900" {
        return true;
    }
    let code = code.parse::<u16>().ok();
    code.is_some_and(|code| (910..=919).contains(&code) || (980..=989).contains(&code))
}

fn export_phone_csv(path: &Path, rows: &[Organization]) -> Result<(), AppError> {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_path(path)
        .map_err(|e| AppError::export(format!("failed to create CSV: {e}")))?;
    for phone in clean_phone_list(rows) {
        writer
            .write_record([phone])
            .map_err(|e| AppError::export(e.to_string()))?;
    }
    writer
        .flush()
        .map_err(|e| AppError::export(format!("failed to flush CSV: {e}")))
}

fn export_phone_xlsx(path: &Path, rows: &[Organization]) -> Result<(), AppError> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    for (index, phone) in clean_phone_list(rows).iter().enumerate() {
        sheet
            .write_string(index as u32, 0, phone)
            .map_err(|e| AppError::export(e.to_string()))?;
    }
    sheet.autofit();
    workbook
        .save(path)
        .map_err(|e| AppError::export(format!("failed to save XLSX: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(phones: &[&str]) -> Organization {
        Organization {
            phones: phones.iter().map(|phone| (*phone).to_owned()).collect(),
            ..Organization::default()
        }
    }

    #[test]
    fn phone_list_normalizes_deduplicates_and_filters_requested_codes() {
        let rows = vec![
            row(&["8 (999) 123-45-67", "+7 999 123 45 67", "8-800-555-35-35"]),
            row(&["8 900 123-45-67", "+7 916 123-45-67", "+7 921 123-45-67"]),
        ];
        assert_eq!(
            clean_phone_list(&rows),
            vec!["+79991234567", "+79211234567"]
        );
    }
}
