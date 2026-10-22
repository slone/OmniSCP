// Saved connection secrets (password or key passphrase), kept in the OS
// credential store under one entry per profile id.

use keyring::Entry;

use crate::error::{OmniError, Result};

const SERVICE: &str = "com.guillaumechantraine.omniscp";

fn entry(profile_id: &str) -> Result<Entry> {
    Entry::new(SERVICE, profile_id).map_err(store_err)
}

fn store_err(err: keyring::Error) -> OmniError {
    // keyring errors never include the secret itself.
    OmniError::Config(format!("Credential store error: {err}"))
}

pub fn get(profile_id: &str) -> Result<Option<String>> {
    match entry(profile_id)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(store_err(e)),
    }
}

pub fn set(profile_id: &str, secret: &str) -> Result<()> {
    entry(profile_id)?.set_password(secret).map_err(store_err)
}

pub fn delete(profile_id: &str) -> Result<()> {
    match entry(profile_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(store_err(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real OS credential store; ignored by default.
    #[test]
    #[ignore]
    fn round_trip_in_os_store() {
        let id = format!("omniscp-test-{}", std::process::id());
        assert_eq!(get(&id).unwrap(), None);
        set(&id, "s3cret").unwrap();
        assert_eq!(get(&id).unwrap().as_deref(), Some("s3cret"));
        set(&id, "changed").unwrap();
        assert_eq!(get(&id).unwrap().as_deref(), Some("changed"));
        delete(&id).unwrap();
        delete(&id).unwrap(); // idempotent
        assert_eq!(get(&id).unwrap(), None);
    }
}
