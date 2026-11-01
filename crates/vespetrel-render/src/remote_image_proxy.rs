//! Private remote-image proxy.
//!
//! Fetches remote images on behalf of the renderer without forwarding cookies or a
//! `Referer` header, over HTTPS only, with a hard size cap and a local on-disk cache.
//! Mirrors the behaviour of Rail's image proxy so embedded trackers cannot correlate
//! requests with the user's session.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("only https:// image URLs are allowed")]
    NotHttps,
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("image exceeds size cap of {cap} bytes (got {got})")]
    TooLarge { cap: usize, got: usize },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("network error: {0}")]
    Network(String),
}

#[derive(Debug, Clone)]
pub struct RemoteImageProxy {
    pub cache_dir: PathBuf,
    pub max_bytes: usize,
    client: reqwest::Client,
}

impl RemoteImageProxy {
    pub fn new(cache_dir: impl Into<PathBuf>, max_bytes: usize) -> Self {
        // A plain client: no cookie store, no default headers, no referer policy.
        let client = reqwest::Client::builder()
            .referer(false)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            cache_dir: cache_dir.into(),
            max_bytes,
            client,
        }
    }

    fn cache_path(&self, url: &str) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        let hex = format!("{:x}", hasher.finalize());
        self.cache_dir.join(format!("{hex}.bin"))
    }

    pub async fn fetch(&self, url: &str) -> Result<Vec<u8>, ProxyError> {
        let parsed = Url::parse(url).map_err(|e| ProxyError::InvalidUrl(e.to_string()))?;
        if parsed.scheme() != "https" {
            return Err(ProxyError::NotHttps);
        }

        // Local cache hit.
        let path = self.cache_path(url);
        if let Ok(bytes) = std::fs::read(&path)
            && bytes.len() <= self.max_bytes
        {
            return Ok(bytes);
        }

        let resp = self
            .client
            .get(parsed.clone())
            .send()
            .await
            .map_err(|e| ProxyError::Network(e.to_string()))?;

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ProxyError::Network(e.to_string()))?;

        if bytes.len() > self.max_bytes {
            return Err(ProxyError::TooLarge {
                cap: self.max_bytes,
                got: bytes.len(),
            });
        }

        // Best-effort cache write; never fail the fetch on a cache error.
        let _ = std::fs::create_dir_all(&self.cache_dir);
        let _ = std::fs::write(&path, &bytes);
        Ok(bytes.to_vec())
    }

    /// Synchronous helper used by tests and callers that already have bytes.
    pub fn cache_contains(&self, url: &str) -> bool {
        self.cache_path(url).exists()
    }

    pub fn cache_dir_path(&self) -> &Path {
        &self.cache_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rejects_http() {
        let tmp = tempfile::tempdir().unwrap();
        let proxy = RemoteImageProxy::new(tmp.path(), 1024);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(proxy.fetch("http://example.com/img.png"));
        assert!(matches!(res, Err(ProxyError::NotHttps)));
    }

    #[test]
    fn rejects_invalid_url() {
        let tmp = tempfile::tempdir().unwrap();
        let proxy = RemoteImageProxy::new(tmp.path(), 1024);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(proxy.fetch("not a url"));
        assert!(matches!(res, Err(ProxyError::InvalidUrl(_))));
    }

    #[test]
    fn cache_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let proxy = RemoteImageProxy::new(tmp.path(), 1024);
        // Pre-populate the cache file manually and ensure it is returned.
        let url = "https://example.com/cached.png";
        let path = proxy.cache_path(url);
        std::fs::create_dir_all(tmp.path()).unwrap();
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"cached-bytes").unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(proxy.fetch(url)).unwrap();
        assert_eq!(res, b"cached-bytes");
    }

    #[test]
    fn size_cap_constant() {
        let tmp = tempfile::tempdir().unwrap();
        let proxy = RemoteImageProxy::new(tmp.path(), 4);
        // A 5-byte cache entry is ignored (treated as miss) because it exceeds cap.
        let url = "https://example.com/big.png";
        let path = proxy.cache_path(url);
        std::fs::write(&path, b"12345").unwrap();
        // Network will fail, but the cached entry should NOT be returned.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(proxy.fetch(url));
        assert!(res.is_err());
    }
}
