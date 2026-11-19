//! Where secrets are kept: the Keychain in the app, memory in tests.

use std::collections::BTreeMap;
use std::sync::Mutex;

/// Secrets by Keychain account (`phin-tech/github-token`, `oauth/sentry`).
pub trait Secrets: Send + Sync {
    fn get(&self, account: &str) -> Option<String>;
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretsError>;
    fn remove(&self, account: &str);
}

/// Why a secret couldn't be kept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("couldn't keep the secret in the Keychain: {0}")]
pub struct SecretsError(pub String);

/// The login Keychain, as generic passwords under the service `Patchbay`.
#[derive(Debug, Clone, Copy)]
pub struct Keychain(typecase_keychain::Keychain);

impl Keychain {
    pub const SERVICE: &str = "Patchbay";

    pub const fn new() -> Keychain {
        Keychain(typecase_keychain::Keychain::new(Keychain::SERVICE))
    }
}

impl Default for Keychain {
    fn default() -> Keychain {
        Keychain::new()
    }
}

impl Secrets for Keychain {
    fn get(&self, account: &str) -> Option<String> {
        self.0.get(account).ok()
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), SecretsError> {
        self.0
            .save(account, secret)
            .map_err(|error| SecretsError(error.to_string()))
    }

    fn remove(&self, account: &str) {
        self.0.remove(account);
    }
}

/// Secrets in a JSON file, for development only: a rebuilt binary has a new signature, and the Keychain asks again
/// for every item it reads. `PATCHBAY_SECRETS=file` uses `secrets.json` in Patchbay's folder instead.
#[derive(Debug)]
pub struct FileSecrets {
    path: std::path::PathBuf,
    lock: Mutex<()>,
}

impl FileSecrets {
    pub fn new(path: impl Into<std::path::PathBuf>) -> FileSecrets {
        FileSecrets {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    fn load(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn store(&self, secrets: &BTreeMap<String, String>) -> Result<(), SecretsError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| SecretsError(error.to_string()))?;
        }
        let text = serde_json::to_string_pretty(secrets).expect("strings always print");
        crate::write_atomically(&self.path, text.as_bytes())
            .map_err(|error| SecretsError(error.to_string()))
    }
}

impl Secrets for FileSecrets {
    fn get(&self, account: &str) -> Option<String> {
        let _held = self.lock.lock().ok()?;
        self.load().remove(account)
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), SecretsError> {
        let _held = self
            .lock
            .lock()
            .map_err(|error| SecretsError(error.to_string()))?;
        let mut secrets = self.load();
        secrets.insert(account.to_string(), secret.trim().to_string());
        self.store(&secrets)
    }

    fn remove(&self, account: &str) {
        if let Ok(_held) = self.lock.lock() {
            let mut secrets = self.load();
            if secrets.remove(account).is_some() {
                let _ = self.store(&secrets);
            }
        }
    }
}

/// The Keychain, or with `PATCHBAY_SECRETS=file`, [`FileSecrets`] in `home`.
pub fn from_env(home: &crate::Home) -> std::sync::Arc<dyn Secrets> {
    if std::env::var("PATCHBAY_SECRETS").as_deref() == Ok("file") {
        std::sync::Arc::new(FileSecrets::new(home.root().join("secrets.json")))
    } else {
        std::sync::Arc::new(Keychain::new())
    }
}

/// Secrets in memory, for tests and trials that mustn't touch the Keychain.
#[derive(Debug, Default)]
pub struct MemorySecrets(Mutex<BTreeMap<String, String>>);

impl MemorySecrets {
    pub fn with(entries: &[(&str, &str)]) -> MemorySecrets {
        MemorySecrets(Mutex::new(
            entries
                .iter()
                .map(|(account, secret)| ((*account).to_string(), (*secret).to_string()))
                .collect(),
        ))
    }
}

impl Secrets for MemorySecrets {
    fn get(&self, account: &str) -> Option<String> {
        self.0.lock().ok()?.get(account).cloned()
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), SecretsError> {
        self.0
            .lock()
            .map_err(|error| SecretsError(error.to_string()))?
            .insert(account.to_string(), secret.trim().to_string());
        Ok(())
    }

    fn remove(&self, account: &str) {
        if let Ok(mut secrets) = self.0.lock() {
            secrets.remove(account);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MemorySecrets, Secrets};

    #[test]
    fn a_secret_kept_in_memory_comes_back_trimmed_and_goes() {
        let secrets = MemorySecrets::default();
        secrets.set("me/x", " abc\n").unwrap();
        assert_eq!(secrets.get("me/x").as_deref(), Some("abc"));
        secrets.remove("me/x");
        assert_eq!(secrets.get("me/x"), None);
    }
}
