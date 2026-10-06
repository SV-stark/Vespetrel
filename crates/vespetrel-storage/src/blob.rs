use bytes::Bytes;
use moka::sync::Cache;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Maximum accepted size of a decompressed blob (decompression-bomb guard).
const MAX_BLOB_UNCOMPRESSED: u64 = 100 * 1024 * 1024;

/// Centralized safe blob path constructor that guards against traversal and symlink escapes
pub fn safe_blob_path(base: &Path, id: &str) -> std::io::Result<PathBuf> {
    // A blob may have been written in either format, so a lookup probes both.
    match lookup_blob_path(base, id) {
        Some(found) => Ok(found),
        None => blob_path_for_ext(base, id, "lz4"),
    }
}

/// Resolve the path for a *specific* on-disk format, without ever falling back to another
/// extension. Writers must use this: returning an existing sibling in the other format would
/// make `write` (lz4) and `write_zstd` (zstd) overwrite each other's files in place, producing
/// a blob that no reader can decode.
fn blob_path_for_ext(base: &Path, id: &str, ext: &str) -> std::io::Result<PathBuf> {
    validate_blob_id(id)?;
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

    let target = can_shard_dir.join(format!("{id}.{ext}"));
    if !target.starts_with(&can_base) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Path traversal detected",
        ));
    }
    Ok(target)
}

/// Find an already-existing blob file, in either supported on-disk format.
fn lookup_blob_path(base: &Path, id: &str) -> Option<PathBuf> {
    let can_base = base.canonicalize().ok()?;
    let can_shard_dir = can_base.join(&id[..2.min(id.len())]).canonicalize().ok()?;
    if !can_shard_dir.starts_with(&can_base) {
        return None;
    }
    for ext in ["lz4", "zst"] {
        let cand = can_shard_dir.join(format!("{id}.{ext}"));
        if let Ok(can_cand) = cand.canonicalize()
            && can_cand.starts_with(&can_base)
        {
            return Some(can_cand);
        }
    }
    None
}

fn validate_blob_id(id: &str) -> std::io::Result<()> {
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
    Ok(())
}

pub fn safe_blob_path_with_ext(
    base: &Path,
    id: &str,
    default_ext: &str,
) -> std::io::Result<PathBuf> {
    // Only honour an existing sibling in the *same* format; otherwise fall through to the
    // requested extension. Returning the other format here is what silently corrupted blobs.
    validate_blob_id(id)?;
    if let Some(found) = lookup_blob_path(base, id)
        && found.extension().and_then(|e| e.to_str()) == Some(default_ext)
    {
        return Ok(found);
    }
    blob_path_for_ext(base, id, default_ext)
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
        let path = blob_path_for_ext(&self.base, id, "lz4")?;
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

        // Drop any stale sibling in the other format so a later lookup can never pick a
        // file that no longer matches the id's current encoding.
        if let Ok(stale) = blob_path_for_ext(&self.base, id, "zst") {
            let _ = std::fs::remove_file(stale);
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
        let max = MAX_BLOB_UNCOMPRESSED;
        if path.extension().and_then(|e| e.to_str()) == Some("zst") {
            use std::io::Read;
            let mut decoder = zstd::Decoder::new(&compressed[..])
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
            let mut decompressed = Vec::new();
            // Read one byte past the limit so an over-sized stream is detectable.
            decoder
                .by_ref()
                .take(max + 1)
                .read_to_end(&mut decompressed)?;
            if decompressed.len() as u64 > max {
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
            if u64::from(uncompressed_size) > max {
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
        // Remove both encodings: a single-format delete can orphan a sibling written in the
        // other format, which then keeps shadowing this id forever.
        self.delete_all(id)
    }

    /// Write with zstd (higher compression, slower) - for archival
    pub fn write_zstd(&self, id: &str, data: &[u8]) -> std::io::Result<PathBuf> {
        self.ensure_base()?;
        let path = blob_path_for_ext(&self.base, id, "zst")?;
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
        // rename is atomic w.r.t. ordering, not durability: without this fsync a crash can
        // leave the final name pointing at a file whose blocks were never written.
        tmp.as_file().sync_all()?;
        tmp.persist(&path).map_err(|e| e.error)?;
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }

        if let Ok(stale) = blob_path_for_ext(&self.base, id, "lz4") {
            let _ = std::fs::remove_file(stale);
        }

        // Without this the moka entry from any earlier `read`/`write` of the same id is
        // served forever, so a re-write of a blob id is invisible for the life of the process.
        self.cache
            .insert(id.to_string(), Bytes::copy_from_slice(data));
        Ok(path)
    }

    /// Remove every on-disk representation of a blob id (both encodings).
    pub fn delete_all(&self, id: &str) -> std::io::Result<()> {
        self.cache.invalidate(id);
        for ext in ["lz4", "zst"] {
            if let Ok(path) = blob_path_for_ext(&self.base, id, ext)
                && path.exists()
            {
                std::fs::remove_file(path)?;
            }
        }
        Ok(())
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

    /// Regression: `safe_blob_path_with_ext` probed `.lz4` before honouring the requested
    /// extension, so `write` (lz4) and `write_zstd` (zstd) for the same id each adopted the
    /// *other* format's file and overwrote it in place. The blob became permanently
    /// undecodable while both writes returned Ok.
    #[test]
    fn blob_lz4_and_zstd_do_not_collide_on_the_same_id() {
        let dir = std::env::temp_dir().join(format!("vespetrel-test-{}", uuid::Uuid::new_v4()));
        let store = BlobStore::new(&dir);
        let id = "collide0001";

        store.write(id, b"VERSION-ONE lz4 payload").unwrap();
        store.write_zstd(id, b"VERSION-TWO zstd payload").unwrap();

        assert_eq!(store.read(id).unwrap(), b"VERSION-TWO zstd payload");

        // And the reverse order.
        let id2 = "collide0002";
        store.write_zstd(id2, b"V2 zstd").unwrap();
        store.write(id2, b"V1 lz4").unwrap();
        assert_eq!(store.read(id2).unwrap(), b"V1 lz4");

        let _ = std::fs::remove_dir_all(dir);
    }

    /// Regression: `write_zstd` never touched the moka cache, so a re-write of an id that was
    /// already cached served stale bytes for the rest of the process lifetime.
    #[test]
    fn blob_rewrites_invalidate_the_cache() {
        let dir = std::env::temp_dir().join(format!("vespetrel-test-{}", uuid::Uuid::new_v4()));
        let store = BlobStore::new(&dir);
        let id = "stale000001";

        store.write(id, b"first").unwrap();
        assert_eq!(store.read(id).unwrap(), b"first");

        store.write_zstd(id, b"second").unwrap();
        assert_eq!(store.read(id).unwrap(), b"second");

        store.write(id, b"third").unwrap();
        assert_eq!(store.read(id).unwrap(), b"third");

        let _ = std::fs::remove_dir_all(dir);
    }

    /// Regression: `delete` only removed the file the lookup happened to resolve, so a sibling
    /// in the other encoding survived and kept shadowing the id.
    #[test]
    fn blob_delete_removes_every_encoding() {
        let dir = std::env::temp_dir().join(format!("vespetrel-test-{}", uuid::Uuid::new_v4()));
        let store = BlobStore::new(&dir);
        let id = "delme00001";

        store.write(id, b"x").unwrap();
        store.write_zstd(id, b"y").unwrap();
        store.delete(id).unwrap();
        assert!(store.read(id).is_err(), "blob still readable after delete");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn blob_path_rejects_traversal_and_hostile_ids() {
        let dir = std::env::temp_dir().join(format!("vespetrel-test-{}", uuid::Uuid::new_v4()));
        let store = BlobStore::new(&dir);
        for bad in ["", "../escape", "a/b", "a\\b", &"x".repeat(129)] {
            assert!(
                store.blob_path(bad).is_err(),
                "hostile blob id {bad:?} was accepted"
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
