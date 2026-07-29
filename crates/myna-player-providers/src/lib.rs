use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use myna_player_core::{CredentialStatus, ProviderDescriptor};
use thiserror::Error;

const KEYRING_SERVICE: &str = "com.mynaplayer.desktop";

#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),
    #[error("credential is empty")]
    Empty,
    #[error("credential store failed: {0}")]
    Backend(String),
    #[error("credential lock was poisoned")]
    Poisoned,
}

pub trait CredentialStore: Send + Sync {
    fn status(&self, provider_id: &str) -> Result<CredentialStatus, CredentialError>;
    fn get(&self, provider_id: &str) -> Result<Option<String>, CredentialError>;
    fn set(&self, provider_id: &str, secret: &str) -> Result<(), CredentialError>;
    fn delete(&self, provider_id: &str) -> Result<(), CredentialError>;
}

#[derive(Debug, Default)]
pub struct KeyringCredentialStore;

impl KeyringCredentialStore {
    fn entry(provider_id: &str) -> Result<keyring::Entry, CredentialError> {
        validate_provider_id(provider_id)?;
        keyring::Entry::new(KEYRING_SERVICE, &format!("translation/{provider_id}"))
            .map_err(|error| CredentialError::Backend(error.to_string()))
    }
}

impl CredentialStore for KeyringCredentialStore {
    fn status(&self, provider_id: &str) -> Result<CredentialStatus, CredentialError> {
        Ok(CredentialStatus {
            provider_id: provider_id.to_owned(),
            configured: self.get(provider_id)?.is_some(),
        })
    }

    fn get(&self, provider_id: &str) -> Result<Option<String>, CredentialError> {
        match Self::entry(provider_id)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(CredentialError::Backend(error.to_string())),
        }
    }

    fn set(&self, provider_id: &str, secret: &str) -> Result<(), CredentialError> {
        if secret.trim().is_empty() {
            return Err(CredentialError::Empty);
        }
        Self::entry(provider_id)?
            .set_password(secret.trim())
            .map_err(|error| CredentialError::Backend(error.to_string()))
    }

    fn delete(&self, provider_id: &str) -> Result<(), CredentialError> {
        match Self::entry(provider_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(CredentialError::Backend(error.to_string())),
        }
    }
}

#[derive(Debug, Default)]
pub struct MemoryCredentialStore {
    values: Mutex<HashMap<String, String>>,
}

impl CredentialStore for MemoryCredentialStore {
    fn status(&self, provider_id: &str) -> Result<CredentialStatus, CredentialError> {
        validate_provider_id(provider_id)?;
        let values = self.values.lock().map_err(|_| CredentialError::Poisoned)?;
        Ok(CredentialStatus {
            provider_id: provider_id.to_owned(),
            configured: values.contains_key(provider_id),
        })
    }

    fn get(&self, provider_id: &str) -> Result<Option<String>, CredentialError> {
        validate_provider_id(provider_id)?;
        let values = self.values.lock().map_err(|_| CredentialError::Poisoned)?;
        Ok(values.get(provider_id).cloned())
    }

    fn set(&self, provider_id: &str, secret: &str) -> Result<(), CredentialError> {
        validate_provider_id(provider_id)?;
        if secret.trim().is_empty() {
            return Err(CredentialError::Empty);
        }
        let mut values = self.values.lock().map_err(|_| CredentialError::Poisoned)?;
        values.insert(provider_id.to_owned(), secret.trim().to_owned());
        Ok(())
    }

    fn delete(&self, provider_id: &str) -> Result<(), CredentialError> {
        validate_provider_id(provider_id)?;
        let mut values = self.values.lock().map_err(|_| CredentialError::Poisoned)?;
        values.remove(provider_id);
        Ok(())
    }
}

#[derive(Clone)]
pub struct ProviderRegistry {
    providers: Arc<Vec<ProviderDescriptor>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self {
            providers: Arc::new(vec![
                ProviderDescriptor {
                    id: "none".into(),
                    display_name: "Transcript only".into(),
                    cloud: false,
                    requires_credential: false,
                    supported_endpoints: Vec::new(),
                    available: true,
                    unavailable_reason: None,
                },
                ProviderDescriptor {
                    id: "deepl".into(),
                    display_name: "DeepL".into(),
                    cloud: true,
                    requires_credential: true,
                    supported_endpoints: vec!["free".into(), "pro".into()],
                    available: true,
                    unavailable_reason: None,
                },
                cloud_provider("openai", "OpenAI", "gpt-5-mini"),
                cloud_provider("gemini", "Google Gemini", "gemini-3.6-flash"),
                cloud_provider("openrouter", "OpenRouter", "openai/gpt-4.1-mini"),
                cloud_provider("minimax", "MiniMax", "MiniMax-M2.7"),
            ]),
        }
    }
}

impl ProviderRegistry {
    pub fn list(&self) -> Vec<ProviderDescriptor> {
        self.providers.as_ref().clone()
    }

    pub fn get(&self, provider_id: &str) -> Option<ProviderDescriptor> {
        self.providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .cloned()
    }
}

fn cloud_provider(id: &str, display_name: &str, default_model: &str) -> ProviderDescriptor {
    ProviderDescriptor {
        id: id.into(),
        display_name: display_name.into(),
        cloud: true,
        requires_credential: true,
        supported_endpoints: vec![default_model.into()],
        available: true,
        unavailable_reason: None,
    }
}

fn validate_provider_id(provider_id: &str) -> Result<(), CredentialError> {
    match provider_id {
        "deepl" | "openai" | "gemini" | "openrouter" | "minimax" => Ok(()),
        _ => Err(CredentialError::UnsupportedProvider(provider_id.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_exposes_cloud_providers_as_available() {
        let registry = ProviderRegistry::default();
        let openai = registry.get("openai").unwrap();
        assert!(openai.available);
        assert_eq!(openai.supported_endpoints, vec!["gpt-5-mini"]);
    }

    #[test]
    fn memory_store_never_exposes_secret_through_status() {
        let store = MemoryCredentialStore::default();
        store.set("deepl", "secret").unwrap();

        assert!(store.status("deepl").unwrap().configured);
        assert_eq!(store.get("deepl").unwrap().as_deref(), Some("secret"));
        store.delete("deepl").unwrap();
        assert!(!store.status("deepl").unwrap().configured);
    }
}
