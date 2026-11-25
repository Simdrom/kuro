//! Tokens live in the Windows Credential Manager, never on disk or in settings.

use std::collections::HashMap;
use std::sync::Mutex;

use kuro_core::Service;
use kuro_core::kioku::Platform;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::http::{ApiError, Result};

#[derive(Clone, Default, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
    /// Unix seconds.
    pub expires_at: i64,
    /// Device the session was issued to, for services that tie it to one.
    #[serde(default)]
    pub device: Option<String>,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Tokens(expires_at: {})", self.expires_at)
    }
}

/// What a stored secret belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretKey {
    Service(Service),
    /// A streaming platform connected to Kioku.
    Kioku(Platform),
}

impl SecretKey {
    /// Name of the credential. Services keep the names they always had.
    fn name(self) -> String {
        match self {
            Self::Service(s) => s.key().to_owned(),
            Self::Kioku(p) => format!("kioku.{}", p.key()),
        }
    }
}

impl From<Service> for SecretKey {
    fn from(s: Service) -> Self {
        Self::Service(s)
    }
}

pub trait SecretStore: Send + Sync {
    fn load(&self, key: SecretKey) -> Result<Option<Tokens>>;
    fn save(&self, key: SecretKey, tokens: &Tokens) -> Result<()>;
    fn delete(&self, key: SecretKey) -> Result<()>;
}

/// A credential blob holds at most 2560 bytes, so tokens are split.
const CHUNK: usize = 2000;
const MAX_CHUNKS: usize = 8;

pub struct Keyring {
    app: String,
}

impl Keyring {
    pub fn new(app: &str) -> Self {
        Self { app: app.to_owned() }
    }

    /// Takes over the tokens stored under an older name.
    pub fn adopt(&self, old: &Keyring) -> Result<()> {
        for s in Service::ALL.map(SecretKey::from) {
            let Some(tokens) = old.load(s)? else { continue };
            if self.load(s)?.is_none() {
                self.save(s, &tokens)?;
            }
            // Only removed once the new copy is safe.
            old.delete(s)?;
        }
        Ok(())
    }

    fn entry(&self, key: SecretKey, i: usize) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.app, &format!("{}.{i}", key.name())).map_err(secret_error)
    }
}

fn secret_error(e: keyring::Error) -> ApiError {
    // Some variants carry the stored bytes; only the kind is reported.
    let kind = format!("{e:?}");
    ApiError::Secrets(kind.split(['(', '{', ' ']).next().unwrap_or("error").to_owned())
}

impl SecretStore for Keyring {
    fn load(&self, key: SecretKey) -> Result<Option<Tokens>> {
        let mut bytes = Zeroizing::new(Vec::new());
        for i in 0..MAX_CHUNKS {
            match self.entry(key, i)?.get_secret() {
                Ok(part) => bytes.extend_from_slice(&Zeroizing::new(part)),
                Err(keyring::Error::NoEntry) => break,
                Err(e) => return Err(secret_error(e)),
            }
        }
        // An unreadable token just means signing in again.
        Ok(serde_json::from_slice(&bytes).ok())
    }

    fn save(&self, key: SecretKey, tokens: &Tokens) -> Result<()> {
        let bytes = Zeroizing::new(serde_json::to_vec(tokens).map_err(|e| ApiError::Decode(e.to_string()))?);
        if bytes.len() > CHUNK * MAX_CHUNKS {
            return Err(ApiError::TooLarge);
        }
        self.delete(key)?;
        for (i, part) in bytes.chunks(CHUNK).enumerate() {
            self.entry(key, i)?.set_secret(part).map_err(secret_error)?;
        }
        Ok(())
    }

    fn delete(&self, key: SecretKey) -> Result<()> {
        for i in 0..MAX_CHUNKS {
            match self.entry(key, i)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => return Err(secret_error(e)),
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<SecretKey, Tokens>>);

impl MemorySecrets {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<SecretKey, Tokens>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SecretStore for MemorySecrets {
    fn load(&self, key: SecretKey) -> Result<Option<Tokens>> {
        Ok(self.map().get(&key).cloned())
    }
    fn save(&self, key: SecretKey, tokens: &Tokens) -> Result<()> {
        self.map().insert(key, tokens.clone());
        Ok(())
    }
    fn delete(&self, key: SecretKey) -> Result<()> {
        self.map().remove(&key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_tokens() {
        let t = Tokens { access: "AAA".into(), refresh: Some("BBB".into()), expires_at: 1, device: None };
        let s = format!("{t:?}");
        assert!(!s.contains("AAA") && !s.contains("BBB"));
    }

    #[cfg(windows)]
    #[test]
    fn adopts_tokens_from_an_older_name() {
        let pid = std::process::id();
        let (old, new) =
            (Keyring::new(&format!("kuro-test-old-{pid}")), Keyring::new(&format!("kuro-test-new-{pid}")));
        let token = |a: &str| Tokens { access: a.into(), refresh: None, expires_at: 1, device: None };
        old.save(Service::AniList.into(), &token("a")).unwrap();
        old.save(Service::Mal.into(), &token("stale")).unwrap();
        new.save(Service::Mal.into(), &token("newer")).unwrap();
        new.adopt(&old).unwrap();
        assert_eq!(new.load(Service::AniList.into()).unwrap().unwrap().access, "a");
        assert_eq!(new.load(Service::Mal.into()).unwrap().unwrap().access, "newer");
        for s in Service::ALL.map(SecretKey::from) {
            assert!(old.load(s).unwrap().is_none());
            new.delete(s).unwrap();
        }
        new.adopt(&old).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn credential_manager_round_trip_with_long_tokens() {
        let k = Keyring::new(&format!("kuro-test-{}", std::process::id()));
        let long =
            Tokens { access: "a".repeat(5000), refresh: Some("b".repeat(3000)), expires_at: 7, device: None };
        k.save(Service::Mal.into(), &long).unwrap();
        assert_eq!(k.load(Service::Mal.into()).unwrap().unwrap().access, long.access);
        // A shorter token must not inherit the old tail.
        let short = Tokens { access: "c".into(), refresh: None, expires_at: 8, device: None };
        k.save(Service::Mal.into(), &short).unwrap();
        assert_eq!(k.load(Service::Mal.into()).unwrap().unwrap().access, "c");
        k.delete(Service::Mal.into()).unwrap();
        assert!(k.load(Service::Mal.into()).unwrap().is_none());
        k.delete(Service::Mal.into()).unwrap();
    }
}
