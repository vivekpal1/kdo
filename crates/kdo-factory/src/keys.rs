//! Bring-your-own credentials.
//!
//! Resolution order for a provider: the environment variable named by the
//! plugin, then `[keys.<provider>]` in the credentials file. The file is
//! never required. Values are redacted from `Debug`.

use crate::error::{FactoryError, FactoryResult};
use crate::plugin::ProviderPlugin;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct KeyStore {
    path: Option<PathBuf>,
    keys: BTreeMap<String, String>,
    perm_warning: Option<String>,
}

impl std::fmt::Debug for KeyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyStore")
            .field("path", &self.path)
            .field("providers", &self.keys.keys().collect::<Vec<_>>())
            .field("perm_warning", &self.perm_warning)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyStatus {
    pub provider: String,
    pub present: bool,
}

impl KeyStore {
    fn empty(path: Option<PathBuf>) -> Self {
        Self {
            path,
            keys: BTreeMap::new(),
            perm_warning: None,
        }
    }

    /// `$KDO_CREDENTIALS`, otherwise `~/.kdo/credentials.toml`.
    pub fn credentials_path() -> Option<PathBuf> {
        if let Some(explicit) = std::env::var_os("KDO_CREDENTIALS") {
            return Some(PathBuf::from(explicit));
        }
        home_dir().map(|home| home.join(".kdo").join("credentials.toml"))
    }

    pub fn load_default() -> FactoryResult<Self> {
        match Self::credentials_path() {
            Some(path) => Self::load(&path),
            None => Ok(Self::empty(None)),
        }
    }

    pub fn load(path: &Path) -> FactoryResult<Self> {
        if !path.exists() {
            return Ok(Self::empty(Some(path.to_path_buf())));
        }
        let text = std::fs::read_to_string(path)?;
        let value: toml::Value =
            toml::from_str(&text).map_err(|err| FactoryError::Plugin(err.to_string()))?;
        let table = value
            .get("keys")
            .and_then(toml::Value::as_table)
            .ok_or_else(|| FactoryError::Plugin(format!("{} is missing [keys]", path.display())))?;
        let mut keys = BTreeMap::new();
        for (name, value) in table {
            let Some(secret) = value.as_str() else {
                return Err(FactoryError::Plugin(format!(
                    "keys.{name} must be a string"
                )));
            };
            keys.insert(name.clone(), secret.to_string());
        }
        Ok(Self {
            path: Some(path.to_path_buf()),
            keys,
            perm_warning: permissions_warning(path),
        })
    }

    pub fn perm_warning(&self) -> Option<&str> {
        self.perm_warning.as_deref()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Environment first, then the file. Empty strings do not count.
    pub fn resolve(&self, provider: &str, env_var: &str) -> FactoryResult<String> {
        if !env_var.is_empty() {
            if let Ok(value) = std::env::var(env_var) {
                if !value.is_empty() {
                    return Ok(value);
                }
            }
        }
        if let Some(value) = self.keys.get(provider) {
            if !value.is_empty() {
                return Ok(value.clone());
            }
        }
        Err(FactoryError::MissingCredential(provider.to_string()))
    }

    pub fn status(&self, providers: &[ProviderPlugin]) -> Vec<KeyStatus> {
        providers
            .iter()
            .map(|provider| KeyStatus {
                provider: provider.name.clone(),
                present: self.resolve(&provider.name, &provider.api_key_env).is_ok(),
            })
            .collect()
    }

    /// Secrets long enough to be worth redacting. Short values are skipped
    /// so we don't blank out ordinary words.
    pub fn secret_values(&self) -> Vec<String> {
        self.keys
            .values()
            .filter(|value| value.len() >= 8)
            .cloned()
            .collect()
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_string();
    for secret in secrets {
        if secret.len() < 8 {
            continue;
        }
        if out.contains(secret.as_str()) {
            out = out.replace(secret.as_str(), "***");
        }
    }
    out
}

fn permissions_warning(path: &Path) -> Option<String> {
    permissions_warning_unix(path)
}

#[cfg(unix)]
fn permissions_warning_unix(path: &Path) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).ok()?.permissions().mode();
    if mode & 0o077 == 0 {
        return None;
    }
    Some(format!(
        "{} is readable by group or other (mode {:o}). Run chmod 600.",
        path.display(),
        mode & 0o777
    ))
}

#[cfg(not(unix))]
fn permissions_warning_unix(_path: &Path) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{Protocol, ProviderPlugin};

    fn provider(name: &str, env_var: &str) -> ProviderPlugin {
        ProviderPlugin {
            name: name.into(),
            protocol: Protocol::Openai,
            base_url: "https://example.invalid".into(),
            api_key_env: env_var.into(),
            models: vec!["demo".into()],
        }
    }

    #[test]
    fn file_key_is_used_when_env_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.toml");
        std::fs::write(&path, "[keys]\ndemo = \"supersecret-value\"\n").unwrap();
        let store = KeyStore::load(&path).unwrap();
        assert_eq!(
            store.resolve("demo", "KDO_FACTORY_TEST_KEY_UNSET").unwrap(),
            "supersecret-value"
        );
        let status = store.status(&[provider("demo", "KDO_FACTORY_TEST_KEY_UNSET")]);
        assert!(status[0].present);
        let rendered = format!("{store:?}");
        assert!(!rendered.contains("supersecret-value"));
        assert!(rendered.contains("demo"));
    }

    #[test]
    fn env_beats_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.toml");
        std::fs::write(&path, "[keys]\ndemo = \"file-secret-value\"\n").unwrap();
        let store = KeyStore::load(&path).unwrap();
        std::env::set_var("KDO_FACTORY_TEST_KEY_PRESENT", "env-secret-value");
        let resolved = store
            .resolve("demo", "KDO_FACTORY_TEST_KEY_PRESENT")
            .unwrap();
        std::env::remove_var("KDO_FACTORY_TEST_KEY_PRESENT");
        assert_eq!(resolved, "env-secret-value");
    }

    #[test]
    fn missing_key_is_an_error_without_the_secret() {
        let store = KeyStore::empty(None);
        let err = store
            .resolve("demo", "KDO_FACTORY_TEST_KEY_UNSET")
            .unwrap_err();
        assert!(matches!(err, FactoryError::MissingCredential(_)));
        assert!(!err.to_string().contains("supersecret"));
    }

    #[test]
    fn redact_replaces_long_secrets_only() {
        let secrets = vec!["short".into(), "supersecret-value".into()];
        let text = redact("token supersecret-value and short", &secrets);
        assert_eq!(text, "token *** and short");
    }
}
