use core::fmt;
use std::sync::RwLock;

use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};

// implementation adapted from octocrab: https://github.com/XAMPPRocky/octocrab/blob/d381cc09db8db8e2a0ebbdfbdbf334c7e42f48a2/src/lib.rs#L923

#[derive(Debug, Clone)]
struct CachedTokenInner {
    expiration: Option<DateTime<Utc>>,
    secret: SecretString,
}

impl CachedTokenInner {
    fn new(secret: SecretString, expiration: Option<DateTime<Utc>>) -> Self {
        Self { secret, expiration }
    }

    fn expose_secret(&self) -> &str {
        self.secret.expose_secret()
    }
}

/// A cached API access token (which may be None)
pub struct CachedToken(RwLock<Option<CachedTokenInner>>);

impl CachedToken {
    /// Returns a valid token if it exists and is not expired or if there is no expiration date.
    fn valid_token_with_buffer(&self, buffer: chrono::Duration) -> Option<SecretString> {
        let inner = self.0.read().unwrap();

        if let Some(token) = inner.as_ref() {
            if let Some(exp) = token.expiration {
                if exp - Utc::now() > buffer {
                    return Some(token.secret.clone());
                }
            } else {
                return Some(token.secret.clone());
            }
        }

        None
    }

    pub fn valid_token(&self) -> Option<SecretString> {
        self.valid_token_with_buffer(chrono::Duration::seconds(30))
    }

    pub fn set<S: Into<SecretString>>(&self, token: S, expiration: Option<DateTime<Utc>>) {
        *self.0.write().unwrap() = Some(CachedTokenInner::new(token.into(), expiration));
    }
}

impl fmt::Debug for CachedToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.read().unwrap().fmt(f)
    }
}

impl fmt::Display for CachedToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let option = self.0.read().unwrap();
        option
            .as_ref()
            .map(|s| s.expose_secret().fmt(f))
            .unwrap_or_else(|| write!(f, "<none>"))
    }
}

impl Clone for CachedToken {
    fn clone(&self) -> CachedToken {
        CachedToken(RwLock::new(self.0.read().unwrap().clone()))
    }
}

impl Default for CachedToken {
    fn default() -> CachedToken {
        CachedToken(RwLock::new(None))
    }
}
