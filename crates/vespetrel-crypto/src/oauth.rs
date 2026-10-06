use oauth2::{CsrfToken, PkceCodeChallenge, PkceCodeVerifier};
use tracing::{debug, info};

#[derive(Clone)]
pub struct OAuth2Config {
    pub client_id: String,
    pub client_secret: Option<String>, // None for PKCE public clients
    pub auth_url: String,
    pub token_url: String,
    pub redirect_uri: String, // http://127.0.0.1:8989/
    pub scopes: Vec<String>,
    /// Request an offline (refreshable) grant.
    ///
    /// Google defaults to `access_type=online`, under which the authorization
    /// server may return **no** `refresh_token` at all — and on a repeat sign-in it
    /// omits one even when the first consent issued one. Vespetrel stored the account
    /// anyway, leaving it with a one-hour token and no way to renew it, so every
    /// sign-in broke once the token expired.
    pub offline_access: bool,
}

impl std::fmt::Debug for OAuth2Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuth2Config")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("auth_url", &self.auth_url)
            .field("token_url", &self.token_url)
            .field("redirect_uri", &self.redirect_uri)
            .field("scopes", &self.scopes)
            .finish()
    }
}

impl OAuth2Config {
    pub fn google(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: None,
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token_url: "https://oauth2.googleapis.com/token".into(),
            // Google's loopback rule matches on the *host* only: the port is dynamic and
            // the path is not compared. Using `/callback` was harmless, but a bare `/`
            // is what Google's own desktop-app samples register.
            redirect_uri: "http://127.0.0.1:8989/".into(),
            scopes: vec![
                "https://mail.google.com/".into(),
                "openid".into(),
                "email".into(),
                "profile".into(),
            ],
            offline_access: true,
        }
    }

    pub fn microsoft(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: None,
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".into(),
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".into(),
            redirect_uri: "http://127.0.0.1:8989/".into(),
            scopes: vec![
                "https://outlook.office.com/IMAP.AccessAsUser.All".into(),
                "https://outlook.office.com/SMTP.Send".into(),
                "offline_access".into(),
                "openid".into(),
                "email".into(),
                "profile".into(),
            ],
            offline_access: true,
        }
    }
}

/// A loopback listener holding one socket per address family on a single port.
///
/// Dropping this closes every socket, which unblocks a `wait_for_callback` parked in `accept`.
pub struct LoopbackListener {
    v4: tokio::net::TcpListener,
    v6: Option<tokio::net::TcpListener>,
    pub port: u16,
}

impl std::fmt::Debug for LoopbackListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopbackListener")
            .field("port", &self.port)
            .field("ipv6", &self.v6.is_some())
            .finish()
    }
}

/// Why a loopback callback was not accepted.
///
/// A denial is terminal (the user answered); everything else is noise to be ignored while the
/// listener keeps waiting for the real redirect.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CallbackRejection {
    /// Provider redirected back with `error=...`: consent was declined or a scope refused.
    Denied(String),
    /// Speculative connection, favicon probe, CSRF mismatch: keep listening.
    Transient(String),
}

impl std::fmt::Display for CallbackRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied(reason) => write!(f, "authorization was not completed ({reason})"),
            Self::Transient(reason) => write!(f, "{reason}"),
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
        let mut auth_url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
            self.config.auth_url,
            urlencoding(&self.config.client_id),
            urlencoding(&self.config.redirect_uri),
            urlencoding(&self.config.scopes.join(" ")),
            urlencoding(csrf_token.secret()),
            pkce_challenge.as_str(),
        );
        if self.config.offline_access {
            // `access_type=offline` asks for a refresh token; `prompt=consent` forces the
            // consent screen to reappear, because otherwise Google silently reuses the
            // standing grant and returns no `refresh_token` on the second and later
            // sign-ins. `include_granted_scopes` keeps previously consented scopes.
            auth_url.push_str("&access_type=offline&prompt=consent&include_granted_scopes=true");
        }
        // Never log the full authorization URL: it carries the live CSRF `state`, and a log
        // reader could hijack an in-flight sign-in.
        debug!(host=%self.config.auth_url, offline_access=self.config.offline_access, "generated PKCE auth url");
        (auth_url, csrf_token, verifier)
    }

    /// Loopback port the browser will be redirected to.
    ///
    /// Parsed with `url` rather than by string splitting: `http://[::1]:8989/callback`
    /// previously yielded an empty port and fell back to 8989, which happened to be right by
    /// luck, and a redirect URI with no explicit port always bound the wrong port.
    pub fn redirect_port(&self) -> u16 {
        match url::Url::parse(&self.config.redirect_uri) {
            Ok(u) => u.port_or_known_default().unwrap_or(8989),
            Err(_) => 8989,
        }
    }

    /// Set dynamic port and update redirect_uri
    pub fn set_redirect_port(&mut self, port: u16) {
        self.config.redirect_uri = format!("http://127.0.0.1:{port}/");
    }

    /// Bind a loopback listener to 127.0.0.1 (using port 0 for OS-assigned ephemeral port)
    ///
    /// Returns a handle that listens on **both** IPv4 (`127.0.0.1`) and IPv6 (`[::1]`) loopback
    /// on the same port. `localhost` resolves to `::1` first on modern Windows and macOS, so an
    /// IPv4-only listener made the browser report "connection refused" for half of all sign-ins
    /// even though the port was correct. IPv6 binding is best-effort: if the port is unavailable
    /// on the v6 stack we still serve v4.
    pub async fn bind_loopback() -> std::io::Result<LoopbackListener> {
        let v4 = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = v4.local_addr()?.port();
        let v6 = tokio::net::TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, port))
            .await
            .ok();
        debug!(port, ipv6 = v6.is_some(), "bound OAuth2 loopback listener");
        Ok(LoopbackListener { v4, v6, port })
    }

    /// Wait for loopback OAuth2 redirect on an already bound listener.
    ///
    /// Keeps accepting until the deadline. The previous implementation called
    /// `listener.accept()` exactly once, so the first local process to connect — a captive
    /// portal probe, a `favicon.ico` fetch, a second instance of the app — permanently
    /// doSed the sign-in with no retry.
    ///
    /// A browser redirect carrying `error=access_denied` (the user pressed "Cancel", or a
    /// scope was denied) now returns an error naming that code. It used to be reported as a
    /// generic "failed to extract authorization code", which is indistinguishable from a
    /// genuine bug and sent people looking in the wrong place.
    pub async fn wait_for_callback_on_listener(
        &self,
        listener: LoopbackListener,
        timeout_secs: u64,
        expected_state: Option<&str>,
    ) -> anyhow::Result<String> {
        use std::time::Duration;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
        // Bound the loop so a hostile peer cannot spin us indefinitely. Generous: a single
        // sign-in can legitimately draw a dozen speculative connections.
        let mut attempts: u32 = 0;
        const MAX_ATTEMPTS: u32 = 256;

        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                anyhow::bail!("OAuth2 sign-in timed out after {timeout_secs}s");
            }
            attempts += 1;
            if attempts > MAX_ATTEMPTS {
                anyhow::bail!("OAuth2 callback listener exceeded {MAX_ATTEMPTS} rejected requests");
            }

            // Race both address families so a callback arriving on `::1` is not queued behind
            // a parked v4 `accept`.
            let accept = tokio::time::timeout(remaining, async {
                match &listener.v6 {
                    Some(v6) => {
                        tokio::select! {
                            r = listener.v4.accept() => r,
                            r = v6.accept() => r,
                        }
                    }
                    None => listener.v4.accept().await,
                }
            })
            .await;
            let (mut socket, _peer) = match accept {
                Ok(Ok(pair)) => pair,
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => {
                    anyhow::bail!("OAuth2 sign-in timed out after {timeout_secs}s");
                }
            };

            let mut request_bytes = Vec::new();
            let mut chunk = [0u8; 1024];
            // Read until complete HTTP header delimiter \r\n\r\n or \n\n (up to 16KB limit).
            // The read is bounded: browsers open speculative connections to loopback and
            // drop them without ever sending a request line. An unbounded `read` left those
            // sockets parked until the whole sign-in deadline expired.
            let read_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            while request_bytes.len() < 16384 {
                let Ok(n) = tokio::time::timeout_at(read_deadline, socket.read(&mut chunk)).await
                else {
                    break;
                };
                match n {
                    Ok(0) => break,
                    Ok(n) => request_bytes.extend_from_slice(&chunk[..n]),
                    Err(_) => break,
                }
                if request_bytes.windows(4).any(|w| w == b"\r\n\r\n")
                    || request_bytes.windows(2).any(|w| w == b"\n\n")
                {
                    break;
                }
            }

            let request_str = String::from_utf8_lossy(&request_bytes).to_string();

            // Respond (success or failure) so the browser never hangs on a socket we abandon.
            match self.validate_callback(&request_str, expected_state) {
                Ok(code) => {
                    let response = http_response(200, "OK", SUCCESS_PAGE);
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.flush().await;
                    info!("successfully captured OAuth2 authorization code");
                    return Ok(code);
                }
                Err(rejection) => {
                    let terminal = matches!(rejection, CallbackRejection::Denied(_));
                    if terminal {
                        info!(reason=%rejection, "OAuth2 provider declined the authorization");
                    } else {
                        tracing::warn!(reason=%rejection, "rejected OAuth2 loopback request; still listening");
                    }
                    let body = ERROR_PAGE.replace("{REASON}", &rejection.to_string());
                    let response = http_response(400, "Bad Request", &body);
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.flush().await;
                    // A denial is the user's answer. Retrying would just re-show a consent
                    // screen they already dismissed, so end the sign-in instead of looping to
                    // a 15-minute timeout with a misleading message.
                    if terminal {
                        return Err(anyhow::anyhow!("OAuth2 {rejection}"));
                    }
                }
            }
        }
    }

    /// Validate a loopback callback request: provider error, Host allowlist, then CSRF state.
    fn validate_callback(
        &self,
        request_str: &str,
        expected_state: Option<&str>,
    ) -> Result<String, CallbackRejection> {
        // Check the provider's own error first. It used to be reported as a generic "failed to
        // extract authorization code", which is indistinguishable from a real bug — so people
        // who pressed Cancel spent their time debugging the app instead of retrying.
        if let Some(code) = parse_param_from_http_request(request_str, "error") {
            let reason = match parse_param_from_http_request(request_str, "error_description") {
                Some(detail) => format!("{code}: {detail}"),
                None => code,
            };
            return Err(CallbackRejection::Denied(reason));
        }
        self.validate_callback_checked(request_str, expected_state)
    }

    fn validate_callback_checked(
        &self,
        request_str: &str,
        expected_state: Option<&str>,
    ) -> Result<String, CallbackRejection> {
        // Validate Host header strictly to prevent DNS rebinding / host injection
        let host_header = request_str
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("host:"))
            .and_then(|l| l.split_once(':').map(|(_, v)| v.trim()))
            .unwrap_or("");

        if host_header.is_empty()
            || host_header
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || c == '\0' || c == '\t' || c == '%')
        {
            return Err(CallbackRejection::Transient("invalid Host header".into()));
        }

        let host_only = host_header.split(':').next().unwrap_or("").trim();
        if host_only != "127.0.0.1" && host_only != "localhost" && host_only != "[::1]" {
            return Err(CallbackRejection::Transient("invalid Host header".into()));
        }

        if let Some(expected) = expected_state {
            // Constant-time comparison. A `&str` `==` short-circuits on the first differing
            // byte and leaks the matching prefix length.
            let got = parse_param_from_http_request(request_str, "state").unwrap_or_default();
            let ok =
                got.len() == expected.len() && crate::ct_eq(got.as_bytes(), expected.as_bytes());
            if !ok {
                return Err(CallbackRejection::Transient("CSRF state mismatch".into()));
            }
        }

        parse_code_from_http_request(request_str).ok_or_else(|| {
            CallbackRejection::Transient("failed to extract authorization code".into())
        })
    }

    /// Wait for loopback OAuth2 redirect with customizable timeout, Host check and CSRF state check
    pub async fn wait_for_callback(
        &self,
        timeout_secs: u64,
        expected_state: Option<&str>,
    ) -> anyhow::Result<String> {
        let listener = Self::bind_loopback().await?;
        self.wait_for_callback_on_listener(listener, timeout_secs, expected_state)
            .await
    }

    /// Build the token-endpoint HTTP client.
    ///
    /// Redirects are disabled. `token_url` is DB-backed configuration that is reloaded on
    /// every proactive refresh, so a `307`/`308` from the configured host would carry the
    /// `client_secret` and `refresh_token` in the request body to whatever host chose the
    /// redirect. The `oauth2` docs call this out explicitly for SSRF.
    fn token_client() -> anyhow::Result<reqwest::Client> {
        use std::time::Duration;
        Ok(reqwest::Client::builder()
            .user_agent("Vespetrel/0.1 OAuth2")
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?)
    }

    /// Exchange an authorization code for tokens.
    ///
    /// `existing_refresh_token` is carried through when the provider omits one from the
    /// response. Google only issues a `refresh_token` on the *first* consent for a given
    /// client; every later sign-in returns an access token with no refresh token, which used
    /// to overwrite the stored credential and strand the account on a one-hour token.
    pub async fn exchange_code(
        &self,
        code: String,
        pkce_verifier: String,
        existing_refresh_token: Option<String>,
    ) -> anyhow::Result<TokenBundle> {
        use zeroize::Zeroizing;

        info!(token_url=%self.config.token_url, "exchanging code for OAuth2 tokens");
        let client = Self::token_client()?;

        let zero_verifier = Zeroizing::new(pkce_verifier);
        let zero_code = Zeroizing::new(code);
        let mut params = std::collections::HashMap::new();
        params.insert("client_id", self.config.client_id.as_str());
        params.insert("code", zero_code.as_str());
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

        // Read the response as bytes, extract what we need, then scrub the raw buffer: it
        // holds the access, refresh and id tokens in one heap allocation.
        let raw: zeroize::Zeroizing<Vec<u8>> =
            zeroize::Zeroizing::new(resp.bytes().await?.to_vec());
        let body: serde_json::Value = serde_json::from_slice(&raw)
            .map_err(|e| anyhow::anyhow!("malformed token response: {e}"))?;
        let access_token = body["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing access_token in token response"))?
            .to_string();

        let refresh_token = body["refresh_token"]
            .as_str()
            .map(|s| s.to_string())
            .or(existing_refresh_token.map(|s| s.to_string()));
        let expires_in = parse_expires_in(&body)?;

        Ok(TokenBundle {
            access_token,
            refresh_token,
            expires_in,
            obtained_at: chrono::Utc::now().timestamp(),
        })
    }

    /// Refresh an expired access token using the long-lived refresh_token
    pub async fn refresh_access_token(&self, refresh_token: &str) -> anyhow::Result<TokenBundle> {
        info!(token_url=%self.config.token_url, "refreshing expired OAuth2 access token");
        let client = Self::token_client()?;

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

        let raw: zeroize::Zeroizing<Vec<u8>> =
            zeroize::Zeroizing::new(resp.bytes().await?.to_vec());
        let body: serde_json::Value = serde_json::from_slice(&raw)
            .map_err(|e| anyhow::anyhow!("malformed refresh response: {e}"))?;
        let access_token = body["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing access_token in refresh response"))?
            .to_string();

        let new_refresh = body["refresh_token"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| Some(refresh_token.to_string()));
        let expires_in = parse_expires_in(&body)?;

        Ok(TokenBundle {
            access_token,
            refresh_token: new_refresh,
            expires_in,
            obtained_at: chrono::Utc::now().timestamp(),
        })
    }
}

/// Read `expires_in` from a token response.
///
/// Several providers return it as a JSON *string* (`"expires_in": "3599"`), which
/// `as_u64()` silently reported as absent. The previous code then defaulted to a full hour
/// regardless of the server's actual lifetime. Fail loudly instead of guessing.
fn parse_expires_in(body: &serde_json::Value) -> anyhow::Result<u64> {
    match body.get("expires_in") {
        None => anyhow::bail!("token response is missing expires_in"),
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            .ok_or_else(|| anyhow::anyhow!("token response has non-numeric expires_in: {v}")),
    }
}

const SUCCESS_PAGE: &str = r#"<!DOCTYPE html>
<html>
<head><title>Vespetrel Authorization</title></head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; text-align: center; padding: 60px; background: #09090b; color: #fafafa;">
    <h1 style="font-size: 2rem; margin-bottom: 12px;">&#129418; Authorization Successful</h1>
    <p style="color: #a1a1aa; font-size: 1.1rem;">Your account has been connected to <strong>Vespetrel</strong>.</p>
    <p style="color: #71717a; font-size: 0.9rem; margin-top: 24px;">You may safely close this browser window.</p>
</body>
</html>"#;

const ERROR_PAGE: &str = r#"<!DOCTYPE html>
<html>
<head><title>Vespetrel Authorization</title></head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; text-align: center; padding: 60px; background: #09090b; color: #fafafa;">
    <h1 style="font-size: 2rem; margin-bottom: 12px;">Authorization Failed</h1>
    <p style="color: #a1a1aa; font-size: 1rem;">{REASON}</p>
    <p style="color: #71717a; font-size: 0.9rem; margin-top: 24px;">You may close this window and try again.</p>
</body>
</html>"#;

fn http_response(code: u16, reason: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
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

#[derive(Clone, serde::Serialize, serde::Deserialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct TokenBundle {
    pub access_token: String,
    pub refresh_token: Option<String>,
    #[zeroize(skip)]
    pub expires_in: u64,
    /// Absolute expiry as a UNIX timestamp, computed once at issue time.
    ///
    /// Without this every consumer had to re-derive absolute time from `expires_in` and
    /// apply its own skew policy, and there was no single `is_expired` to consult.
    #[serde(default = "default_obtained_at")]
    #[zeroize(skip)]
    pub obtained_at: i64,
}

fn default_obtained_at() -> i64 {
    chrono::Utc::now().timestamp()
}

impl TokenBundle {
    /// Has this token expired, allowing for clock skew?
    ///
    /// `skew_secs` should cover the round trip and the provider's own tolerance.
    pub fn is_expired(&self, skew_secs: i64) -> bool {
        chrono::Utc::now().timestamp() + skew_secs >= self.obtained_at + self.expires_in as i64
    }
}

impl std::fmt::Debug for TokenBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenBundle")
            .field("access_token", &"[REDACTED]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("expires_in", &self.expires_in)
            .field("obtained_at", &self.obtained_at)
            .finish()
    }
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
    fn google_requests_an_offline_grant() {
        // Regression: without `access_type=offline`, Google may answer with no
        // `refresh_token`, and the account silently expires after an hour.
        let cfg = OAuth2Config::google("cid");
        assert!(cfg.offline_access);
        let (url, _, _) = OAuth2Engine::new(cfg).auth_url();
        assert!(url.contains("access_type=offline"), "{url}");
        assert!(url.contains("prompt=consent"), "{url}");
        assert!(url.contains("include_granted_scopes=true"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
        // Spaces must be percent-encoded, not left raw or turned into `+`.
        assert!(
            url.contains("scope=https%3A%2F%2Fmail.google.com%2F%20openid"),
            "{url}"
        );
    }

    #[test]
    fn loopback_redirect_uses_a_bare_root_path() {
        let mut cfg = OAuth2Config::google("cid");
        cfg.redirect_uri = "http://127.0.0.1:5555/".into();
        assert_eq!(
            OAuth2Engine::new(cfg).redirect_port(),
            5555,
            "the listener must bind the same port the provider is told to redirect to"
        );
    }

    #[tokio::test]
    async fn loopback_listener_binds_one_port_on_both_families() {
        let listener = OAuth2Engine::bind_loopback().await.unwrap();
        assert!(listener.port > 0);
        assert!(
            listener.v6.is_some(),
            "IPv6 loopback should bind so a browser resolving localhost to ::1 can reach us"
        );
    }

    #[test]
    fn provider_denial_is_reported_by_name() {
        // Regression: `?error=access_denied` used to surface as "failed to extract
        // authorization code", indistinguishable from a real bug.
        let engine = OAuth2Engine::new(OAuth2Config::google("cid"));
        let req = "GET /?error=access_denied&error_description=User+denied+access HTTP/1.1\r\n\
                    Host: 127.0.0.1:8989\r\n\r\n";
        let err = engine.validate_callback(req, None).unwrap_err();
        assert_eq!(
            err,
            CallbackRejection::Denied("access_denied: User denied access".into())
        );
    }

    #[test]
    fn callback_requires_matching_state() {
        let engine = OAuth2Engine::new(OAuth2Config::google("cid"));
        let req = "GET /?code=abc&state=expected HTTP/1.1\r\nHost: 127.0.0.1:8989\r\n\r\n";
        assert!(engine.validate_callback(req, Some("expected")).is_ok());
        assert_eq!(
            engine
                .validate_callback(req, Some("different"))
                .unwrap_err(),
            CallbackRejection::Transient("CSRF state mismatch".into())
        );
    }

    #[test]
    fn callback_rejects_foreign_host_header() {
        // Guards against DNS rebinding: only loopback hosts may redeem a code.
        let engine = OAuth2Engine::new(OAuth2Config::google("cid"));
        let expected = CallbackRejection::Transient("invalid Host header".into());
        let evil = "GET /?code=abc&state=s HTTP/1.1\r\nHost: evil.example.com\r\n\r\n";
        assert_eq!(
            engine.validate_callback(evil, Some("s")).unwrap_err(),
            expected
        );
        let plain = "GET /?code=abc&state=s HTTP/1.1\r\n\r\n";
        assert_eq!(
            engine.validate_callback(plain, Some("s")).unwrap_err(),
            expected
        );
        // A denial is reported by name even when the Host header is fine.
        let denied = "GET /?error=access_denied&state=s HTTP/1.1\r\nHost: 127.0.0.1:8989\r\n\r\n";
        assert!(matches!(
            engine.validate_callback(denied, Some("s")).unwrap_err(),
            CallbackRejection::Denied(_)
        ));
    }

    #[test]
    fn expires_in_accepts_string_and_number_forms() {
        assert_eq!(
            parse_expires_in(&serde_json::json!({"expires_in": 3599})).unwrap(),
            3599
        );
        assert_eq!(
            parse_expires_in(&serde_json::json!({"expires_in": "3599"})).unwrap(),
            3599
        );
        assert!(parse_expires_in(&serde_json::json!({})).is_err());
        assert!(parse_expires_in(&serde_json::json!({"expires_in": "soon"})).is_err());
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
