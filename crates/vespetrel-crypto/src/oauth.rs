use oauth2::{CsrfToken, PkceCodeChallenge, PkceCodeVerifier};
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
            scopes: vec![
                "https://mail.google.com/".into(),
                "openid".into(),
                "email".into(),
            ],
        }
    }

    pub fn microsoft(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: None,
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".into(),
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".into(),
            redirect_uri: "http://127.0.0.1:8989/callback".into(),
            scopes: vec![
                "https://outlook.office.com/IMAP.AccessAsUser.All".into(),
                "https://outlook.office.com/SMTP.Send".into(),
                "offline_access".into(),
            ],
        }
    }
}

/// OAuth2 PKCE engine with local loopback listener §4.5
pub struct OAuth2Engine {
    config: OAuth2Config,
}

impl OAuth2Engine {
    pub fn new(config: OAuth2Config) -> Self {
        Self { config }
    }

    /// Generate PKCE authorization URL to open in browser along with random CSRF token and PKCE verifier
    pub fn auth_url(&self) -> (String, CsrfToken, PkceCodeVerifier) {
        let (pkce_challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let csrf_token = CsrfToken::new_random();
        let auth_url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
            self.config.auth_url,
            urlencoding(&self.config.client_id),
            urlencoding(&self.config.redirect_uri),
            urlencoding(&self.config.scopes.join(" ")),
            urlencoding(csrf_token.secret()),
            pkce_challenge.as_str(),
        );
        debug!(url=%auth_url, "generated PKCE auth url");
        (auth_url, csrf_token, verifier)
    }

    pub fn redirect_port(&self) -> u16 {
        if let Some(rest) = self.config.redirect_uri.split("://").nth(1) {
            let host_port = rest.split('/').next().unwrap_or("");
            if let Some(port_str) = host_port.split(':').nth(1)
                && let Ok(p) = port_str.parse::<u16>()
            {
                return p;
            }
        }
        8989
    }

    /// Set dynamic port and update redirect_uri
    pub fn set_redirect_port(&mut self, port: u16) {
        self.config.redirect_uri = format!("http://127.0.0.1:{port}/callback");
    }

    /// Bind a loopback listener to 127.0.0.1 (using port 0 for OS-assigned ephemeral port)
    pub async fn bind_loopback() -> std::io::Result<(tokio::net::TcpListener, u16)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        Ok((listener, port))
    }

    /// Wait for loopback OAuth2 redirect on an already bound TcpListener
    pub async fn wait_for_callback_on_listener(
        &self,
        listener: tokio::net::TcpListener,
        timeout_secs: u64,
        expected_state: Option<&str>,
    ) -> anyhow::Result<String> {
        use std::time::Duration;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let future = async {
            let (mut socket, _) = listener.accept().await?;
            let mut request_bytes = Vec::new();
            let mut chunk = [0u8; 1024];

            // Read until complete HTTP header delimiter \r\n\r\n or \n\n (up to 16KB limit)
            while request_bytes.len() < 16384 {
                let n = socket.read(&mut chunk).await?;
                if n == 0 {
                    break;
                }
                request_bytes.extend_from_slice(&chunk[..n]);
                if request_bytes.windows(4).any(|w| w == b"\r\n\r\n")
                    || request_bytes.windows(2).any(|w| w == b"\n\n")
                {
                    break;
                }
            }

            let request_str = String::from_utf8_lossy(&request_bytes);

            // Validate Host header to prevent DNS rebinding / host injection
            let host_header = request_str
                .lines()
                .find(|l| l.to_lowercase().starts_with("host:"))
                .and_then(|l| l.split_once(':').map(|(_, v)| v.trim()))
                .unwrap_or("");

            if !host_header.starts_with("127.0.0.1") && !host_header.starts_with("localhost") {
                anyhow::bail!("OAuth2 invalid Host header: security check failed");
            }

            if let Some(expected) = expected_state {
                let state = parse_param_from_http_request(&request_str, "state");
                if state.as_deref() != Some(expected) {
                    anyhow::bail!("OAuth2 CSRF state mismatch: security check failed");
                }
            }

            let code = parse_code_from_http_request(&request_str).ok_or_else(|| {
                anyhow::anyhow!("failed to extract authorization code from callback request")
            })?;

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
        };

        tokio::time::timeout(Duration::from_secs(timeout_secs), future)
            .await
            .map_err(|_| {
                anyhow::anyhow!("OAuth2 callback listener timed out after {timeout_secs}s")
            })?
    }

    /// Wait for loopback OAuth2 redirect with customizable timeout, Host check and CSRF state check
    pub async fn wait_for_callback(
        &self,
        timeout_secs: u64,
        expected_state: Option<&str>,
    ) -> anyhow::Result<String> {
        let port = self.redirect_port();
        let addr = format!("127.0.0.1:{port}");
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        self.wait_for_callback_on_listener(listener, timeout_secs, expected_state)
            .await
    }

    pub async fn exchange_code(
        &self,
        code: String,
        pkce_verifier: String,
    ) -> anyhow::Result<TokenBundle> {
        use std::time::Duration;
        use zeroize::Zeroizing;

        info!(token_url=%self.config.token_url, "exchanging code for OAuth2 tokens");
        let client = reqwest::Client::builder()
            .user_agent("Vespetrel/0.1 OAuth2")
            .timeout(Duration::from_secs(10))
            .build()?;

        let zero_verifier = Zeroizing::new(pkce_verifier);
        let mut params = std::collections::HashMap::new();
        params.insert("client_id", self.config.client_id.as_str());
        params.insert("code", code.as_str());
        params.insert("redirect_uri", self.config.redirect_uri.as_str());
        params.insert("grant_type", "authorization_code");
        params.insert("code_verifier", zero_verifier.as_str());

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

    /// Refresh an expired access token using the long-lived refresh_token
    pub async fn refresh_access_token(&self, refresh_token: &str) -> anyhow::Result<TokenBundle> {
        use std::time::Duration;

        info!(token_url=%self.config.token_url, "refreshing expired OAuth2 access token");
        let client = reqwest::Client::builder()
            .user_agent("Vespetrel/0.1 OAuth2")
            .timeout(Duration::from_secs(10))
            .build()?;

        let mut params = std::collections::HashMap::new();
        params.insert("client_id", self.config.client_id.as_str());
        params.insert("refresh_token", refresh_token);
        params.insert("grant_type", "refresh_token");

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
            anyhow::bail!("token refresh failed with status {}: {}", status, body);
        }

        let body: serde_json::Value = resp.json().await?;
        let access_token = body["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing access_token in refresh response"))?
            .to_string();

        let new_refresh = body["refresh_token"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| Some(refresh_token.to_string()));
        let expires_in = body["expires_in"].as_u64().unwrap_or(3600);

        Ok(TokenBundle {
            access_token,
            refresh_token: new_refresh,
            expires_in,
        })
    }
}

/// Parse named query parameter from raw HTTP request line with URL decoding
pub fn parse_param_from_http_request(req: &str, param: &str) -> Option<String> {
    let first_line = req.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split('?').nth(1)?;

    for pair in query.split('&') {
        if let Some((_, v)) = pair.split_once('=').filter(|(k, _)| *k == param) {
            return Some(urldecode(v));
        }
    }

    None
}

/// Parse `code=...` query parameter from raw HTTP request line
pub fn parse_code_from_http_request(req: &str) -> Option<String> {
    parse_param_from_http_request(req, "code")
}

fn urlencoding(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

pub fn urldecode(input: &str) -> String {
    let replaced = input.replace('+', " ");
    match urlencoding::decode(&replaced) {
        Ok(cow) => cow.into_owned(),
        Err(_) => replaced,
    }
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop,
)]
pub struct TokenBundle {
    pub access_token: String,
    pub refresh_token: Option<String>,
    #[zeroize(skip)]
    pub expires_in: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_code_missing() {
        let req = "GET /callback?error=access_denied HTTP/1.1\r\nHost: 127.0.0.1:8989\r\n\r\n";
        let code = parse_code_from_http_request(req);
        assert_eq!(code, None);
    }

    #[test]
    fn test_urldecode_and_urlencode() {
        let raw = "hello world+test%2F123&state=abc";
        assert_eq!(
            urldecode("hello%20world%2Btest%2F123%26state%3Dabc"),
            "hello world+test/123&state=abc"
        );
        assert_eq!(urldecode("a+b"), "a b");
        let encoded = urlencoding(raw);
        assert!(encoded.contains("%20") || encoded.contains("%2F"));
    }
}
