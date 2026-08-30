use oauth2::{CsrfToken, PkceCodeChallenge};
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
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        info!("starting loopback listener on 127.0.0.1:8989 for OAuth2 callback");
        let listener = TcpListener::bind("127.0.0.1:8989").await?;

        let (mut socket, _) = listener.accept().await?;
        let mut buf = [0u8; 4096];
        let n = socket.read(&mut buf).await?;
        let request_str = String::from_utf8_lossy(&buf[..n]);

        let code = parse_code_from_http_request(&request_str)
            .ok_or_else(|| anyhow::anyhow!("failed to extract authorization code from callback request"))?;

        // Respond with friendly confirmation page
        let response_body = r#"<!DOCTYPE html>
<html>
<head><title>Vespetrel Authorization</title></head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; text-align: center; padding: 60px; background: #09090b; color: #fafafa;">
    <h1 style="font-size: 2rem; margin-bottom: 12px;">🕊️ Authorization Successful</h1>
    <p style="color: #a1a1aa; font-size: 1.1rem;">Your account has been connected to <strong>Vespetrel</strong>.</p>
    <p style="color: #71717a; font-size: 0.9rem; margin-top: 24px;">You may safely close this browser window.</p>
</body>
</html>"#;

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );

        let _ = socket.write_all(response.as_bytes()).await;
        let _ = socket.flush().await;

        info!("successfully captured OAuth2 authorization code");
        Ok(code)
    }

    pub async fn exchange_code(&self, code: String, pkce_verifier: String) -> anyhow::Result<TokenBundle> {
        info!(token_url=%self.config.token_url, "exchanging code for OAuth2 tokens");
        let client = reqwest::Client::builder()
            .user_agent("Vespetrel/0.1 OAuth2")
            .build()?;

        let mut params = std::collections::HashMap::new();
        params.insert("client_id", self.config.client_id.as_str());
        params.insert("code", code.as_str());
        params.insert("redirect_uri", self.config.redirect_uri.as_str());
        params.insert("grant_type", "authorization_code");
        params.insert("code_verifier", pkce_verifier.as_str());

        if let Some(secret) = &self.config.client_secret {
            params.insert("client_secret", secret.as_str());
        }

        let resp = client
            .post(&self.config.token_url)
            .form(&params)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("token exchange failed with status {}: {}", status, body);
        }

        let body: serde_json::Value = resp.json().await?;
        let access_token = body["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing access_token in token response"))?
            .to_string();

        let refresh_token = body["refresh_token"].as_str().map(|s| s.to_string());
        let expires_in = body["expires_in"].as_u64().unwrap_or(3600);

        Ok(TokenBundle {
            access_token,
            refresh_token,
            expires_in,
        })
    }
}

/// Parse `code=...` query parameter from raw HTTP request line
pub fn parse_code_from_http_request(req: &str) -> Option<String> {
    let first_line = req.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split('?').nth(1)?;

    for pair in query.split('&') {
        let mut parts = pair.split('=');
        if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
            if k == "code" {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn urlencoding(s: &str) -> String {
    s.replace(' ', "%20")
        .replace(':', "%3A")
        .replace('/', "%2F")
        .replace('?', "%3F")
        .replace('&', "%26")
        .replace('=', "%3D")
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenBundle {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_code_from_http_get() {
        let req = "GET /callback?code=4/0AbCdEf123456&state=csrf-test HTTP/1.1\r\nHost: 127.0.0.1:8989\r\n\r\n";
        let code = parse_code_from_http_request(req);
        assert_eq!(code.as_deref(), Some("4/0AbCdEf123456"));
    }

    #[test]
    fn parse_code_missing() {
        let req = "GET /callback?error=access_denied HTTP/1.1\r\nHost: 127.0.0.1:8989\r\n\r\n";
        let code = parse_code_from_http_request(req);
        assert_eq!(code, None);
    }
}
