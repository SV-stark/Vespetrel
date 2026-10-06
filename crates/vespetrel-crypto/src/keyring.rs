use thiserror::Error;

#[derive(Debug, Error)]
pub enum KeyringError {
    #[error("keyring error: {0}")]
    Backend(String),
    /// No credential exists for this key.
    ///
    /// Distinct from `Backend` so callers can tell "not signed in" from "the OS keychain is
    /// locked or unavailable". The previous code mapped every error — including
    /// `keyring::Error::NoEntry` — to `Backend(String)`, so those were indistinguishable.
    #[error("not found: {0}")]
    NotFound(String),
}

/// OS keyring wrapper via `keyring-rs` v3 - §4.5
pub struct Keyring {
    service: String,
}

impl Keyring {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry, KeyringError> {
        keyring::Entry::new(&self.service, key).map_err(|e| KeyringError::Backend(e.to_string()))
    }

    /// Classify a keyring error, preserving the absent-credential case.
    fn classify(e: keyring::Error, key: &str) -> KeyringError {
        match e {
            keyring::Error::NoEntry => KeyringError::NotFound(key.to_string()),
            other => KeyringError::Backend(other.to_string()),
        }
    }

    pub fn set(&self, key: &str, secret: &str) -> Result<(), KeyringError> {
        let e = self.entry(key)?;
        e.set_password(secret).map_err(|e| Self::classify(e, key))
    }

    /// Read a secret.
    ///
    /// The value is wrapped in `Zeroizing` so OAuth access/refresh tokens are scrubbed when
    /// dropped rather than left on the heap.
    pub fn get(&self, key: &str) -> Result<zeroize::Zeroizing<String>, KeyringError> {
        let e = self.entry(key)?;
        e.get_password()
            .map(zeroize::Zeroizing::new)
            .map_err(|e| Self::classify(e, key))
    }

    pub fn delete(&self, key: &str) -> Result<(), KeyringError> {
        let e = self.entry(key)?;
        e.delete_credential().map_err(|e| Self::classify(e, key))
    }
}
