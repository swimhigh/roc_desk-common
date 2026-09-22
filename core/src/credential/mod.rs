pub mod keyring_store;

use async_trait::async_trait;

use crate::error::AppError;

/// Secret storage abstraction shared by tool crates (SSH passwords/private key
/// passphrases, AI API keys, database passwords, ...). Values never get
/// persisted as plaintext JSON; concrete implementations are expected to use
/// an OS-native secret store.
#[async_trait]
pub trait CredentialStore: Send + Sync {
    async fn set(&self, key: &str, secret: &str) -> Result<(), AppError>;
    async fn get(&self, key: &str) -> Result<Option<String>, AppError>;
    async fn delete(&self, key: &str) -> Result<(), AppError>;
}

pub use keyring_store::KeyringStore;
