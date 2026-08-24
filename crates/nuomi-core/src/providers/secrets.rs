//! Secret storage: API keys live in the OS keyring, referenced by name.

use async_trait::async_trait;

use super::ProviderError;

/// Abstraction so tests can avoid touching the real OS keyring.
#[async_trait]
pub trait SecretStore: Send + Sync {
    async fn set(&self, reference: &str, secret: &str) -> Result<(), ProviderError>;
    async fn get(&self, reference: &str) -> Result<String, ProviderError>;
    async fn delete(&self, reference: &str) -> Result<(), ProviderError>;
}

/// OS keyring backed store. `reference` is used as the keyring account;
/// service name is fixed to `nuomi`.
pub struct OsKeyring;

const SERVICE: &str = "nuomi";

fn entry(reference: &str) -> Result<keyring::Entry, ProviderError> {
    keyring::Entry::new(SERVICE, reference).map_err(|e| ProviderError::Keyring(e.to_string()))
}

#[async_trait]
impl SecretStore for OsKeyring {
    async fn set(&self, reference: &str, secret: &str) -> Result<(), ProviderError> {
        let entry = entry(reference)?;
        let secret = secret.to_string();
        tokio::task::spawn_blocking(move || entry.set_password(&secret))
            .await
            .map_err(|e| ProviderError::Keyring(e.to_string()))?
            .map_err(|e| ProviderError::Keyring(e.to_string()))
    }

    async fn get(&self, reference: &str) -> Result<String, ProviderError> {
        let entry = entry(reference)?;
        tokio::task::spawn_blocking(move || entry.get_password())
            .await
            .map_err(|e| ProviderError::Keyring(e.to_string()))?
            .map_err(|e| ProviderError::Keyring(e.to_string()))
    }

    async fn delete(&self, reference: &str) -> Result<(), ProviderError> {
        let entry = entry(reference)?;
        tokio::task::spawn_blocking(move || entry.delete_credential())
            .await
            .map_err(|e| ProviderError::Keyring(e.to_string()))?
            .map_err(|e| ProviderError::Keyring(e.to_string()))
    }
}

/// In-memory store for tests and demo mode (never persisted).
#[derive(Default)]
pub struct MemorySecretStore {
    map: tokio::sync::RwLock<std::collections::HashMap<String, String>>,
}

#[async_trait]
impl SecretStore for MemorySecretStore {
    async fn set(&self, reference: &str, secret: &str) -> Result<(), ProviderError> {
        self.map
            .write()
            .await
            .insert(reference.to_string(), secret.to_string());
        Ok(())
    }

    async fn get(&self, reference: &str) -> Result<String, ProviderError> {
        self.map
            .read()
            .await
            .get(reference)
            .cloned()
            .ok_or_else(|| ProviderError::Keyring(format!("no secret for '{reference}'")))
    }

    async fn delete(&self, reference: &str) -> Result<(), ProviderError> {
        self.map.write().await.remove(reference);
        Ok(())
    }
}
