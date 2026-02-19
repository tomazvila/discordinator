use color_eyre::eyre::Result;

/// Service name used for keyring storage.
const SERVICE_NAME: &str = "discordinator";
/// Account name used for keyring storage.
const ACCOUNT_NAME: &str = "discord_token";

/// Trait for token storage backends. Allows mocking in tests.
pub trait TokenStore: Send + Sync {
    fn get_token(&self) -> Result<Option<String>>;
    fn set_token(&self, token: &str) -> Result<()>;
    fn delete_token(&self) -> Result<()>;
}

/// Real keyring-based token store using the `keyring` crate.
pub struct KeyringStore;

impl TokenStore for KeyringStore {
    fn get_token(&self) -> Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE_NAME, ACCOUNT_NAME)
            .map_err(|e| color_eyre::eyre::eyre!("Keyring entry error: {}", e))?;
        match entry.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(color_eyre::eyre::eyre!("Keyring get error: {}", e)),
        }
    }

    fn set_token(&self, token: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE_NAME, ACCOUNT_NAME)
            .map_err(|e| color_eyre::eyre::eyre!("Keyring entry error: {}", e))?;
        entry
            .set_password(token)
            .map_err(|e| color_eyre::eyre::eyre!("Keyring set error: {}", e))?;
        Ok(())
    }

    fn delete_token(&self) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE_NAME, ACCOUNT_NAME)
            .map_err(|e| color_eyre::eyre::eyre!("Keyring entry error: {}", e))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // Already deleted
            Err(e) => Err(color_eyre::eyre::eyre!("Keyring delete error: {}", e)),
        }
    }
}

/// In-memory token store for testing.
#[derive(Default)]
pub struct MemoryTokenStore {
    token: std::sync::Mutex<Option<String>>,
}

impl MemoryTokenStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_token(token: &str) -> Self {
        Self {
            token: std::sync::Mutex::new(Some(token.to_string())),
        }
    }
}

impl TokenStore for MemoryTokenStore {
    fn get_token(&self) -> Result<Option<String>> {
        Ok(self.token.lock().unwrap().clone())
    }

    fn set_token(&self, token: &str) -> Result<()> {
        *self.token.lock().unwrap() = Some(token.to_string());
        Ok(())
    }

    fn delete_token(&self) -> Result<()> {
        *self.token.lock().unwrap() = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_get_set_delete() {
        let store = MemoryTokenStore::new();

        // Initially empty
        assert!(store.get_token().unwrap().is_none());

        // Set and get
        store.set_token("test_token_123").unwrap();
        assert_eq!(
            store.get_token().unwrap(),
            Some("test_token_123".to_string())
        );

        // Delete
        store.delete_token().unwrap();
        assert!(store.get_token().unwrap().is_none());
    }

    #[test]
    fn memory_store_with_initial_token() {
        let store = MemoryTokenStore::with_token("initial_token");
        assert_eq!(
            store.get_token().unwrap(),
            Some("initial_token".to_string())
        );
    }

    #[test]
    fn memory_store_overwrite_token() {
        let store = MemoryTokenStore::with_token("first");
        store.set_token("second").unwrap();
        assert_eq!(store.get_token().unwrap(), Some("second".to_string()));
    }

    #[test]
    fn memory_store_delete_when_empty_is_ok() {
        let store = MemoryTokenStore::new();
        // Should not error when deleting from empty store
        store.delete_token().unwrap();
    }
}
