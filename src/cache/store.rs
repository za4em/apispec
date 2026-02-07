use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use sha2::{Digest, Sha256};

use crate::cache::metadata::CacheMetadata;
use crate::error::AppError;

pub const CACHE_DIR_ENV: &str = "APISPEC_CACHE_DIR";

const SPEC_FILE_NAME: &str = "spec.raw";
const METADATA_FILE_NAME: &str = "metadata.json";
const TEMP_FILE_SUFFIX: &str = ".tmp";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedSpec {
    pub bytes: Vec<u8>,
    pub metadata: Option<CacheMetadata>,
}

#[derive(Debug, Clone)]
pub struct CacheStore {
    root_dir: PathBuf,
}

impl CacheStore {
    pub fn new() -> Result<Self, AppError> {
        let root_dir = cache_root_dir()?;
        Ok(Self { root_dir })
    }

    #[cfg(test)]
    pub fn with_root(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }

    pub fn read(&self, canonical_source: &str) -> Result<Option<CachedSpec>, AppError> {
        let entry_dir = self.entry_dir(canonical_source);
        let spec_path = entry_dir.join(SPEC_FILE_NAME);

        if !spec_path.exists() {
            return Ok(None);
        }

        let bytes = fs::read(&spec_path).map_err(|source| AppError::CacheIo {
            path: spec_path.clone(),
            source,
        })?;

        let metadata_path = entry_dir.join(METADATA_FILE_NAME);
        let metadata = if metadata_path.exists() {
            let metadata_raw = fs::read(&metadata_path).map_err(|source| AppError::CacheIo {
                path: metadata_path.clone(),
                source,
            })?;
            serde_json::from_slice::<CacheMetadata>(&metadata_raw).ok()
        } else {
            None
        };

        Ok(Some(CachedSpec { bytes, metadata }))
    }

    pub fn write(
        &self,
        canonical_source: &str,
        bytes: &[u8],
        metadata: &CacheMetadata,
    ) -> Result<(), AppError> {
        let entry_dir = self.entry_dir(canonical_source);
        fs::create_dir_all(&entry_dir).map_err(|source| AppError::CacheIo {
            path: entry_dir.clone(),
            source,
        })?;

        let spec_path = entry_dir.join(SPEC_FILE_NAME);
        atomic_write(&spec_path, bytes)?;

        let metadata_path = entry_dir.join(METADATA_FILE_NAME);
        let metadata_bytes = serde_json::to_vec_pretty(metadata)
            .map_err(|source| AppError::CacheMetadataSerialize { source })?;
        atomic_write(&metadata_path, &metadata_bytes)?;

        Ok(())
    }

    fn entry_dir(&self, canonical_source: &str) -> PathBuf {
        let key = cache_key(canonical_source);
        self.root_dir.join(key)
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("cache-entry");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let tmp_path = path.with_file_name(format!(
        "{file_name}.{}.{}{}",
        std::process::id(),
        nonce,
        TEMP_FILE_SUFFIX
    ));

    fs::write(&tmp_path, bytes).map_err(|source| AppError::CacheIo {
        path: tmp_path.clone(),
        source,
    })?;

    fs::rename(&tmp_path, path).map_err(|source| AppError::CacheIo {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

pub fn cache_key(canonical_source: &str) -> String {
    format!("{:x}", Sha256::digest(canonical_source.as_bytes()))
}

fn cache_root_dir() -> Result<PathBuf, AppError> {
    if let Ok(override_dir) = std::env::var(CACHE_DIR_ENV) {
        return Ok(PathBuf::from(override_dir));
    }

    let dirs =
        ProjectDirs::from("dev", "apispec", "apispec").ok_or(AppError::CacheDirUnavailable)?;
    Ok(dirs.cache_dir().join("specs"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::metadata::CacheMetadata;
    use tempfile::TempDir;

    #[test]
    fn read_missing_entry_returns_none() {
        let temp = TempDir::new().unwrap();
        let store = CacheStore::with_root(temp.path().to_path_buf());

        let result = store.read("missing").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn write_and_read_round_trip() {
        let temp = TempDir::new().unwrap();
        let store = CacheStore::with_root(temp.path().to_path_buf());
        let bytes = br#"{"openapi":"3.1.0"}"#;
        let metadata = CacheMetadata::new("source", None, None, None, "3.1.0", bytes);

        store.write("source", bytes, &metadata).unwrap();
        let cached = store.read("source").unwrap().unwrap();

        assert_eq!(cached.bytes, bytes);
        assert!(cached.metadata.is_some());
        assert_eq!(cached.metadata.unwrap().openapi_version, "3.1.0");
    }

    #[test]
    fn tolerates_corrupt_metadata_file() {
        let temp = TempDir::new().unwrap();
        let store = CacheStore::with_root(temp.path().to_path_buf());
        let bytes = br#"{"openapi":"3.1.0"}"#;
        let metadata = CacheMetadata::new("source", None, None, None, "3.1.0", bytes);
        store.write("source", bytes, &metadata).unwrap();

        let entry_dir = store.entry_dir("source");
        std::fs::write(entry_dir.join(METADATA_FILE_NAME), b"not-json").unwrap();

        let cached = store.read("source").unwrap().unwrap();
        assert_eq!(cached.bytes, bytes);
        assert!(cached.metadata.is_none());
    }
}
