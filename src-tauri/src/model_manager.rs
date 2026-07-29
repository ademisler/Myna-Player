use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use myna_player_core::ModelDescriptor;
use sha2::{Digest, Sha256};

const WHISPER_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin";
const WHISPER_BASE_SHA256: &str =
    "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe";
const WHISPER_BASE_SIZE: u64 = 147_951_465;
const VAD_URL: &str =
    "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin";
const VAD_SHA256: &str = "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987";
const VAD_SIZE: u64 = 885_098;

#[derive(Debug, Clone)]
struct CachedHash {
    size_bytes: u64,
    modified_ns: u128,
    sha256: String,
}

#[derive(Debug, Clone, Copy, Default)]
struct DownloadProgress {
    downloaded_bytes: u64,
}

#[derive(Clone)]
pub struct ModelManager {
    root: PathBuf,
    client: reqwest::Client,
    installing: Arc<Mutex<HashSet<String>>>,
    progress: Arc<Mutex<HashMap<String, DownloadProgress>>>,
    verification_cache: Arc<Mutex<HashMap<PathBuf, CachedHash>>>,
}

struct CatalogModel {
    id: &'static str,
    display_name: &'static str,
    description: &'static str,
    file_name: &'static str,
    url: &'static str,
    sha256: &'static str,
    size_bytes: u64,
}

const CATALOG: [CatalogModel; 2] = [
    CatalogModel {
        id: "whisper-base",
        display_name: "Whisper base",
        description: "Multilingual speech recognition model (about 141 MB).",
        file_name: "ggml-base.bin",
        url: WHISPER_BASE_URL,
        sha256: WHISPER_BASE_SHA256,
        size_bytes: WHISPER_BASE_SIZE,
    },
    CatalogModel {
        id: "silero-vad",
        display_name: "Silero VAD 6.2",
        description: "Detects speech and skips silence before Whisper inference.",
        file_name: "ggml-silero-v6.2.0.bin",
        url: VAD_URL,
        sha256: VAD_SHA256,
        size_bytes: VAD_SIZE,
    },
];

impl ModelManager {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(20 * 60))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            root,
            client,
            installing: Arc::new(Mutex::new(HashSet::new())),
            progress: Arc::new(Mutex::new(HashMap::new())),
            verification_cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn list(&self) -> Vec<ModelDescriptor> {
        CATALOG.iter().map(|model| self.describe(model)).collect()
    }

    pub fn verify(&self, id: &str) -> Result<ModelDescriptor, String> {
        let model = catalog_model(id)?;
        let path = self.root.join(model.file_name);
        if !path.is_file() {
            return Ok(self.describe(model));
        }
        let actual = self.sha256_cached(&path)?;
        if actual != model.sha256 {
            return Err(format!(
                "checksum mismatch for {}: expected {}, got {}",
                model.file_name, model.sha256, actual
            ));
        }
        Ok(self.describe(model))
    }

    pub async fn install(&self, id: &str) -> Result<ModelDescriptor, String> {
        let model = catalog_model(id)?;
        let final_path = self.root.join(model.file_name);
        if final_path.is_file() && self.sha256_cached(&final_path)? == model.sha256 {
            return Ok(self.describe(model));
        }
        self.begin_install(model.id)?;
        self.set_progress(model.id, 0);
        let result = self.install_inner(model).await;
        self.finish_install(model.id);
        result.map(|()| self.describe(model))
    }

    async fn install_inner(&self, model: &CatalogModel) -> Result<(), String> {
        let final_path = self.root.join(model.file_name);
        let partial_path = final_path.with_extension("bin.part");
        let response = self
            .client
            .get(model.url)
            .send()
            .await
            .map_err(|error| format!("model download failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("model download returned {}", response.status()));
        }
        let declared_size = response.content_length();
        if declared_size.is_some_and(|size| size != model.size_bytes) {
            return Err(format!(
                "unexpected model size: expected {}, server announced {}",
                model.size_bytes,
                declared_size.unwrap_or_default()
            ));
        }
        let mut file = tokio::fs::File::create(&partial_path)
            .await
            .map_err(|error| error.to_string())?;
        let mut response = response;
        let mut written = 0_u64;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| format!("model download interrupted: {error}"))?
        {
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                .await
                .map_err(|error| error.to_string())?;
            written = written.saturating_add(chunk.len() as u64);
            self.set_progress(model.id, written);
            if written > model.size_bytes.saturating_add(1_024) {
                let _ = tokio::fs::remove_file(&partial_path).await;
                return Err("download exceeded the expected model size".into());
            }
        }
        tokio::io::AsyncWriteExt::flush(&mut file)
            .await
            .map_err(|error| error.to_string())?;
        drop(file);
        if written != model.size_bytes {
            let _ = tokio::fs::remove_file(&partial_path).await;
            return Err(format!(
                "incomplete model download: expected {} bytes, received {}",
                model.size_bytes, written
            ));
        }
        let actual = sha256_file(&partial_path)?;
        if actual != model.sha256 {
            let _ = fs::remove_file(&partial_path);
            return Err(format!(
                "downloaded model failed SHA-256 verification: expected {}, got {}",
                model.sha256, actual
            ));
        }
        if final_path.exists() {
            fs::remove_file(&final_path).map_err(|error| error.to_string())?;
        }
        fs::rename(&partial_path, &final_path).map_err(|error| error.to_string())?;
        if let Ok(mut cache) = self.verification_cache.lock() {
            cache.remove(&partial_path);
            cache.remove(&final_path);
        }
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<ModelDescriptor, String> {
        let model = catalog_model(id)?;
        if self.is_installing(id) {
            return Err(format!("{} is currently downloading", model.display_name));
        }
        let path = self.root.join(model.file_name);
        if path.exists() {
            fs::remove_file(&path).map_err(|error| error.to_string())?;
        }
        if let Ok(mut cache) = self.verification_cache.lock() {
            cache.remove(&path);
        }
        Ok(self.describe(model))
    }

    fn sha256_cached(&self, path: &Path) -> Result<String, String> {
        let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        if let Ok(cache) = self.verification_cache.lock()
            && let Some(entry) = cache.get(path)
            && entry.size_bytes == metadata.len()
            && entry.modified_ns == modified_ns
        {
            return Ok(entry.sha256.clone());
        }
        let sha256 = sha256_file(path)?;
        if let Ok(mut cache) = self.verification_cache.lock() {
            cache.insert(
                path.to_path_buf(),
                CachedHash {
                    size_bytes: metadata.len(),
                    modified_ns,
                    sha256: sha256.clone(),
                },
            );
        }
        Ok(sha256)
    }

    fn begin_install(&self, id: &str) -> Result<(), String> {
        let mut installing = self
            .installing
            .lock()
            .map_err(|_| "model installation lock was poisoned".to_string())?;
        if !installing.insert(id.to_owned()) {
            return Err(format!("model {id} is already downloading"));
        }
        Ok(())
    }

    fn finish_install(&self, id: &str) {
        if let Ok(mut installing) = self.installing.lock() {
            installing.remove(id);
        }
        if let Ok(mut progress) = self.progress.lock() {
            progress.remove(id);
        }
    }

    fn set_progress(&self, id: &str, downloaded_bytes: u64) {
        if let Ok(mut progress) = self.progress.lock() {
            progress.insert(id.to_owned(), DownloadProgress { downloaded_bytes });
        }
    }

    fn is_installing(&self, id: &str) -> bool {
        self.installing
            .lock()
            .is_ok_and(|installing| installing.contains(id))
    }

    fn describe(&self, model: &CatalogModel) -> ModelDescriptor {
        let path = self.root.join(model.file_name);
        let installed = path.is_file();
        let verified = installed
            && self
                .sha256_cached(&path)
                .is_ok_and(|actual| actual == model.sha256);
        let installing = self.is_installing(model.id);
        let downloaded_bytes = self
            .progress
            .lock()
            .ok()
            .and_then(|progress| progress.get(model.id).copied())
            .map(|progress| progress.downloaded_bytes)
            .unwrap_or(0);
        ModelDescriptor {
            id: model.id.into(),
            display_name: model.display_name.into(),
            description: model.description.into(),
            file_name: model.file_name.into(),
            size_bytes: model.size_bytes,
            sha256: model.sha256.into(),
            installed,
            verified,
            installing,
            downloaded_bytes,
            path: installed.then(|| path.to_string_lossy().into_owned()),
        }
    }
}

fn catalog_model(id: &str) -> Result<&'static CatalogModel, String> {
    CATALOG
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| format!("unknown model: {id}"))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_unique_ids_and_pinned_checksums() {
        let mut ids = std::collections::HashSet::new();
        for model in CATALOG {
            assert!(ids.insert(model.id));
            assert_eq!(model.sha256.len(), 64);
            assert!(model.size_bytes > 0);
        }
    }

    #[test]
    fn cached_hash_is_invalidated_when_file_metadata_changes() {
        let directory = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(directory.path().join("models")).unwrap();
        let path = directory.path().join("model.bin");
        fs::write(&path, b"first").unwrap();
        let first = manager.sha256_cached(&path).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        fs::write(&path, b"second-version").unwrap();
        let second = manager.sha256_cached(&path).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn concurrent_install_of_the_same_model_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(directory.path().join("models")).unwrap();
        manager.begin_install("whisper-base").unwrap();
        assert!(manager.begin_install("whisper-base").is_err());
        let descriptor = manager
            .list()
            .into_iter()
            .find(|model| model.id == "whisper-base")
            .unwrap();
        assert!(descriptor.installing);
        manager.finish_install("whisper-base");
        assert!(
            !manager
                .list()
                .into_iter()
                .find(|model| model.id == "whisper-base")
                .unwrap()
                .installing
        );
    }
}
