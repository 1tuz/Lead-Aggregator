use keyring::{Entry, Error};
use twogis_domain::AppError;

const SERVICE: &str = "dev.local.lead-aggregator";
const ACCOUNT: &str = "2gis-places-api-key";

fn entry() -> Result<Entry, AppError> {
    Entry::new(SERVICE, ACCOUNT)
        .map_err(|_| AppError::storage("Не удалось подключить системное хранилище секретов"))
}

pub fn load_2gis_api_key() -> Result<Option<String>, AppError> {
    match entry()?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(_) => Err(AppError::storage(
            "Не удалось прочитать ключ 2ГИС из системного хранилища",
        )),
    }
}

pub fn save_2gis_api_key(key: &str) -> Result<(), AppError> {
    entry()?
        .set_password(key)
        .map_err(|_| AppError::storage("Не удалось сохранить ключ 2ГИС в системном хранилище"))
}

pub fn delete_2gis_api_key() -> Result<(), AppError> {
    match entry()?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(_) => Err(AppError::storage(
            "Не удалось удалить ключ 2ГИС из системного хранилища",
        )),
    }
}
