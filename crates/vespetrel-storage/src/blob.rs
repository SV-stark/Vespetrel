use bytes::Bytes;
use moka::sync::Cache;
use std::path::{Path, PathBuf};

/// Compressed blob store for raw RFC822 and attachments - § local storage layer
/// Uses lz4_flex for fast compression + moka bounded in-memory cache
pub struct BlobStore {
    base: PathBuf,
    cache: Cache<String, Bytes>,
}

impl BlobStore {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        // 50MB max total in-memory weight
        let cache = Cache::builder()
            .max_capacity(50 * 1024 * 1024)
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

    fn blob_path(&self, id: &str) -> std::io::Result<PathBuf> {
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid blob ID: must be alphanumeric or hyphen/underscore",
            ));
        }
        // Shard by first 2 chars to avoid huge directories
        let shard = &id[..2.min(id.len())];
        Ok(self.base.join(shard).join(format!("{id}.lz4")))
    }

    pub fn write(&self, id: &str, data: &[u8]) -> std::io::Result<PathBuf> {
        self.ensure_base()?;
        let path = self.blob_path(id)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let compressed = lz4_flex::compress_prepend_size(data);

        // Atomic write via unique temporary file
        let tmp_path = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_path, compressed)?;
        std::fs::rename(&tmp_path, &path)?;

        self.cache
            .insert(id.to_string(), Bytes::copy_from_slice(data));
        Ok(path)
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
        let compressed = std::fs::read(path)?;
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
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid blob ID: must be alphanumeric or hyphen/underscore",
            ));
        }
        let shard = &id[..2.min(id.len())];
        let path = self.base.join(shard).join(format!("{id}.zst"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let compressed = zstd::encode_all(data, 3).map_err(std::io::Error::other)?;
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, compressed)?;
        std::fs::rename(&tmp_path, &path)?;
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
        let _ = std::fs::remove_dir_all(dir);
    }
}
