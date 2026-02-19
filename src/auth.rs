use color_eyre::eyre::Result;

use crate::config::AuthConfig;
use crate::infrastructure::keyring::TokenStore;

/// Token retrieval priority:
/// 1. Environment variable `DISCORD_TOKEN` (highest priority)
/// 2. Keyring (OS secure storage)
/// 3. Config file token_file path
///
/// Returns None if no token is found from any source.
pub fn retrieve_token(
    config: &AuthConfig,
    keyring: &dyn TokenStore,
    env_getter: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<String>> {
    // 1. Environment variable (highest priority)
    if let Some(token) = env_getter("DISCORD_TOKEN") {
        if !token.is_empty() {
            tracing::info!("Token found in DISCORD_TOKEN environment variable");
            return Ok(Some(token));
        }
    }

    // 2. Keyring
    if config.token_source == "keyring" {
        if let Some(token) = keyring.get_token()? {
            if !token.is_empty() {
                tracing::info!("Token found in keyring");
                return Ok(Some(token));
            }
        }
    }

    // 3. Config file
    if let Some(ref token_file) = config.token_file {
        let expanded = shellexpand(token_file);
        let path = std::path::Path::new(&expanded);
        if path.exists() {
            let token = std::fs::read_to_string(path)
                .map_err(|e| color_eyre::eyre::eyre!("Failed to read token file: {}", e))?;
            let token = token.trim().to_string();
            if !token.is_empty() {
                tracing::info!("Token found in file: {}", token_file);
                return Ok(Some(token));
            }
        }
    }

    Ok(None)
}

/// Store a token in the keyring for future sessions.
pub fn store_token(keyring: &dyn TokenStore, token: &str) -> Result<()> {
    keyring.set_token(token)?;
    tracing::info!("Token stored in keyring");
    Ok(())
}

/// Delete the stored token from the keyring.
pub fn delete_token(keyring: &dyn TokenStore) -> Result<()> {
    keyring.delete_token()?;
    tracing::info!("Token deleted from keyring");
    Ok(())
}

/// Simple ~ expansion for token file paths.
fn shellexpand(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::keyring::MemoryTokenStore;

    fn default_auth_config() -> AuthConfig {
        AuthConfig {
            token_source: "keyring".to_string(),
            token_file: None,
        }
    }

    fn no_env(_key: &str) -> Option<String> {
        None
    }

    #[test]
    fn env_var_has_highest_priority() {
        let config = default_auth_config();
        let keyring = MemoryTokenStore::with_token("keyring_token");

        let env_fn = |key: &str| -> Option<String> {
            if key == "DISCORD_TOKEN" {
                Some("env_token".to_string())
            } else {
                None
            }
        };

        let token = retrieve_token(&config, &keyring, &env_fn).unwrap();
        assert_eq!(token, Some("env_token".to_string()));
    }

    #[test]
    fn keyring_is_second_priority() {
        let config = default_auth_config();
        let keyring = MemoryTokenStore::with_token("keyring_token");

        let token = retrieve_token(&config, &keyring, &no_env).unwrap();
        assert_eq!(token, Some("keyring_token".to_string()));
    }

    #[test]
    fn token_file_is_third_priority() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("token");
        std::fs::write(&token_path, "file_token\n").unwrap();

        let config = AuthConfig {
            token_source: "file".to_string(),
            token_file: Some(token_path.to_str().unwrap().to_string()),
        };
        let keyring = MemoryTokenStore::new(); // Empty keyring

        let token = retrieve_token(&config, &keyring, &no_env).unwrap();
        assert_eq!(token, Some("file_token".to_string()));
    }

    #[test]
    fn returns_none_when_no_token_found() {
        let config = AuthConfig {
            token_source: "file".to_string(),
            token_file: None,
        };
        let keyring = MemoryTokenStore::new();

        let token = retrieve_token(&config, &keyring, &no_env).unwrap();
        assert!(token.is_none());
    }

    #[test]
    fn env_var_empty_string_is_skipped() {
        let config = default_auth_config();
        let keyring = MemoryTokenStore::with_token("keyring_token");

        let env_fn = |key: &str| -> Option<String> {
            if key == "DISCORD_TOKEN" {
                Some(String::new()) // Empty
            } else {
                None
            }
        };

        let token = retrieve_token(&config, &keyring, &env_fn).unwrap();
        assert_eq!(token, Some("keyring_token".to_string()));
    }

    #[test]
    fn keyring_not_checked_when_source_is_file() {
        let config = AuthConfig {
            token_source: "file".to_string(),
            token_file: None,
        };
        let keyring = MemoryTokenStore::with_token("keyring_token");

        // Keyring has a token but token_source is "file", so it's skipped
        let token = retrieve_token(&config, &keyring, &no_env).unwrap();
        assert!(token.is_none());
    }

    #[test]
    fn store_token_in_keyring() {
        let keyring = MemoryTokenStore::new();

        store_token(&keyring, "new_token_123").unwrap();
        assert_eq!(
            keyring.get_token().unwrap(),
            Some("new_token_123".to_string())
        );
    }

    #[test]
    fn delete_token_from_keyring() {
        let keyring = MemoryTokenStore::with_token("token_to_delete");

        delete_token(&keyring).unwrap();
        assert!(keyring.get_token().unwrap().is_none());
    }

    #[test]
    fn token_file_is_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("token");
        std::fs::write(&token_path, "  trimmed_token  \n\n").unwrap();

        let config = AuthConfig {
            token_source: "file".to_string(),
            token_file: Some(token_path.to_str().unwrap().to_string()),
        };
        let keyring = MemoryTokenStore::new();

        let token = retrieve_token(&config, &keyring, &no_env).unwrap();
        assert_eq!(token, Some("trimmed_token".to_string()));
    }

    #[test]
    fn nonexistent_token_file_returns_none() {
        let config = AuthConfig {
            token_source: "file".to_string(),
            token_file: Some("/nonexistent/path/token".to_string()),
        };
        let keyring = MemoryTokenStore::new();

        let token = retrieve_token(&config, &keyring, &no_env).unwrap();
        assert!(token.is_none());
    }

    #[test]
    fn shellexpand_expands_tilde() {
        let expanded = shellexpand("~/test/path");
        assert!(!expanded.starts_with("~/"));
        assert!(expanded.ends_with("test/path"));
    }

    #[test]
    fn shellexpand_leaves_absolute_path() {
        let expanded = shellexpand("/absolute/path");
        assert_eq!(expanded, "/absolute/path");
    }

    #[test]
    fn priority_env_over_keyring_over_file() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("token");
        std::fs::write(&token_path, "file_token").unwrap();

        let config = AuthConfig {
            token_source: "keyring".to_string(),
            token_file: Some(token_path.to_str().unwrap().to_string()),
        };
        let keyring = MemoryTokenStore::with_token("keyring_token");

        // With env var → env wins
        let env_fn = |key: &str| -> Option<String> {
            if key == "DISCORD_TOKEN" {
                Some("env_token".to_string())
            } else {
                None
            }
        };
        let token = retrieve_token(&config, &keyring, &env_fn).unwrap();
        assert_eq!(token, Some("env_token".to_string()));

        // Without env var → keyring wins
        let token = retrieve_token(&config, &keyring, &no_env).unwrap();
        assert_eq!(token, Some("keyring_token".to_string()));

        // Without env var or keyring → file wins
        let empty_keyring = MemoryTokenStore::new();
        let token = retrieve_token(&config, &empty_keyring, &no_env).unwrap();
        assert_eq!(token, Some("file_token".to_string()));
    }
}
