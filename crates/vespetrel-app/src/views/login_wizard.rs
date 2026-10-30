//! Interactive GUI Login Wizard Modal §4 & §7 Phase 2 - Thunderbird-Style Setup
use vespetrel_core::{Account, ProviderType};

pub const DEFAULT_GOOGLE_CLIENT_ID: &str =
    "406964657835-aq8lmia8j95dhl1a2bvharmfk3t1hgqj.apps.googleusercontent.com";
pub const DEFAULT_MICROSOFT_CLIENT_ID: &str = "d3590ed6-52b3-4102-aeff-aad2292ab01c";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthModeChoice {
    OAuth2,
    Password,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SocketSecurity {
    #[default]
    SslTls,
    StartTls,
    Plain,
}

impl SocketSecurity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::SslTls => "SSL/TLS",
            Self::StartTls => "STARTTLS",
            Self::Plain => "None / Insecure",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutodiscoverStatus {
    Idle,
    Probing(String),
    Found { source: String, details: String },
    ManualOverride,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredConfig {
    pub provider_name: String,
    pub provider_type: ProviderType,
    pub incoming_host: String,
    pub incoming_port: u16,
    pub incoming_security: SocketSecurity,
    pub incoming_auth: AuthModeChoice,
    pub outgoing_host: String,
    pub outgoing_port: u16,
    pub outgoing_security: SocketSecurity,
    pub outgoing_auth: AuthModeChoice,
    pub username_template: String,
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
    pub incoming_security: SocketSecurity,
    pub outgoing_host: String,
    pub outgoing_port: u16,
    pub outgoing_security: SocketSecurity,
    pub use_tls: bool,
    pub client_id: Option<String>,
    pub oauth_status: Option<String>,
    pub autodiscover_status: AutodiscoverStatus,
    pub is_manual_expanded: bool,
    pub remember_password: bool,
}

impl LoginWizardState {
    pub fn new() -> Self {
        Self {
            step: WizardStep::EnterCredentials,
            provider_type: ProviderType::Imap,
            auth_mode: AuthModeChoice::Password,
            name: String::new(),
            email: String::new(),
            password_or_token: String::new(),
            incoming_host: String::new(),
            incoming_port: 993,
            incoming_security: SocketSecurity::SslTls,
            outgoing_host: String::new(),
            outgoing_port: 587,
            outgoing_security: SocketSecurity::StartTls,
            use_tls: true,
            client_id: None,
            oauth_status: None,
            autodiscover_status: AutodiscoverStatus::Idle,
            is_manual_expanded: false,
            remember_password: true,
        }
    }

    pub fn toggle_manual_config(&mut self) {
        self.is_manual_expanded = !self.is_manual_expanded;
    }

    /// Resolves the effective OAuth2 client ID, honoring user override, environment variables,
    /// or falling back to built-in Thunderbird-compatible client IDs for zero-configuration sign-in.
    pub fn effective_client_id(&self) -> String {
        if let Some(ref id) = self.client_id
            && !id.trim().is_empty()
        {
            return id.trim().to_string();
        }
        match self.provider_type {
            ProviderType::Gmail => std::env::var("VESPETREL_GOOGLE_CLIENT_ID")
                .unwrap_or_else(|_| DEFAULT_GOOGLE_CLIENT_ID.to_string()),
            ProviderType::Graph => std::env::var("VESPETREL_MICROSOFT_CLIENT_ID")
                .unwrap_or_else(|_| DEFAULT_MICROSOFT_CLIENT_ID.to_string()),
            _ => String::new(),
        }
    }

    pub fn select_provider(&mut self, provider: ProviderType) {
        self.provider_type = provider;
        match self.provider_type {
            ProviderType::Gmail => {
                self.incoming_host = "imap.gmail.com".into();
                self.incoming_port = 993;
                self.incoming_security = SocketSecurity::SslTls;
                self.outgoing_host = "smtp.gmail.com".into();
                self.outgoing_port = 587;
                self.outgoing_security = SocketSecurity::StartTls;
                self.auth_mode = AuthModeChoice::OAuth2;
                self.autodiscover_status = AutodiscoverStatus::Found {
                    source: "Google (Gmail / Google Workspace)".into(),
                    details: "IMAP imap.gmail.com:993 (SSL/TLS) · SMTP smtp.gmail.com:587 (STARTTLS) · OAuth2".into(),
                };
            }
            ProviderType::Graph => {
                self.incoming_host = "outlook.office365.com".into();
                self.incoming_port = 993;
                self.incoming_security = SocketSecurity::SslTls;
                self.outgoing_host = "smtp.office365.com".into();
                self.outgoing_port = 587;
                self.outgoing_security = SocketSecurity::StartTls;
                self.auth_mode = AuthModeChoice::OAuth2;
                self.autodiscover_status = AutodiscoverStatus::Found {
                    source: "Microsoft 365 / Exchange Online".into(),
                    details: "Exchange / Graph (outlook.office365.com) · OAuth2".into(),
                };
            }
            ProviderType::Jmap => {
                self.incoming_host = "api.fastmail.com".into();
                self.incoming_port = 443;
                self.incoming_security = SocketSecurity::SslTls;
                self.outgoing_host = "api.fastmail.com".into();
                self.outgoing_port = 443;
                self.outgoing_security = SocketSecurity::SslTls;
                self.auth_mode = AuthModeChoice::Password;
                self.autodiscover_status = AutodiscoverStatus::Found {
                    source: "Fastmail (JMAP)".into(),
                    details: "JMAP Push Mail (api.fastmail.com:443) · API Token / Password".into(),
                };
            }
            ProviderType::Imap => {
                self.incoming_host = String::new();
                self.incoming_port = 993;
                self.incoming_security = SocketSecurity::SslTls;
                self.outgoing_host = String::new();
                self.outgoing_port = 587;
                self.outgoing_security = SocketSecurity::StartTls;
                self.auth_mode = AuthModeChoice::Password;
                self.autodiscover_status = AutodiscoverStatus::ManualOverride;
            }
        }
        self.step = WizardStep::EnterCredentials;
    }

    /// Automatically inspects email domain to populate provider, hostnames, ports, and auth mode
    /// matching Mozilla Thunderbird autodiscover heuristics and well-known catalog.
    pub fn apply_autodiscover_for_email(&mut self, email: &str) {
        let email = email.trim();
        if let Some((_, domain)) = email.split_once('@') {
            let domain_lower = domain.to_lowercase();
            match domain_lower.as_str() {
                "gmail.com" | "googlemail.com" => {
                    self.provider_type = ProviderType::Gmail;
                    self.incoming_host = "imap.gmail.com".into();
                    self.incoming_port = 993;
                    self.incoming_security = SocketSecurity::SslTls;
                    self.outgoing_host = "smtp.gmail.com".into();
                    self.outgoing_port = 587;
                    self.outgoing_security = SocketSecurity::StartTls;
                    self.auth_mode = AuthModeChoice::OAuth2;
                    self.autodiscover_status = AutodiscoverStatus::Found {
                        source: "Google Workspace / Gmail".into(),
                        details: "IMAP imap.gmail.com:993 (SSL/TLS) · SMTP smtp.gmail.com:587 (STARTTLS) · OAuth2".into(),
                    };
                }
                "outlook.com" | "hotmail.com" | "live.com" | "msn.com" | "office365.com"
                | "microsoft.com" => {
                    self.provider_type = ProviderType::Graph;
                    self.incoming_host = "outlook.office365.com".into();
                    self.incoming_port = 993;
                    self.incoming_security = SocketSecurity::SslTls;
                    self.outgoing_host = "smtp.office365.com".into();
                    self.outgoing_port = 587;
                    self.outgoing_security = SocketSecurity::StartTls;
                    self.auth_mode = AuthModeChoice::OAuth2;
                    self.autodiscover_status = AutodiscoverStatus::Found {
                        source: "Microsoft 365 / Exchange".into(),
                        details: "Exchange / Graph (outlook.office365.com) · OAuth2 PKCE".into(),
                    };
                }
                "fastmail.com" | "fastmail.fm" | "messagingengine.com" => {
                    self.provider_type = ProviderType::Jmap;
                    self.incoming_host = "api.fastmail.com".into();
                    self.incoming_port = 443;
                    self.incoming_security = SocketSecurity::SslTls;
                    self.outgoing_host = "api.fastmail.com".into();
                    self.outgoing_port = 443;
                    self.outgoing_security = SocketSecurity::SslTls;
                    self.auth_mode = AuthModeChoice::Password;
                    self.autodiscover_status = AutodiscoverStatus::Found {
                        source: "Fastmail (JMAP)".into(),
                        details: "JMAP Push Mail (api.fastmail.com:443) · API Token / Password"
                            .into(),
                    };
                }
                "icloud.com" | "me.com" | "mac.com" => {
                    self.provider_type = ProviderType::Imap;
                    self.incoming_host = "imap.mail.me.com".into();
                    self.incoming_port = 993;
                    self.incoming_security = SocketSecurity::SslTls;
                    self.outgoing_host = "smtp.mail.me.com".into();
                    self.outgoing_port = 587;
                    self.outgoing_security = SocketSecurity::StartTls;
                    self.auth_mode = AuthModeChoice::Password;
                    self.autodiscover_status = AutodiscoverStatus::Found {
                        source: "Apple iCloud".into(),
                        details: "IMAP imap.mail.me.com:993 (SSL/TLS) · SMTP smtp.mail.me.com:587 (STARTTLS)".into(),
                    };
                }
                "yahoo.com" | "ymail.com" | "aol.com" | "verizon.net" => {
                    self.provider_type = ProviderType::Imap;
                    self.incoming_host = "imap.mail.yahoo.com".into();
                    self.incoming_port = 993;
                    self.incoming_security = SocketSecurity::SslTls;
                    self.outgoing_host = "smtp.mail.yahoo.com".into();
                    self.outgoing_port = 587;
                    self.outgoing_security = SocketSecurity::StartTls;
                    self.auth_mode = AuthModeChoice::Password;
                    self.autodiscover_status = AutodiscoverStatus::Found {
                        source: "Yahoo / AOL Mail".into(),
                        details: "IMAP imap.mail.yahoo.com:993 (SSL/TLS) · SMTP smtp.mail.yahoo.com:587 (STARTTLS)".into(),
                    };
                }
                "zoho.com" | "zohomail.com" => {
                    self.provider_type = ProviderType::Imap;
                    self.incoming_host = "imappro.zoho.com".into();
                    self.incoming_port = 993;
                    self.incoming_security = SocketSecurity::SslTls;
                    self.outgoing_host = "smtppro.zoho.com".into();
                    self.outgoing_port = 587;
                    self.outgoing_security = SocketSecurity::StartTls;
                    self.auth_mode = AuthModeChoice::Password;
                    self.autodiscover_status = AutodiscoverStatus::Found {
                        source: "Zoho Mail".into(),
                        details: "IMAP imappro.zoho.com:993 (SSL/TLS) · SMTP smtppro.zoho.com:587 (STARTTLS)".into(),
                    };
                }
                "protonmail.com" | "pm.me" => {
                    self.provider_type = ProviderType::Imap;
                    self.incoming_host = "127.0.0.1".into();
                    self.incoming_port = 1143;
                    self.incoming_security = SocketSecurity::StartTls;
                    self.outgoing_host = "127.0.0.1".into();
                    self.outgoing_port = 1025;
                    self.outgoing_security = SocketSecurity::StartTls;
                    self.auth_mode = AuthModeChoice::Password;
                    self.autodiscover_status = AutodiscoverStatus::Found {
                        source: "ProtonMail Bridge".into(),
                        details: "Local Bridge IMAP 127.0.0.1:1143 · SMTP 127.0.0.1:1025".into(),
                    };
                }
                "gmx.com" | "gmx.net" => {
                    self.provider_type = ProviderType::Imap;
                    self.incoming_host = "imap.gmx.com".into();
                    self.incoming_port = 993;
                    self.incoming_security = SocketSecurity::SslTls;
                    self.outgoing_host = "mail.gmx.com".into();
                    self.outgoing_port = 587;
                    self.outgoing_security = SocketSecurity::StartTls;
                    self.auth_mode = AuthModeChoice::Password;
                    self.autodiscover_status = AutodiscoverStatus::Found {
                        source: "GMX Mail".into(),
                        details:
                            "IMAP imap.gmx.com:993 (SSL/TLS) · SMTP mail.gmx.com:587 (STARTTLS)"
                                .into(),
                    };
                }
                "web.de" => {
                    self.provider_type = ProviderType::Imap;
                    self.incoming_host = "imap.web.de".into();
                    self.incoming_port = 993;
                    self.incoming_security = SocketSecurity::SslTls;
                    self.outgoing_host = "smtp.web.de".into();
                    self.outgoing_port = 587;
                    self.outgoing_security = SocketSecurity::StartTls;
                    self.auth_mode = AuthModeChoice::Password;
                    self.autodiscover_status = AutodiscoverStatus::Found {
                        source: "WEB.DE".into(),
                        details: "IMAP imap.web.de:993 (SSL/TLS) · SMTP smtp.web.de:587 (STARTTLS)"
                            .into(),
                    };
                }
                "mail.ru" => {
                    self.provider_type = ProviderType::Imap;
                    self.incoming_host = "imap.mail.ru".into();
                    self.incoming_port = 993;
                    self.incoming_security = SocketSecurity::SslTls;
                    self.outgoing_host = "smtp.mail.ru".into();
                    self.outgoing_port = 465;
                    self.outgoing_security = SocketSecurity::SslTls;
                    self.auth_mode = AuthModeChoice::Password;
                    self.autodiscover_status = AutodiscoverStatus::Found {
                        source: "Mail.ru".into(),
                        details:
                            "IMAP imap.mail.ru:993 (SSL/TLS) · SMTP smtp.mail.ru:465 (SSL/TLS)"
                                .into(),
                    };
                }
                "yandex.com" | "yandex.ru" => {
                    self.provider_type = ProviderType::Imap;
                    self.incoming_host = "imap.yandex.com".into();
                    self.incoming_port = 993;
                    self.incoming_security = SocketSecurity::SslTls;
                    self.outgoing_host = "smtp.yandex.com".into();
                    self.outgoing_port = 465;
                    self.outgoing_security = SocketSecurity::SslTls;
                    self.auth_mode = AuthModeChoice::Password;
                    self.autodiscover_status = AutodiscoverStatus::Found {
                        source: "Yandex Mail".into(),
                        details: "IMAP imap.yandex.com:993 (SSL/TLS) · SMTP smtp.yandex.com:465 (SSL/TLS)".into(),
                    };
                }
                _ => {
                    self.provider_type = ProviderType::Imap;
                    self.incoming_host = format!("imap.{domain_lower}");
                    self.incoming_port = 993;
                    self.incoming_security = SocketSecurity::SslTls;
                    self.outgoing_host = format!("smtp.{domain_lower}");
                    self.outgoing_port = 587;
                    self.outgoing_security = SocketSecurity::StartTls;
                    self.auth_mode = AuthModeChoice::Password;
                    self.autodiscover_status = AutodiscoverStatus::Found {
                        source: "Standard mail server heuristics".into(),
                        details: format!(
                            "IMAP imap.{domain_lower}:993 (SSL/TLS) · SMTP smtp.{domain_lower}:587 (STARTTLS)"
                        ),
                    };
                }
            }
        }
    }

    /// Applies configuration discovered from Mozilla ISPDB XML or Active Autodiscover
    pub fn apply_discovered_config(&mut self, config: DiscoveredConfig) {
        self.provider_type = config.provider_type;
        self.incoming_host = config.incoming_host.clone();
        self.incoming_port = config.incoming_port;
        self.incoming_security = config.incoming_security;
        self.outgoing_host = config.outgoing_host.clone();
        self.outgoing_port = config.outgoing_port;
        self.outgoing_security = config.outgoing_security;
        self.auth_mode = config.incoming_auth;

        let auth_str = match self.auth_mode {
            AuthModeChoice::OAuth2 => "OAuth2",
            AuthModeChoice::Password => "Password",
        };

        self.autodiscover_status = AutodiscoverStatus::Found {
            source: format!("Mozilla ISPDB ({})", config.provider_name),
            details: format!(
                "IMAP {}:{} ({}) · SMTP {}:{} ({}) · {}",
                self.incoming_host,
                self.incoming_port,
                self.incoming_security.label(),
                self.outgoing_host,
                self.outgoing_port,
                self.outgoing_security.label(),
                auth_str
            ),
        };
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

        if !self.incoming_host.trim().is_empty() {
            acct.auth_config.server_host = Some(self.incoming_host.trim().to_string());
            acct.auth_config.server_port = Some(self.incoming_port);
        }

        Ok(acct)
    }
}

impl Default for LoginWizardState {
    fn default() -> Self {
        Self::new()
    }
}

// -----------------------------------------------------------------------------
// Mozilla ISPDB XML Autoconfiguration Parser §4
// -----------------------------------------------------------------------------

fn extract_tag_value(xml: &str, tag: &str) -> Option<String> {
    let open_tag = format!("<{tag}>");
    let close_tag = format!("</{tag}>");
    let start = xml.find(&open_tag)? + open_tag.len();
    let end = xml[start..].find(&close_tag)? + start;
    Some(xml[start..end].trim().to_string())
}

fn extract_section<'a>(xml: &'a str, start_tag_prefix: &str, end_tag: &str) -> Option<&'a str> {
    let start_idx = xml.find(start_tag_prefix)?;
    let after_start = &xml[start_idx..];
    let tag_close = after_start.find('>')?;
    let content_start = start_idx + tag_close + 1;
    let end_idx = xml[content_start..].find(end_tag)? + content_start;
    Some(&xml[content_start..end_idx])
}

/// Parses Mozilla ISPDB clientConfig XML (v1.1 format) into a structured DiscoveredConfig
pub fn parse_mozilla_ispdb_xml(xml: &str) -> Option<DiscoveredConfig> {
    let provider_name = extract_tag_value(xml, "displayName")
        .or_else(|| extract_tag_value(xml, "displayShortName"))
        .unwrap_or_else(|| "Mail Provider".into());

    // Extract incomingServer (prefer imap)
    let incoming_section =
        extract_section(xml, "<incomingServer type=\"imap\"", "</incomingServer>")
            .or_else(|| extract_section(xml, "<incomingServer", "</incomingServer>"))?;

    let in_host = extract_tag_value(incoming_section, "hostname")?;
    let in_port = extract_tag_value(incoming_section, "port")
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(993);

    let in_socket = extract_tag_value(incoming_section, "socketType").unwrap_or_default();
    let in_security = if in_socket.eq_ignore_ascii_case("ssl") {
        SocketSecurity::SslTls
    } else if in_socket.eq_ignore_ascii_case("starttls") {
        SocketSecurity::StartTls
    } else {
        SocketSecurity::Plain
    };

    let in_auth = extract_tag_value(incoming_section, "authentication").unwrap_or_default();
    let in_auth_mode = if in_auth.eq_ignore_ascii_case("oauth2") {
        AuthModeChoice::OAuth2
    } else {
        AuthModeChoice::Password
    };

    let username_tmpl =
        extract_tag_value(incoming_section, "username").unwrap_or_else(|| "%EMAILADDRESS%".into());

    // Extract outgoingServer (smtp)
    let outgoing_section =
        extract_section(xml, "<outgoingServer type=\"smtp\"", "</outgoingServer>")
            .or_else(|| extract_section(xml, "<outgoingServer", "</outgoingServer>"))?;

    let out_host = extract_tag_value(outgoing_section, "hostname")?;
    let out_port = extract_tag_value(outgoing_section, "port")
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(587);

    let out_socket = extract_tag_value(outgoing_section, "socketType").unwrap_or_default();
    let out_security = if out_socket.eq_ignore_ascii_case("ssl") {
        SocketSecurity::SslTls
    } else if out_socket.eq_ignore_ascii_case("starttls") {
        SocketSecurity::StartTls
    } else {
        SocketSecurity::Plain
    };

    let out_auth = extract_tag_value(outgoing_section, "authentication").unwrap_or_default();
    let out_auth_mode = if out_auth.eq_ignore_ascii_case("oauth2") {
        AuthModeChoice::OAuth2
    } else {
        AuthModeChoice::Password
    };

    // Classify provider type
    let provider_type = if in_host.contains("gmail.com") || in_host.contains("google.com") {
        ProviderType::Gmail
    } else if in_host.contains("office365.com") || in_host.contains("microsoft.com") {
        ProviderType::Graph
    } else if in_host.contains("fastmail") {
        ProviderType::Jmap
    } else {
        ProviderType::Imap
    };

    Some(DiscoveredConfig {
        provider_name,
        provider_type,
        incoming_host: in_host,
        incoming_port: in_port,
        incoming_security: in_security,
        incoming_auth: in_auth_mode,
        outgoing_host: out_host,
        outgoing_port: out_port,
        outgoing_security: out_security,
        outgoing_auth: out_auth_mode,
        username_template: username_tmpl,
    })
}

/// Asynchronously queries Mozilla's live ISPDB Autoconfiguration directory
pub async fn query_mozilla_ispdb(domain: &str) -> Option<DiscoveredConfig> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;

    let url = format!("https://autoconfig.thunderbird.net/v1.1/{}", domain.trim());
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body = resp.text().await.ok()?;
    parse_mozilla_ispdb_xml(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_login_wizard_flow() {
        let mut wizard = LoginWizardState::new();
        assert_eq!(wizard.step, WizardStep::EnterCredentials);

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

    #[test]
    fn test_autodiscover_domains() {
        let mut wizard = LoginWizardState::new();

        wizard.apply_autodiscover_for_email("user@gmail.com");
        assert_eq!(wizard.provider_type, ProviderType::Gmail);
        assert_eq!(wizard.incoming_host, "imap.gmail.com");
        assert_eq!(wizard.auth_mode, AuthModeChoice::OAuth2);

        wizard.apply_autodiscover_for_email("colleague@outlook.com");
        assert_eq!(wizard.provider_type, ProviderType::Graph);
        assert_eq!(wizard.incoming_host, "outlook.office365.com");
        assert_eq!(wizard.auth_mode, AuthModeChoice::OAuth2);

        wizard.apply_autodiscover_for_email("privacy@fastmail.com");
        assert_eq!(wizard.provider_type, ProviderType::Jmap);
        assert_eq!(wizard.incoming_host, "api.fastmail.com");

        wizard.apply_autodiscover_for_email("custom@mycorp.net");
        assert_eq!(wizard.provider_type, ProviderType::Imap);
        assert_eq!(wizard.incoming_host, "imap.mycorp.net");
        assert_eq!(wizard.outgoing_host, "smtp.mycorp.net");
        assert_eq!(wizard.incoming_port, 993);
        assert_eq!(wizard.outgoing_port, 587);
        assert_eq!(wizard.incoming_security, SocketSecurity::SslTls);
        assert_eq!(wizard.outgoing_security, SocketSecurity::StartTls);
    }

    #[test]
    fn test_mozilla_ispdb_xml_parsing() {
        let sample_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<clientConfig version="1.1">
  <emailProvider id="example.com">
    <domain>example.com</domain>
    <displayName>Example Corp Mail</displayName>
    <displayShortName>Example</displayShortName>
    <incomingServer type="imap">
      <hostname>imap.example.com</hostname>
      <port>993</port>
      <socketType>SSL</socketType>
      <authentication>password-cleartext</authentication>
      <username>%EMAILADDRESS%</username>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>smtp.example.com</hostname>
      <port>587</port>
      <socketType>STARTTLS</socketType>
      <authentication>password-cleartext</authentication>
      <username>%EMAILADDRESS%</username>
    </outgoingServer>
  </emailProvider>
</clientConfig>"#;

        let parsed = parse_mozilla_ispdb_xml(sample_xml).expect("Should parse Mozilla ISPDB XML");
        assert_eq!(parsed.provider_name, "Example Corp Mail");
        assert_eq!(parsed.incoming_host, "imap.example.com");
        assert_eq!(parsed.incoming_port, 993);
        assert_eq!(parsed.incoming_security, SocketSecurity::SslTls);
        assert_eq!(parsed.incoming_auth, AuthModeChoice::Password);
        assert_eq!(parsed.outgoing_host, "smtp.example.com");
        assert_eq!(parsed.outgoing_port, 587);
        assert_eq!(parsed.outgoing_security, SocketSecurity::StartTls);
        assert_eq!(parsed.outgoing_auth, AuthModeChoice::Password);
        assert_eq!(parsed.username_template, "%EMAILADDRESS%");
        assert_eq!(parsed.provider_type, ProviderType::Imap);

        let mut wizard = LoginWizardState::new();
        wizard.apply_discovered_config(parsed);
        assert_eq!(wizard.incoming_host, "imap.example.com");
        assert_eq!(wizard.outgoing_host, "smtp.example.com");
        assert!(matches!(
            wizard.autodiscover_status,
            AutodiscoverStatus::Found { .. }
        ));
    }

    #[test]
    fn test_mozilla_ispdb_oauth2_parsing() {
        let google_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<clientConfig version="1.1">
  <emailProvider id="googlemail.com">
    <domain>gmail.com</domain>
    <displayName>Google Mail</displayName>
    <incomingServer type="imap">
      <hostname>imap.gmail.com</hostname>
      <port>993</port>
      <socketType>SSL</socketType>
      <authentication>OAuth2</authentication>
      <username>%EMAILADDRESS%</username>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>smtp.gmail.com</hostname>
      <port>465</port>
      <socketType>SSL</socketType>
      <authentication>OAuth2</authentication>
      <username>%EMAILADDRESS%</username>
    </outgoingServer>
  </emailProvider>
</clientConfig>"#;

        let parsed = parse_mozilla_ispdb_xml(google_xml).expect("Should parse Google ISPDB XML");
        assert_eq!(parsed.provider_name, "Google Mail");
        assert_eq!(parsed.provider_type, ProviderType::Gmail);
        assert_eq!(parsed.incoming_auth, AuthModeChoice::OAuth2);
        assert_eq!(parsed.outgoing_auth, AuthModeChoice::OAuth2);
        assert_eq!(parsed.incoming_security, SocketSecurity::SslTls);
        assert_eq!(parsed.outgoing_security, SocketSecurity::SslTls);
    }

    #[test]
    fn test_manual_config_and_default_client_ids() {
        let mut wizard = LoginWizardState::new();
        assert!(!wizard.is_manual_expanded);
        wizard.toggle_manual_config();
        assert!(wizard.is_manual_expanded);

        // Check Google default client ID fallback
        wizard.provider_type = ProviderType::Gmail;
        assert_eq!(wizard.effective_client_id(), DEFAULT_GOOGLE_CLIENT_ID);

        // Check Microsoft default client ID fallback
        wizard.provider_type = ProviderType::Graph;
        assert_eq!(wizard.effective_client_id(), DEFAULT_MICROSOFT_CLIENT_ID);

        // Check explicit override
        wizard.client_id = Some("my-custom-client-id-12345".into());
        assert_eq!(wizard.effective_client_id(), "my-custom-client-id-12345");
    }
}
