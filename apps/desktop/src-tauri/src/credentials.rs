use keyring::{Entry, Error};
use twogis_domain::AppError;

const SERVICE: &str = "dev.local.lead-aggregator";
const ACCOUNT_PARSELAB: &str = "parselab-license-key";

fn entry(account: &str) -> Result<Entry, AppError> {
    Entry::new(SERVICE, account)
        .map_err(|_| AppError::storage("Не удалось подключить системное хранилище секретов"))
}

pub fn load_parselab_key() -> Result<Option<String>, AppError> {
    load(ACCOUNT_PARSELAB, "лицензионный ключ")
}

pub fn save_parselab_key(key: &str) -> Result<(), AppError> {
    save(ACCOUNT_PARSELAB, key, "лицензионный ключ")
}

pub fn delete_parselab_key() -> Result<(), AppError> {
    delete(ACCOUNT_PARSELAB, "лицензионный ключ")
}

fn load(account: &str, label: &str) -> Result<Option<String>, AppError> {
    match entry(account)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(_) => Err(AppError::storage(format!(
            "Не удалось прочитать {label} из системного хранилища"
        ))),
    }
}

fn save(account: &str, key: &str, label: &str) -> Result<(), AppError> {
    entry(account)?.set_password(key).map_err(|_| {
        AppError::storage(format!(
            "Не удалось сохранить {label} в системном хранилище"
        ))
    })
}

fn delete(account: &str, label: &str) -> Result<(), AppError> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(_) => Err(AppError::storage(format!(
            "Не удалось удалить {label} из системного хранилища"
        ))),
    }
}
