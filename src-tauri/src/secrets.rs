const SERVICE: &str = "com.dimfi.livecaption";
const PRIMARY_API_KEY: &str = "primary-api-key";

pub fn normalize_api_key(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let key = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .unwrap_or(trimmed)
        .trim();

    if key.is_empty() {
        return Err("API Key 不能为空".to_string());
    }

    if key.chars().any(char::is_whitespace) {
        return Err("API Key 不能包含空白字符".to_string());
    }

    Ok(key.to_string())
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, PRIMARY_API_KEY).map_err(|error| error.to_string())
}

pub fn set_api_key(value: &str) -> Result<(), String> {
    let normalized = normalize_api_key(value)?;
    entry()?
        .set_password(&normalized)
        .map_err(|error| error.to_string())
}

pub fn get_api_key() -> Result<Option<String>, String> {
    match entry()?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub fn has_api_key() -> bool {
    matches!(get_api_key(), Ok(Some(_)))
}

#[allow(dead_code)]
pub fn delete_api_key() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
