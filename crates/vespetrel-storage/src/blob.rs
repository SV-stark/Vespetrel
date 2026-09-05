use bytes::Bytes;
use moka::sync::Cache;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Centralized safe blob path constructor that guards against traversal and symlink escapes
pub fn safe_blob_path(base: &Path, id: &str) -> std::io::Result<PathBuf> {
    safe_blob_path_with_ext(base, id, "lz4")
}

pub fn safe_blob_path_with_ext(
    base: &Path,
    id: &str,
    default_ext: &str,
) -> std::io::Result<PathBuf> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid blob ID: must be ASCII alphanumeric or hyphen/underscore",
        ));
    }
    std::fs::create_dir_all(base)?;
    let can_base = base.canonicalize()?;

    let shard = &id[..2.min(id.len())];
    let raw_shard_dir = can_base.join(shard);
    std::fs::create_dir_all(&raw_shard_dir)?;
    let can_shard_dir = raw_shard_dir.canonicalize()?;

    if !can_shard_dir.starts_with(&can_base) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Shard directory symlink escape detected",
        ));
    }

    let lz4_cand = can_shard_dir.join(format!("{id}.lz4"));
    if let Ok(can_cand) = lz4_cand.canonicalize() {
        if !can_cand.starts_with(&can_base) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Path traversal / symlink escape detected",
            ));
        }
        return Ok(can_cand);
    }
    let zst_cand = can_shard_dir.join(format!("{id}.zst"));
    if let Ok(can_cand) = zst_cand.canonicalize() {
        if !can_cand.starts_with(&can_base) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Path traversal / symlink escape detected",
            ));
        }
        return Ok(can_cand);
    }
    let target = can_shard_dir.join(format!("{id}.{default_ext}"));
    if !target.starts_with(&can_base) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Path traversal detected",
        ));
    }
    Ok(target)
}

/// Compressed blob store for raw RFC822 and attachments - § local storage layer
/// Uses lz4_flex for fast compression + moka bounded in-memory cache
pub struct BlobStore {
    base: PathBuf,
    cache: Cache<String, Bytes>,
}

impl BlobStore {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        // 50MB max total in-memory weight with 1 hour idle expiration
        let cache = Cache::builder()
            .max_capacity(50 * 1024 * 1024)
            .time_to_idle(Duration::from_secs(3600))
            .weigher(|_k, v: &Bytes| v.len() as u32)
            .build();
        Self {
            base: base.into(),
            cache,
        }
    }

    pub fn ensure_base(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.base)
    }

    pub fn write(&self, id: &str, data: &[u8]) -> std::io::Result<PathBuf> {
        self.ensure_base()?;
        let path = safe_blob_path(&self.base, id)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let compressed = lz4_flex::compress_prepend_size(data);

        // Atomic write via NamedTempFile in the same directory, sync to disk, then persist
        let parent = path.parent().unwrap_or(&self.base);
        let mut tmp = tempfile::Builder::new()
            .prefix("blob-")
            .suffix(".tmp")
            .tempfile_in(parent)?;
        use std::io::Write;
        tmp.write_all(&compressed)?;
        tmp.as_file().sync_all()?;
        tmp.persist(&path).map_err(|e| e.error)?;

        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }

        self.cache
            .insert(id.to_string(), Bytes::copy_from_slice(data));
        Ok(path)
    }

    pub fn blob_path(&self, id: &str) -> std::io::Result<PathBuf> {
        safe_blob_path(&self.base, id)
    }

    pub fn read(&self, id: &str) -> std::io::Result<Vec<u8>> {
        if let Some(cached) = self.cache.get(id) {
            return Ok(cached.to_vec());
        }
        let path = self.blob_path(id)?;
        let data = self.read_path(&path)?;
        self.cache
            .insert(id.to_string(), Bytes::copy_from_slice(&data));
        Ok(data)
    }

    pub fn read_bytes(&self, id: &str) -> std::io::Result<Bytes> {
        if let Some(cached) = self.cache.get(id) {
            return Ok(cached);
        }
        let raw = self.read(id)?;
        Ok(Bytes::from(raw))
    }

    pub fn read_path(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        let can_base = self.base.canonicalize()?;
        let can_path = path.canonicalize()?;
        if !can_path.starts_with(&can_base) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Blob path traversal escape detected",
            ));
        }
        let compressed = std::fs::read(&can_path)?;
        if path.extension().and_then(|e| e.to_str()) == Some("zst") {
            use std::io::Read;
            let mut decoder = zstd::Decoder::new(&compressed[..])
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
            let mut decompressed = Vec::new();
            // Max 100MB decompression limit guard
            decoder
                .by_ref()
                .take(100 * 1024 * 1024 + 1)
                .read_to_end(&mut decompressed)?;
            if decompressed.len() > 100 * 1024 * 1024 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Zstd blob uncompressed size exceeds 100MB limit",
                ));
            }
            return Ok(decompressed);
        }

        if compressed.len() >= 4 {
            let uncompressed_size =
                u32::from_le_bytes([compressed[0], compressed[1], compressed[2], compressed[3]]);
            // Max 100MB decompression limit guard
            if uncompressed_size > 100 * 1024 * 1024 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Blob uncompressed size exceeds 100MB limit",
                ));
            }
        }
        lz4_flex::decompress_size_prepended(&compressed)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }

    /// Read blob as UTF-8 string with SIMD validation
    pub fn read_utf8(&self, id: &str) -> std::io::Result<String> {
        let raw = self.read(id)?;
        // Vectorized SIMD UTF-8 validation (AVX2/NEON/SSE4)
        simdutf8::basic::from_utf8(&raw)
            .map(|s| s.to_string())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }

    pub fn delete(&self, id: &str) -> std::io::Result<()> {
        self.cache.invalidate(id);
        let path = self.blob_path(id)?;
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Write with zstd (higher compression, slower) - for archival
    pub fn write_zstd(&self, id: &str, data: &[u8]) -> std::io::Result<PathBuf> {
        self.ensure_base()?;
        let path = safe_blob_path_with_ext(&self.base, id, "zst")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let compressed = zstd::encode_all(data, 3).map_err(std::io::Error::other)?;
        let parent = path.parent().unwrap_or(&self.base);
        let mut tmp = tempfile::Builder::new()
            .prefix("blob-zst-")
            .suffix(".tmp")
            .tempfile_in(parent)?;
        use std::io::Write;
        tmp.write_all(&compressed)?;
        tmp.persist(&path).map_err(|e| e.error)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_roundtrip() {
        let dir = std::env::temp_dir().join(format!("vespetrel-test-{}", uuid::Uuid::new_v4()));
        let store = BlobStore::new(&dir);
        let data = b"Hello, this is a test RFC822 message with some content to compress";
        let id = "abcd1234";
        store.write(id, data).unwrap();
        let out = store.read(id).unwrap();
        assert_eq!(out, data);
        store.delete(id).unwrap();

        // Test zstd roundtrip
        let zstd_id = "zstd5678";
        store.write_zstd(zstd_id, data).unwrap();
        let zstd_out = store.read(zstd_id).unwrap();
        assert_eq!(zstd_out, data);
        store.delete(zstd_id).unwrap();

        let _ = std::fs::remove_dir_all(dir);
    }
}
