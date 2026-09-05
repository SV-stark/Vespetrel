use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Provider type enum - mirrors SQL `provider_type` column
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    Imap,
    Jmap,
    Graph,
    Gmail, // Gmail via IMAP+OAuth2 or Graph-like
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Imap => write!(f, "imap"),
            Self::Jmap => write!(f, "jmap"),
            Self::Graph => write!(f, "graph"),
            Self::Gmail => write!(f, "gmail"),
        }
    }
}

impl std::str::FromStr for ProviderType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "imap" => Ok(Self::Imap),
            "jmap" => Ok(Self::Jmap),
            "graph" => Ok(Self::Graph),
            "gmail" => Ok(Self::Gmail),
            _ => Err(format!("unknown provider type: {s}")),
        }
    }
}

/// OAuth2 / password auth configuration stored as JSON in `auth_config` column
/// Tokens themselves are stored in OS keyring; this holds references / metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub auth_method: AuthMethod,
    pub username: Option<String>,
    /// Keyring service key for password / access token lookup
    pub keyring_key: Option<String>,
    /// Keyring service key for refresh token lookup
    #[serde(default)]
    pub refresh_token_keyring_key: Option<String>,
    /// Token expiration timestamp in seconds since UNIX epoch
    #[serde(default)]
    pub expires_at: Option<i64>,
    /// OAuth2 client info (non-secret)
    pub oauth: Option<OAuthConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    #[default]
    Password,
    OAuth2,
    XoAuth2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            auth_method: AuthMethod::Password,
            username: None,
            keyring_key: None,
            refresh_token_keyring_key: None,
            expires_at: None,
            oauth: None,
        }
    }
}

/// Per-account sync cursor state stored as JSON in `sync_state`
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
    #[serde(default)]
    pub last_sync_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub jmap_state: Option<String>,
    #[serde(default)]
    pub graph_delta_token: Option<String>,
    /// Map folder_remote_id -> highestmodseq / uidvalidity
    #[serde(default)]
    pub folder_states: std::collections::HashMap<String, FolderSyncState>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderSyncState {
    pub uid_validity: Option<u32>,
    pub highest_mod_seq: Option<u64>,
    pub uid_next: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub email: String,
    pub provider_type: ProviderType,
    pub auth_config: AuthConfig,
    pub sync_state: SyncState,
    pub is_active: bool,
    pub color: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Account {
    pub fn new(
        name: impl Into<String>,
        email: impl Into<String>,
        provider_type: ProviderType,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            email: email.into(),
            provider_type,
            auth_config: AuthConfig::default(),
            sync_state: SyncState::default(),
            is_active: true,
            color: None,
            created_at: Utc::now(),
        }
    }

    pub fn with_color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("account name cannot be empty".into());
        }
        if !self.email.contains('@') {
            return Err("invalid email address".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_type_formatting_and_parsing() {
        assert_eq!(ProviderType::Imap.to_string(), "imap");
        assert_eq!(ProviderType::Jmap.to_string(), "jmap");
        assert_eq!(ProviderType::Graph.to_string(), "graph");
        assert_eq!(ProviderType::Gmail.to_string(), "gmail");

        assert_eq!("IMAP".parse::<ProviderType>().unwrap(), ProviderType::Imap);
        assert_eq!("jmap".parse::<ProviderType>().unwrap(), ProviderType::Jmap);
        assert_eq!(
            "Graph".parse::<ProviderType>().unwrap(),
            ProviderType::Graph
        );
        assert_eq!(
            "gmail".parse::<ProviderType>().unwrap(),
            ProviderType::Gmail
        );
        assert!("unknown".parse::<ProviderType>().is_err());
    }

    #[test]
    fn test_account_creation_and_validation() {
        let valid = Account::new("Work", "user@company.com", ProviderType::Imap);
        assert!(valid.validate().is_ok());
        assert!(valid.is_active);

        let empty_name = Account::new("   ", "user@company.com", ProviderType::Jmap);
        assert!(empty_name.validate().is_err());

        let invalid_email = Account::new("Personal", "notanemail", ProviderType::Graph);
        assert!(invalid_email.validate().is_err());
    }
}
