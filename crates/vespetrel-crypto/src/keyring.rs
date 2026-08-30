use thiserror::Error;

#[derive(Debug, Error)]
pub enum KeyringError {
    #[error("keyring error: {0}")]
    Backend(String),
    #[error("not found: {0}")]
    NotFound(String),
}

/// OS keyring wrapper via `keyring-rs` v3 - §4.5
pub struct Keyring {
    service: String,
}

impl Keyring {
    pub fn new(service: impl Into<String>) -> Self { Self { service: service.into() } }

    fn entry(&self, key: &str) -> Result<keyring::Entry, KeyringError> {
        keyring::Entry::new(&self.service, key).map_err(|e| KeyringError::Backend(e.to_string()))
    }

    pub fn set(&self, key: &str, secret: &str) -> Result<(), KeyringError> {
        let e = self.entry(key)?;
        e.set_password(secret).map_err(|e| KeyringError::Backend(e.to_string()))
    }

    pub fn get(&self, key: &str) -> Result<String, KeyringError> {
        let e = self.entry(key)?;
        e.get_password().map_err(|e| KeyringError::Backend(e.to_string()))
    }

    pub fn delete(&self, key: &str) -> Result<(), KeyringError> {
        let e = self.entry(key)?;
        e.delete_credential().map_err(|e| KeyringError::Backend(e.to_string()))
    }
}
