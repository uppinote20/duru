//! Anthropic API key storage for the warming daemon.
//!
//! Keys live in the OS keychain via the `keyring` crate (Keychain on macOS,
//! Credential Manager on Windows, Secret Service on Linux). A [`SecretBackend`]
//! trait keeps logic testable without touching real credentials.

use std::io;
#[cfg(test)]
use std::sync::Mutex;

const SERVICE: &str = "duru-warmer";
const ACCOUNT: &str = "anthropic-api-key";

pub trait SecretBackend {
    fn get(&self) -> io::Result<Option<String>>;
    fn set(&self, value: &str) -> io::Result<()>;
    fn remove(&self) -> io::Result<()>;
}

pub struct KeyringBackend;

impl KeyringBackend {
    pub fn new() -> Self {
        Self
    }

    fn entry(&self) -> io::Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(map_keyring_err)
    }
}

impl Default for KeyringBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretBackend for KeyringBackend {
    fn get(&self) -> io::Result<Option<String>> {
        match self.entry()?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_keyring_err(e)),
        }
    }

    fn set(&self, value: &str) -> io::Result<()> {
        self.entry()?.set_password(value).map_err(map_keyring_err)
    }

    fn remove(&self) -> io::Result<()> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring_err(e)),
        }
    }
}

fn map_keyring_err(e: keyring::Error) -> io::Error {
    io::Error::other(format!("keyring: {e}"))
}

#[cfg(test)]
pub struct MemoryBackend {
    cell: Mutex<Option<String>>,
}

#[cfg(test)]
impl MemoryBackend {
    pub fn new() -> Self {
        Self {
            cell: Mutex::new(None),
        }
    }
}

#[cfg(test)]
impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl SecretBackend for MemoryBackend {
    fn get(&self) -> io::Result<Option<String>> {
        Ok(self.cell.lock().unwrap().clone())
    }
    fn set(&self, value: &str) -> io::Result<()> {
        *self.cell.lock().unwrap() = Some(value.to_string());
        Ok(())
    }
    fn remove(&self) -> io::Result<()> {
        *self.cell.lock().unwrap() = None;
        Ok(())
    }
}

pub fn set_api_key(backend: &dyn SecretBackend, value: &str) -> io::Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "api key is empty",
        ));
    }
    backend.set(trimmed)
}

pub fn get_api_key(backend: &dyn SecretBackend) -> io::Result<Option<String>> {
    backend.get()
}

pub fn remove_api_key(backend: &dyn SecretBackend) -> io::Result<()> {
    backend.remove()
}

// Consumer lands in MVP3 #21 (daemon startup pre-flight) + #23 (no_api_key guardrail log).
#[allow(dead_code)]
pub fn has_api_key(backend: &dyn SecretBackend) -> io::Result<bool> {
    Ok(backend.get()?.is_some())
}

pub fn redact_key(key: &str) -> String {
    const PREFIX_LEN: usize = 9;
    let trimmed = key.trim();
    if trimmed.chars().count() <= PREFIX_LEN + 1 {
        return "…".to_string();
    }
    let prefix: String = trimmed.chars().take(PREFIX_LEN).collect();
    format!("{prefix}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_key_typical_anthropic_key() {
        let full = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789";
        let got = redact_key(full);
        assert_eq!(got, "sk-ant-ap…");
        assert!(!got.contains("api03-abc"));
    }

    #[test]
    fn redact_key_short_input_fully_redacted() {
        assert_eq!(redact_key(""), "…");
        assert_eq!(redact_key("sk-ant"), "…");
        assert_eq!(redact_key("sk-ant-abc"), "…");
    }

    #[test]
    fn redact_key_trims_whitespace_first() {
        let full = "   sk-ant-api03-xyz_0123456789   ";
        assert_eq!(redact_key(full), "sk-ant-ap…");
    }

    #[test]
    fn redact_key_never_includes_full_secret() {
        let full = "sk-ant-api03-SENSITIVE_MATERIAL_DO_NOT_LEAK";
        let redacted = redact_key(full);
        assert!(!redacted.contains("SENSITIVE"));
        assert!(!redacted.contains("MATERIAL"));
        assert!(redacted.ends_with('…'));
    }

    #[test]
    fn memory_backend_roundtrip() {
        let backend = MemoryBackend::new();
        assert_eq!(backend.get().unwrap(), None);

        backend.set("sk-ant-test-key").unwrap();
        assert_eq!(backend.get().unwrap().as_deref(), Some("sk-ant-test-key"));

        backend.remove().unwrap();
        assert_eq!(backend.get().unwrap(), None);
    }

    #[test]
    fn memory_backend_remove_is_idempotent() {
        let backend = MemoryBackend::new();
        backend.remove().unwrap();
        backend.remove().unwrap();
        assert_eq!(backend.get().unwrap(), None);
    }

    #[test]
    fn set_api_key_rejects_empty() {
        let backend = MemoryBackend::new();
        let err = set_api_key(&backend, "").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

        let err = set_api_key(&backend, "   \t\n").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

        assert_eq!(backend.get().unwrap(), None);
    }

    #[test]
    fn set_api_key_trims_before_storing() {
        let backend = MemoryBackend::new();
        set_api_key(&backend, "  sk-ant-test  ").unwrap();
        assert_eq!(backend.get().unwrap().as_deref(), Some("sk-ant-test"));
    }

    #[test]
    fn get_api_key_absent_is_none() {
        let backend = MemoryBackend::new();
        assert_eq!(get_api_key(&backend).unwrap(), None);
    }

    #[test]
    fn has_api_key_reflects_state() {
        let backend = MemoryBackend::new();
        assert!(!has_api_key(&backend).unwrap());

        set_api_key(&backend, "sk-ant-test").unwrap();
        assert!(has_api_key(&backend).unwrap());

        remove_api_key(&backend).unwrap();
        assert!(!has_api_key(&backend).unwrap());
    }
}
