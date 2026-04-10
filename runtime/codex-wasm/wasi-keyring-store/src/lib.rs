//! Credential store shim for wasip2.
//!
//! Uses a pluggable backend pattern: the host crate (codex-wasm-app-server or
//! codex-wasm-tui) registers a `CredentialBackend` that calls WIT imports.
//! Falls back to an in-memory store when no backend is registered.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fmt::Debug;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

#[derive(Debug)]
pub enum CredentialStoreError {
    Other(String),
}

impl CredentialStoreError {
    pub fn message(&self) -> String {
        match self {
            Self::Other(s) => s.clone(),
        }
    }

    pub fn into_error(self) -> Box<dyn Error> {
        Box::new(self)
    }
}

impl fmt::Display for CredentialStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

impl Error for CredentialStoreError {}

/// Pluggable credential backend — implemented by the host crate to call WIT imports.
pub trait CredentialBackend: Send + Sync {
    fn load(&self, service: &str, account: &str) -> Result<Option<String>, String>;
    fn save(&self, service: &str, account: &str, value: &str) -> Result<(), String>;
    fn delete(&self, service: &str, account: &str) -> Result<bool, String>;
}

static BACKEND: OnceLock<Box<dyn CredentialBackend>> = OnceLock::new();

/// Register a credential backend. Should be called once during initialization.
pub fn set_credential_backend(backend: Box<dyn CredentialBackend>) {
    let _ = BACKEND.set(backend);
}

/// Shared credential store abstraction.
pub trait KeyringStore: Debug + Send + Sync {
    fn load(&self, service: &str, account: &str) -> Result<Option<String>, CredentialStoreError>;
    fn save(&self, service: &str, account: &str, value: &str) -> Result<(), CredentialStoreError>;
    fn delete(&self, service: &str, account: &str) -> Result<bool, CredentialStoreError>;
}

/// Default keyring store — delegates to the registered `CredentialBackend`,
/// falling back to an in-memory store if no backend is registered.
#[derive(Debug)]
pub struct DefaultKeyringStore;

/// In-memory fallback store (used when no backend is registered).
static FALLBACK_STORE: std::sync::LazyLock<Mutex<HashMap<String, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

impl KeyringStore for DefaultKeyringStore {
    fn load(&self, service: &str, account: &str) -> Result<Option<String>, CredentialStoreError> {
        if let Some(backend) = BACKEND.get() {
            backend
                .load(service, account)
                .map_err(CredentialStoreError::Other)
        } else {
            let guard = FALLBACK_STORE
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            Ok(guard.get(&format!("{service}:{account}")).cloned())
        }
    }

    fn save(&self, service: &str, account: &str, value: &str) -> Result<(), CredentialStoreError> {
        if let Some(backend) = BACKEND.get() {
            backend
                .save(service, account, value)
                .map_err(CredentialStoreError::Other)
        } else {
            let mut guard = FALLBACK_STORE
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            guard.insert(format!("{service}:{account}"), value.to_string());
            Ok(())
        }
    }

    fn delete(&self, service: &str, account: &str) -> Result<bool, CredentialStoreError> {
        if let Some(backend) = BACKEND.get() {
            backend
                .delete(service, account)
                .map_err(CredentialStoreError::Other)
        } else {
            let mut guard = FALLBACK_STORE
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            Ok(guard.remove(&format!("{service}:{account}")).is_some())
        }
    }
}

pub mod tests {
    use super::*;

    #[derive(Default, Clone, Debug)]
    pub struct MockKeyringStore {
        data: Arc<Mutex<HashMap<String, String>>>,
    }

    impl MockKeyringStore {
        pub fn saved_value(&self, account: &str) -> Option<String> {
            let guard = self.data.lock().unwrap_or_else(PoisonError::into_inner);
            guard.get(account).cloned()
        }
    }

    impl KeyringStore for MockKeyringStore {
        fn load(
            &self,
            _service: &str,
            account: &str,
        ) -> Result<Option<String>, CredentialStoreError> {
            let guard = self.data.lock().unwrap_or_else(PoisonError::into_inner);
            Ok(guard.get(account).cloned())
        }

        fn save(
            &self,
            _service: &str,
            account: &str,
            value: &str,
        ) -> Result<(), CredentialStoreError> {
            let mut guard = self.data.lock().unwrap_or_else(PoisonError::into_inner);
            guard.insert(account.to_string(), value.to_string());
            Ok(())
        }

        fn delete(
            &self,
            _service: &str,
            account: &str,
        ) -> Result<bool, CredentialStoreError> {
            let mut guard = self.data.lock().unwrap_or_else(PoisonError::into_inner);
            Ok(guard.remove(account).is_some())
        }
    }
}
