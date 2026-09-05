//! Interactive GUI Login Wizard Modal §4 & §7 Phase 2
use vespetrel_core::{Account, ProviderType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthModeChoice {
    OAuth2,
    Password,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WizardStep {
    SelectProvider,
    EnterCredentials,
    OAuth2Waiting,
    Validating,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct LoginWizardState {
    pub step: WizardStep,
    pub provider_type: ProviderType,
    pub auth_mode: AuthModeChoice,
    pub name: String,
    pub email: String,
    pub password_or_token: String,
    pub incoming_host: String,
    pub incoming_port: u16,
    pub outgoing_host: String,
    pub outgoing_port: u16,
    pub use_tls: bool,
    pub client_id: Option<String>,
    pub oauth_status: Option<String>,
}

impl LoginWizardState {
    pub fn new() -> Self {
        Self {
            step: WizardStep::SelectProvider,
            provider_type: ProviderType::Imap,
            auth_mode: AuthModeChoice::Password,
            name: String::new(),
            email: String::new(),
            password_or_token: String::new(),
            incoming_host: String::new(),
            incoming_port: 993,
            outgoing_host: String::new(),
            outgoing_port: 587,
            use_tls: true,
            client_id: None,
            oauth_status: None,
        }
    }

    pub fn select_provider(&mut self, provider: ProviderType) {
        self.provider_type = provider;
        match self.provider_type {
            ProviderType::Gmail => {
                self.incoming_host = "imap.gmail.com".into();
                self.incoming_port = 993;
                self.outgoing_host = "smtp.gmail.com".into();
                self.outgoing_port = 587;
                self.auth_mode = AuthModeChoice::OAuth2;
                self.step = WizardStep::EnterCredentials;
            }
            ProviderType::Graph => {
                self.incoming_host = "graph.microsoft.com".into();
                self.incoming_port = 443;
                self.outgoing_host = "graph.microsoft.com".into();
                self.outgoing_port = 443;
                self.auth_mode = AuthModeChoice::OAuth2;
                self.step = WizardStep::EnterCredentials;
            }
            ProviderType::Jmap => {
                self.incoming_host = "api.fastmail.com".into();
                self.incoming_port = 443;
                self.outgoing_host = "api.fastmail.com".into();
                self.outgoing_port = 443;
                self.auth_mode = AuthModeChoice::Password;
                self.step = WizardStep::EnterCredentials;
            }
            ProviderType::Imap => {
                self.incoming_host = String::new();
                self.incoming_port = 993;
                self.outgoing_host = String::new();
                self.outgoing_port = 587;
                self.auth_mode = AuthModeChoice::Password;
                self.step = WizardStep::EnterCredentials;
            }
        }
    }

    pub fn validate_and_build_account(&self) -> Result<Account, String> {
        if self.email.trim().is_empty() || !self.email.contains('@') {
            return Err("Invalid email address".into());
        }

        let name = if self.name.trim().is_empty() {
            self.email.clone()
        } else {
            self.name.clone()
        };

        // Determine effective provider type based on selected auth_mode
        let effective_provider = match (self.provider_type.clone(), self.auth_mode) {
            (ProviderType::Gmail, AuthModeChoice::Password) => ProviderType::Imap,
            (p, _) => p,
        };

        let mut acct = Account::new(name, self.email.clone(), effective_provider);
        match (&acct.provider_type, self.auth_mode) {
            (ProviderType::Imap, _) => {
                acct.auth_config.auth_method = vespetrel_core::account::AuthMethod::Password;
                acct.auth_config.username = Some(self.email.clone());
                acct.auth_config.keyring_key = Some(format!("vespetrel_{}", self.email));
            }
            (ProviderType::Gmail, AuthModeChoice::OAuth2)
            | (ProviderType::Graph, AuthModeChoice::OAuth2) => {
                acct.auth_config.auth_method = vespetrel_core::account::AuthMethod::OAuth2;
                acct.auth_config.username = Some(self.email.clone());
                acct.auth_config.keyring_key = Some(format!("vespetrel_oauth_{}", self.email));
                acct.auth_config.refresh_token_keyring_key =
                    Some(format!("vespetrel_refresh_{}", self.email));
            }
            (ProviderType::Gmail, AuthModeChoice::Password) => {
                acct.auth_config.auth_method = vespetrel_core::account::AuthMethod::Password;
                acct.auth_config.username = Some(self.email.clone());
                acct.auth_config.keyring_key = Some(format!("vespetrel_{}", self.email));
            }
            (ProviderType::Graph, AuthModeChoice::Password) => {
                acct.auth_config.auth_method = vespetrel_core::account::AuthMethod::Password;
                acct.auth_config.username = Some(self.email.clone());
                acct.auth_config.keyring_key = Some(format!("vespetrel_{}", self.email));
            }
            (ProviderType::Jmap, _) => {
                acct.auth_config.auth_method = vespetrel_core::account::AuthMethod::Password;
                acct.auth_config.username = Some(self.email.clone());
                acct.auth_config.keyring_key = Some(format!("vespetrel_jmap_{}", self.email));
            }
        }

        Ok(acct)
    }
}

impl Default for LoginWizardState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_login_wizard_flow() {
        let mut wizard = LoginWizardState::new();
        assert_eq!(wizard.step, WizardStep::SelectProvider);

        wizard.select_provider(ProviderType::Gmail);
        assert_eq!(wizard.incoming_host, "imap.gmail.com");
        assert_eq!(wizard.auth_mode, AuthModeChoice::OAuth2);
        assert_eq!(wizard.step, WizardStep::EnterCredentials);

        wizard.email = "test@gmail.com".into();
        wizard.name = "Test User".into();
        wizard.password_or_token = "oauth_token_123".into();

        let acct = wizard.validate_and_build_account().unwrap();
        assert_eq!(acct.email, "test@gmail.com");
        assert_eq!(acct.name, "Test User");
        assert_eq!(acct.provider_type, ProviderType::Gmail);

        // Test Gmail with App Password (IMAP fallback)
        wizard.auth_mode = AuthModeChoice::Password;
        let acct_imap = wizard.validate_and_build_account().unwrap();
        assert_eq!(acct_imap.email, "test@gmail.com");
        assert_eq!(acct_imap.provider_type, ProviderType::Imap);
    }
}
