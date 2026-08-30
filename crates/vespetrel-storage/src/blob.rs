use std::path::{Path, PathBuf};

/// Compressed blob store for raw RFC822 and attachments - § local storage layer
/// Uses lz4_flex for fast compression (zstd available for archival)
pub struct BlobStore {
    base: PathBuf,
}

impl BlobStore {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    pub fn ensure_base(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.base)
    }

    fn blob_path(&self, id: &str) -> PathBuf {
        // Shard by first 2 chars to avoid huge directories
        let shard = &id[..2.min(id.len())];
        self.base.join(shard).join(format!("{id}.lz4"))
    }

    pub fn write(&self, id: &str, data: &[u8]) -> std::io::Result<PathBuf> {
        self.ensure_base()?;
        let path = self.blob_path(id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let compressed = lz4_flex::compress_prepend_size(data);
        std::fs::write(&path, compressed)?;
        Ok(path)
    }

    pub fn read(&self, id: &str) -> std::io::Result<Vec<u8>> {
        let path = self.blob_path(id);
        self.read_path(&path)
    }

    pub fn read_path(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        let compressed = std::fs::read(path)?;
        lz4_flex::decompress_size_prepended(&compressed)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }

    pub fn delete(&self, id: &str) -> std::io::Result<()> {
        let path = self.blob_path(id);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Write with zstd (higher compression, slower) - for archival
    pub fn write_zstd(&self, id: &str, data: &[u8]) -> std::io::Result<PathBuf> {
        self.ensure_base()?;
        let shard = &id[..2.min(id.len())];
        let path = self.base.join(shard).join(format!("{id}.zst"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let compressed = zstd::encode_all(data, 3).map_err(std::io::Error::other)?;
        std::fs::write(&path, compressed)?;
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
