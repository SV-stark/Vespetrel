use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, RedirectUrl,
    Scope, TokenResponse, TokenUrl,
};
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct OAuth2Config {
    pub client_id: String,
    pub client_secret: Option<String>, // None for PKCE public clients
    pub auth_url: String,
    pub token_url: String,
    pub redirect_uri: String, // http://127.0.0.1:8989/callback
    pub scopes: Vec<String>,
}

impl OAuth2Config {
    pub fn google(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: None,
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token_url: "https://oauth2.googleapis.com/token".into(),
            redirect_uri: "http://127.0.0.1:8989/callback".into(),
            scopes: vec!["https://mail.google.com/".into(), "openid".into(), "email".into()],
        }
    }

    pub fn microsoft(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: None,
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".into(),
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".into(),
            redirect_uri: "http://127.0.0.1:8989/callback".into(),
            scopes: vec!["https://outlook.office.com/IMAP.AccessAsUser.All".into(), "https://outlook.office.com/SMTP.Send".into(), "offline_access".into()],
        }
    }
}

/// OAuth2 PKCE engine with local loopback listener §4.5
pub struct OAuth2Engine {
    config: OAuth2Config,
}

impl OAuth2Engine {
    pub fn new(config: OAuth2Config) -> Self { Self { config } }

    /// Generate PKCE authorization URL to open in browser
    pub fn auth_url(&self) -> (String, CsrfToken, PkceCodeChallenge) {
        // Real: use oauth2 crate's Client with PKCE
        let (pkce_challenge, _verifier) = PkceCodeChallenge::new_random_sha256();
        let auth_url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256",
            self.config.auth_url,
            urlencoding(&self.config.client_id),
            urlencoding(&self.config.redirect_uri),
            urlencoding(&self.config.scopes.join(" ")),
            pkce_challenge.as_str(),
        );
        // Store verifier + csrf in keyring/state for callback
        debug!(url=%auth_url, "generated PKCE auth url");
        (auth_url, CsrfToken::new("csrf-stub".into()), pkce_challenge)
    }

    /// Start local loopback listener on 127.0.0.1:8989 and wait for ?code=
    pub async fn wait_for_callback(&self) -> anyhow::Result<String> {
        info!("starting loopback listener on 127.0.0.1:8989/callback");
        // Real: tokio::net::TcpListener + parse HTTP request line for code
        // For now, stub that would block; return error instructing manual paste
        anyhow::bail!("loopback listener stub - paste code manually via exchange_code()")
    }

    pub async fn exchange_code(&self, _code: String, _pkce_verifier: String) -> anyhow::Result<TokenBundle> {
        // Real: POST to token_url with code_verifier
        info!("exchanging code for tokens (stub)");
        Ok(TokenBundle { access_token: "stub-access".into(), refresh_token: Some("stub-refresh".into()), expires_in: 3600 })
    }
}

fn urlencoding(s: &str) -> String {
    // minimal percent-encode for demo; real code uses oauth2 crate's URL builder
    s.replace(' ', "%20").replace(':', "%3A").replace('/', "%2F").replace('?', "%3F").replace('&', "%26").replace('=', "%3D")
}

#[derive(Debug, Clone)]
pub struct TokenBundle {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}
