use base64::Engine;
use color_eyre::eyre::{eyre, Result};
use secrecy::{ExposeSecret, SecretString};

use crate::config::{AuthConfig, DiscordConfig};
use crate::infrastructure::anti_detection;
use crate::infrastructure::keyring::TokenStore;

/// Token retrieval priority:
/// 1. Environment variable `DISCORD_TOKEN` (highest priority)
/// 2. Keyring (OS secure storage)
/// 3. Config file `token_file` path
///
/// Returns None if no token is found from any source.
pub fn retrieve_token(
    config: &AuthConfig,
    keyring: &dyn TokenStore,
    env_getter: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<SecretString>> {
    // 1. Environment variable (highest priority)
    if let Some(token) = env_getter("DISCORD_TOKEN") {
        if !token.is_empty() {
            tracing::info!("Token found in DISCORD_TOKEN environment variable");
            return Ok(Some(SecretString::from(token)));
        }
    }

    // 2. Keyring
    if config.token_source == "keyring" {
        if let Some(token) = keyring.get_token()? {
            if !token.expose_secret().is_empty() {
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
                .map_err(|e| eyre!("Failed to read token file: {}", e))?;
            let token = token.trim().to_string();
            if !token.is_empty() {
                tracing::info!("Token found in file: {}", token_file);
                return Ok(Some(SecretString::from(token)));
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

// === Task 39: Email + Password Login ===

/// Response from `login_with_credentials`.
#[derive(Debug, Clone, PartialEq)]
pub enum LoginResponse {
    /// Login successful, contains the auth token.
    Token(String),
    /// 2FA required, contains the MFA ticket.
    MfaRequired { ticket: String },
}

/// Build a reqwest Client with anti-detection headers for unauthenticated requests.
fn build_auth_client(config: &DiscordConfig) -> Result<reqwest::Client> {
    use reqwest::header::{HeaderMap, HeaderValue};

    let super_props = anti_detection::build_super_properties(config);
    let mut headers = HeaderMap::new();
    headers.insert(
        "User-Agent",
        HeaderValue::from_str(&config.browser_user_agent)
            .map_err(|e| eyre!("Invalid User-Agent: {}", e))?,
    );
    headers.insert(
        "X-Super-Properties",
        HeaderValue::from_str(&super_props)
            .map_err(|e| eyre!("Invalid X-Super-Properties: {}", e))?,
    );
    headers.insert("X-Discord-Locale", HeaderValue::from_static("en-US"));

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| eyre!("Failed to build HTTP client: {}", e))
}

/// Login with email + password via Discord's auth endpoint.
/// Uses anti-detection headers. `api_base` is parameterized for testing.
pub async fn login_with_credentials(
    email: &str,
    password: &str,
    config: &DiscordConfig,
    api_base: &str,
) -> Result<LoginResponse> {
    let client = build_auth_client(config)?;

    let body = serde_json::json!({
        "login": email,
        "password": password,
        "undelete": false,
        "login_source": null,
        "gift_code_sku_id": null
    });

    let response = client
        .post(format!("{api_base}/auth/login"))
        .json(&body)
        .send()
        .await
        .map_err(|e| eyre!("Login request failed: {}", e))?;

    let status = response.status();
    let response_body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| eyre!("Failed to parse login response: {}", e))?;

    if !status.is_success() {
        let message = response_body["message"].as_str().unwrap_or("Login failed");
        return Err(eyre!("{}", message));
    }

    // Check if token is present (no 2FA)
    if let Some(token) = response_body["token"].as_str() {
        if !token.is_empty() && token != "null" {
            return Ok(LoginResponse::Token(token.to_string()));
        }
    }

    // Check for MFA required
    if let Some(ticket) = response_body["ticket"].as_str() {
        return Ok(LoginResponse::MfaRequired {
            ticket: ticket.to_string(),
        });
    }

    Err(eyre!("Unexpected login response"))
}

/// Submit TOTP code for 2FA authentication.
/// Returns the auth token on success.
pub async fn submit_mfa_totp(
    ticket: &str,
    code: &str,
    config: &DiscordConfig,
    api_base: &str,
) -> Result<String> {
    let client = build_auth_client(config)?;

    let body = serde_json::json!({
        "code": code,
        "ticket": ticket,
        "login_source": null,
        "gift_code_sku_id": null
    });

    let response = client
        .post(format!("{api_base}/auth/mfa/totp"))
        .json(&body)
        .send()
        .await
        .map_err(|e| eyre!("MFA request failed: {}", e))?;

    let status = response.status();
    let response_body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| eyre!("Failed to parse MFA response: {}", e))?;

    if !status.is_success() {
        let message = response_body["message"]
            .as_str()
            .unwrap_or("MFA verification failed");
        return Err(eyre!("{}", message));
    }

    response_body["token"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| eyre!("No token in MFA response"))
}

// === Task 38: Token Validation via Gateway ===

/// Validate a token by attempting a gateway connection.
/// Connects to the gateway, sends IDENTIFY, and checks for READY vs error.
/// Returns Ok(true) for valid token, Ok(false) for invalid, Err for connection failure.
pub async fn validate_token_via_gateway(
    token: &str,
    gateway_url: &str,
    config: &DiscordConfig,
) -> Result<bool> {
    use crate::domain::event::{self, GatewayEvent};
    use crate::infrastructure::gateway::build_identify_payload;
    use futures_util::{SinkExt, StreamExt};
    use std::time::Duration;
    use tokio_tungstenite::tungstenite::Message;

    let (ws_stream, _) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async(gateway_url),
    )
    .await
    .map_err(|_| eyre!("Gateway connection timed out"))?
    .map_err(|e| eyre!("Gateway connection failed: {}", e))?;

    let (mut write, mut read) = ws_stream.split();

    // Wait for HELLO
    let hello_msg = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .map_err(|_| eyre!("Timeout waiting for HELLO"))?
        .ok_or_else(|| eyre!("Connection closed before HELLO"))?
        .map_err(|e| eyre!("WebSocket error: {}", e))?;

    let hello_text = hello_msg
        .into_text()
        .map_err(|e| eyre!("HELLO not text: {}", e))?;
    let hello_payload: serde_json::Value =
        serde_json::from_str(&hello_text).map_err(|e| eyre!("HELLO parse error: {}", e))?;
    let hello_event = event::parse_gateway_payload(&hello_payload);

    if !matches!(hello_event, GatewayEvent::Hello { .. }) {
        return Err(eyre!("Expected HELLO, got {:?}", hello_event));
    }

    // Send IDENTIFY
    let identify = build_identify_payload(token, config);
    let identify_text = serde_json::to_string(&identify)?;
    write
        .send(Message::Text(identify_text.into()))
        .await
        .map_err(|e| eyre!("Failed to send IDENTIFY: {}", e))?;

    // Wait for READY or error
    let response = tokio::time::timeout(Duration::from_secs(10), read.next())
        .await
        .map_err(|_| eyre!("Timeout waiting for READY"))?
        .ok_or_else(|| eyre!("Connection closed after IDENTIFY"))?
        .map_err(|e| eyre!("WebSocket error after IDENTIFY: {}", e))?;

    let response_text = response
        .into_text()
        .map_err(|e| eyre!("Response not text: {}", e))?;
    let response_payload: serde_json::Value =
        serde_json::from_str(&response_text).map_err(|e| eyre!("Response parse error: {}", e))?;
    let response_event = event::parse_gateway_payload(&response_payload);

    // Close the connection
    let _ = write.send(Message::Close(None)).await;

    match response_event {
        GatewayEvent::Ready(_) => Ok(true),
        _ => Ok(false),
    }
}

// === Task 40: QR Code Authentication ===

/// QR code authentication session. Generates RSA keypair and computes fingerprint.
pub struct QrAuthSession {
    private_key: rsa::RsaPrivateKey,
    public_key_der: Vec<u8>,
    pub fingerprint: String,
}

impl QrAuthSession {
    /// Create a new QR auth session with a fresh RSA-2048 keypair.
    pub fn new() -> Result<Self> {
        use rsa::pkcs8::EncodePublicKey;
        use sha2::{Digest, Sha256};

        let mut rng = rand::thread_rng();
        let private_key = rsa::RsaPrivateKey::new(&mut rng, 2048)
            .map_err(|e| eyre!("RSA key generation failed: {}", e))?;
        let public_key = private_key.to_public_key();

        let public_key_der = public_key
            .to_public_key_der()
            .map_err(|e| eyre!("Public key DER encoding failed: {}", e))?
            .to_vec();

        // Fingerprint = SHA-256 of public key DER, base64 encoded
        let mut hasher = Sha256::new();
        hasher.update(&public_key_der);
        let hash = hasher.finalize();
        let fingerprint = base64::engine::general_purpose::STANDARD.encode(hash);

        Ok(Self {
            private_key,
            public_key_der,
            fingerprint,
        })
    }

    /// Get the base64-encoded public key for the "init" WebSocket message.
    pub fn encoded_public_key(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.public_key_der)
    }

    /// Get the QR code URL that the user scans with Discord mobile.
    pub fn qr_url(&self) -> String {
        format!("https://discord.com/ra/{}", self.fingerprint)
    }

    /// Decrypt an RSA-OAEP encrypted payload (base64 encoded).
    pub fn decrypt_payload(&self, encrypted_b64: &str) -> Result<String> {
        let encrypted = base64::engine::general_purpose::STANDARD
            .decode(encrypted_b64)
            .map_err(|e| eyre!("Base64 decode failed: {}", e))?;
        let padding = rsa::Oaep::new::<sha2::Sha256>();
        let decrypted = self
            .private_key
            .decrypt(padding, &encrypted)
            .map_err(|e| eyre!("RSA decryption failed: {}", e))?;
        String::from_utf8(decrypted).map_err(|e| eyre!("Decrypted payload is not UTF-8: {}", e))
    }

    /// Compute the nonce proof: decrypt the nonce, SHA-256 hash it, base64 encode.
    pub fn compute_nonce_proof(&self, encrypted_nonce_b64: &str) -> Result<String> {
        use sha2::{Digest, Sha256};

        let encrypted = base64::engine::general_purpose::STANDARD
            .decode(encrypted_nonce_b64)
            .map_err(|e| eyre!("Base64 decode failed: {}", e))?;
        let padding = rsa::Oaep::new::<sha2::Sha256>();
        let decrypted = self
            .private_key
            .decrypt(padding, &encrypted)
            .map_err(|e| eyre!("RSA decryption failed: {}", e))?;

        let mut hasher = Sha256::new();
        hasher.update(&decrypted);
        let hash = hasher.finalize();
        Ok(base64::engine::general_purpose::STANDARD.encode(hash))
    }

    /// Generate QR code lines for terminal rendering.
    /// Returns an error if QR code generation fails (e.g., URL too long).
    pub fn generate_qr_lines(&self) -> Result<Vec<String>> {
        let url = self.qr_url();
        let code = qrcode::QrCode::new(url.as_bytes())
            .map_err(|e| color_eyre::eyre::eyre!("QR code generation failed: {}", e))?;
        let image = code
            .render::<char>()
            .quiet_zone(true)
            .module_dimensions(2, 1)
            .build();
        Ok(image
            .lines()
            .map(std::string::ToString::to_string)
            .collect())
    }
}

/// Build the QR auth "init" WebSocket message.
pub fn build_qr_auth_init(encoded_public_key: &str) -> serde_json::Value {
    serde_json::json!({
        "op": "init",
        "encoded_public_key": encoded_public_key,
    })
}

/// Build the QR auth "`nonce_proof`" WebSocket message.
pub fn build_qr_auth_nonce_proof(proof: &str) -> serde_json::Value {
    serde_json::json!({
        "op": "nonce_proof",
        "proof": proof,
    })
}

/// Build the QR auth heartbeat message.
pub fn build_qr_auth_heartbeat() -> serde_json::Value {
    serde_json::json!({
        "op": "heartbeat",
    })
}

/// Parsed QR auth protocol message from the server.
#[derive(Debug, Clone, PartialEq)]
pub enum QrAuthMessage {
    Hello {
        heartbeat_interval: u64,
        timeout_ms: u64,
    },
    NonceProof {
        encrypted_nonce: String,
    },
    PendingRemoteInit {
        fingerprint: String,
    },
    PendingTicket {
        encrypted_user_payload: String,
    },
    PendingLogin {
        ticket: String,
    },
    Cancel,
    Unknown {
        op: String,
    },
}

/// Parse a QR auth WebSocket message from the server.
pub fn parse_qr_auth_message(payload: &serde_json::Value) -> QrAuthMessage {
    let op = payload["op"].as_str().unwrap_or("");
    match op {
        "hello" => QrAuthMessage::Hello {
            heartbeat_interval: payload["heartbeat_interval"].as_u64().unwrap_or(0),
            timeout_ms: payload["timeout_ms"].as_u64().unwrap_or(0),
        },
        "nonce_proof" => QrAuthMessage::NonceProof {
            encrypted_nonce: payload["encrypted_nonce"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        },
        "pending_remote_init" => QrAuthMessage::PendingRemoteInit {
            fingerprint: payload["fingerprint"].as_str().unwrap_or("").to_string(),
        },
        "pending_ticket" => QrAuthMessage::PendingTicket {
            encrypted_user_payload: payload["encrypted_user_payload"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        },
        "pending_login" => QrAuthMessage::PendingLogin {
            ticket: payload["ticket"].as_str().unwrap_or("").to_string(),
        },
        "cancel" => QrAuthMessage::Cancel,
        _ => QrAuthMessage::Unknown { op: op.to_string() },
    }
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

    fn test_discord_config() -> DiscordConfig {
        DiscordConfig {
            client_build_number: 346892,
            browser_version: "131.0.0.0".to_string(),
            browser_user_agent: "Mozilla/5.0 Test".to_string(),
        }
    }

    // === Token retrieval tests (Task 11) ===

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
        assert_eq!(token.unwrap().expose_secret(), "env_token");
    }

    #[test]
    fn keyring_is_second_priority() {
        let config = default_auth_config();
        let keyring = MemoryTokenStore::with_token("keyring_token");

        let token = retrieve_token(&config, &keyring, &no_env).unwrap();
        assert_eq!(token.unwrap().expose_secret(), "keyring_token");
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
        assert_eq!(token.unwrap().expose_secret(), "file_token");
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
        assert_eq!(token.unwrap().expose_secret(), "keyring_token");
    }

    #[test]
    fn keyring_not_checked_when_source_is_file() {
        let config = AuthConfig {
            token_source: "file".to_string(),
            token_file: None,
        };
        let keyring = MemoryTokenStore::with_token("keyring_token");

        let token = retrieve_token(&config, &keyring, &no_env).unwrap();
        assert!(token.is_none());
    }

    #[test]
    fn store_token_in_keyring() {
        let keyring = MemoryTokenStore::new();

        store_token(&keyring, "new_token_123").unwrap();
        assert_eq!(
            keyring.get_token().unwrap().unwrap().expose_secret(),
            "new_token_123"
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
        assert_eq!(token.unwrap().expose_secret(), "trimmed_token");
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
        assert_eq!(token.unwrap().expose_secret(), "env_token");

        // Without env var → keyring wins
        let token = retrieve_token(&config, &keyring, &no_env).unwrap();
        assert_eq!(token.unwrap().expose_secret(), "keyring_token");

        // Without env var or keyring → file wins
        let empty_keyring = MemoryTokenStore::new();
        let token = retrieve_token(&config, &empty_keyring, &no_env).unwrap();
        assert_eq!(token.unwrap().expose_secret(), "file_token");
    }

    // === Task 39: Email + Password Login Tests ===

    /// Helper to start a mock HTTP server that returns a given status and body.
    async fn start_mock_http(status_code: u16, body: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);

        let response_body = body.to_string();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let _ = socket.read(&mut buf).await.unwrap();

            let status_text = match status_code {
                200 => "OK",
                400 => "Bad Request",
                401 => "Unauthorized",
                _ => "Error",
            };

            let response = format!(
                "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                status_code,
                status_text,
                response_body.len(),
                response_body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });

        base_url
    }

    #[tokio::test]
    async fn login_with_valid_credentials_returns_token() {
        let body = r#"{"token": "mfa.valid_token_123"}"#;
        let base_url = start_mock_http(200, body).await;
        let config = test_discord_config();

        let result = login_with_credentials("user@example.com", "password123", &config, &base_url)
            .await
            .unwrap();
        assert_eq!(
            result,
            LoginResponse::Token("mfa.valid_token_123".to_string())
        );
    }

    #[tokio::test]
    async fn login_with_invalid_credentials_returns_error() {
        let body = r#"{"message": "Invalid login credentials", "code": 50035}"#;
        let base_url = start_mock_http(400, body).await;
        let config = test_discord_config();

        let result =
            login_with_credentials("user@example.com", "wrongpass", &config, &base_url).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid login credentials"), "Error: {}", err);
    }

    #[tokio::test]
    async fn login_with_2fa_required_returns_mfa_ticket() {
        let body = r#"{"token": null, "mfa": ["totp"], "ticket": "mfa_ticket_abc123"}"#;
        let base_url = start_mock_http(200, body).await;
        let config = test_discord_config();

        let result = login_with_credentials("user@example.com", "password123", &config, &base_url)
            .await
            .unwrap();
        assert_eq!(
            result,
            LoginResponse::MfaRequired {
                ticket: "mfa_ticket_abc123".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn submit_mfa_totp_with_valid_code_returns_token() {
        let body = r#"{"token": "mfa.authenticated_token_456"}"#;
        let base_url = start_mock_http(200, body).await;
        let config = test_discord_config();

        let token = submit_mfa_totp("mfa_ticket_abc", "123456", &config, &base_url)
            .await
            .unwrap();
        assert_eq!(token, "mfa.authenticated_token_456");
    }

    #[tokio::test]
    async fn submit_mfa_totp_with_invalid_code_returns_error() {
        let body = r#"{"message": "Invalid two-factor code", "code": 60008}"#;
        let base_url = start_mock_http(400, body).await;
        let config = test_discord_config();

        let result = submit_mfa_totp("mfa_ticket_abc", "000000", &config, &base_url).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid two-factor code"), "Error: {}", err);
    }

    #[tokio::test]
    async fn login_uses_anti_detection_headers() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);

        let (captured_tx, mut captured_rx) = tokio::sync::mpsc::channel::<String>(1);
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = socket.read(&mut buf).await.unwrap();
            let request_text = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = captured_tx.send(request_text).await;

            let body = r#"{"token": "test"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });

        let config = test_discord_config();
        let _ = login_with_credentials("user@test.com", "pass", &config, &base_url).await;

        let request = captured_rx.recv().await.unwrap();
        // reqwest/hyper normalizes header names to lowercase
        let request_lower = request.to_lowercase();
        assert!(
            request_lower.contains("x-super-properties"),
            "Request must include X-Super-Properties header: {}",
            request
        );
        assert!(
            request_lower.contains("x-discord-locale"),
            "Request must include X-Discord-Locale header: {}",
            request
        );
        assert!(
            request_lower.contains("user-agent"),
            "Request must include User-Agent header: {}",
            request
        );
    }

    #[tokio::test]
    async fn login_sends_correct_body() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);

        let (captured_tx, mut captured_rx) = tokio::sync::mpsc::channel::<String>(1);
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = socket.read(&mut buf).await.unwrap();
            let request_text = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = captured_tx.send(request_text).await;

            let body = r#"{"token": "test"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });

        let config = test_discord_config();
        let _ = login_with_credentials("test@example.com", "mypassword", &config, &base_url).await;

        let request = captured_rx.recv().await.unwrap();
        // Find the body (after \r\n\r\n)
        if let Some(body_start) = request.find("\r\n\r\n") {
            let body = &request[body_start + 4..];
            let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
            assert_eq!(parsed["login"], "test@example.com");
            assert_eq!(parsed["password"], "mypassword");
            assert_eq!(parsed["undelete"], false);
        }
    }

    // === Task 40: QR Code Authentication Tests ===

    #[test]
    fn qr_auth_session_generates_keypair() {
        let session = QrAuthSession::new().unwrap();
        assert!(!session.encoded_public_key().is_empty());
        assert!(!session.fingerprint.is_empty());
    }

    #[test]
    fn qr_auth_session_fingerprint_is_base64() {
        let session = QrAuthSession::new().unwrap();
        // Fingerprint should be valid base64
        let decoded = base64::engine::general_purpose::STANDARD.decode(&session.fingerprint);
        assert!(decoded.is_ok(), "Fingerprint should be valid base64");
        // SHA-256 produces 32 bytes
        assert_eq!(decoded.unwrap().len(), 32);
    }

    #[test]
    fn qr_auth_session_qr_url_format() {
        let session = QrAuthSession::new().unwrap();
        let url = session.qr_url();
        assert!(url.starts_with("https://discord.com/ra/"));
        assert!(url.len() > "https://discord.com/ra/".len());
    }

    #[test]
    fn qr_auth_session_encrypt_decrypt_roundtrip() {
        let session = QrAuthSession::new().unwrap();
        let public_key = session.private_key.to_public_key();

        // Encrypt a test payload with the public key
        let plaintext = b"test_token_abc123";
        let padding = rsa::Oaep::new::<sha2::Sha256>();
        let mut rng = rand::thread_rng();
        let encrypted = rsa::RsaPublicKey::encrypt(&public_key, &mut rng, padding, plaintext)
            .expect("Encryption failed");

        let encrypted_b64 = base64::engine::general_purpose::STANDARD.encode(&encrypted);

        // Decrypt with the session
        let decrypted = session.decrypt_payload(&encrypted_b64).unwrap();
        assert_eq!(decrypted, "test_token_abc123");
    }

    #[test]
    fn qr_auth_session_nonce_proof_computation() {
        let session = QrAuthSession::new().unwrap();
        let public_key = session.private_key.to_public_key();

        // Encrypt a test nonce
        let nonce = b"test_nonce_value";
        let padding = rsa::Oaep::new::<sha2::Sha256>();
        let mut rng = rand::thread_rng();
        let encrypted = rsa::RsaPublicKey::encrypt(&public_key, &mut rng, padding, nonce)
            .expect("Encryption failed");
        let encrypted_b64 = base64::engine::general_purpose::STANDARD.encode(&encrypted);

        let proof = session.compute_nonce_proof(&encrypted_b64).unwrap();

        // Proof should be base64-encoded SHA-256 of the nonce
        let decoded_proof = base64::engine::general_purpose::STANDARD
            .decode(&proof)
            .unwrap();
        assert_eq!(decoded_proof.len(), 32, "SHA-256 hash should be 32 bytes");

        // Verify it matches manual SHA-256 of the nonce
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(nonce);
        let expected_hash = hasher.finalize();
        assert_eq!(decoded_proof, expected_hash.as_slice());
    }

    #[test]
    fn qr_auth_generates_qr_lines() {
        let session = QrAuthSession::new().unwrap();
        let lines = session.generate_qr_lines().unwrap();

        // QR code should produce multiple lines
        assert!(!lines.is_empty(), "QR code should produce lines");
        assert!(lines.len() > 5, "QR code should have multiple rows");

        // All lines should have consistent width
        let widths: Vec<usize> = lines.iter().map(|l| l.chars().count()).collect();
        let first_width = widths[0];
        for (i, w) in widths.iter().enumerate() {
            assert_eq!(
                *w, first_width,
                "Line {} has width {} but first line has {}",
                i, w, first_width
            );
        }
    }

    #[test]
    fn qr_auth_encoded_public_key_is_valid_base64() {
        let session = QrAuthSession::new().unwrap();
        let encoded = session.encoded_public_key();
        let decoded = base64::engine::general_purpose::STANDARD.decode(&encoded);
        assert!(decoded.is_ok(), "Encoded public key should be valid base64");
        // RSA-2048 SPKI DER is typically ~294 bytes
        assert!(decoded.unwrap().len() > 200);
    }

    #[test]
    fn qr_auth_two_sessions_have_different_keys() {
        let session1 = QrAuthSession::new().unwrap();
        let session2 = QrAuthSession::new().unwrap();
        assert_ne!(
            session1.fingerprint, session2.fingerprint,
            "Different sessions should have different fingerprints"
        );
    }

    #[test]
    fn build_qr_auth_init_message() {
        let msg = build_qr_auth_init("test_key_base64");
        assert_eq!(msg["op"], "init");
        assert_eq!(msg["encoded_public_key"], "test_key_base64");
    }

    #[test]
    fn build_qr_auth_nonce_proof_message() {
        let msg = build_qr_auth_nonce_proof("proof_base64");
        assert_eq!(msg["op"], "nonce_proof");
        assert_eq!(msg["proof"], "proof_base64");
    }

    #[test]
    fn build_qr_auth_heartbeat_message() {
        let msg = build_qr_auth_heartbeat();
        assert_eq!(msg["op"], "heartbeat");
    }

    #[test]
    fn parse_qr_auth_hello() {
        let payload = serde_json::json!({
            "op": "hello",
            "heartbeat_interval": 41250,
            "timeout_ms": 120000
        });
        let msg = parse_qr_auth_message(&payload);
        assert_eq!(
            msg,
            QrAuthMessage::Hello {
                heartbeat_interval: 41250,
                timeout_ms: 120000,
            }
        );
    }

    #[test]
    fn parse_qr_auth_nonce_proof() {
        let payload = serde_json::json!({
            "op": "nonce_proof",
            "encrypted_nonce": "encrypted_nonce_base64"
        });
        let msg = parse_qr_auth_message(&payload);
        assert_eq!(
            msg,
            QrAuthMessage::NonceProof {
                encrypted_nonce: "encrypted_nonce_base64".to_string(),
            }
        );
    }

    #[test]
    fn parse_qr_auth_pending_remote_init() {
        let payload = serde_json::json!({
            "op": "pending_remote_init",
            "fingerprint": "fp_abc123"
        });
        let msg = parse_qr_auth_message(&payload);
        assert_eq!(
            msg,
            QrAuthMessage::PendingRemoteInit {
                fingerprint: "fp_abc123".to_string(),
            }
        );
    }

    #[test]
    fn parse_qr_auth_pending_login() {
        let payload = serde_json::json!({
            "op": "pending_login",
            "ticket": "ticket_xyz789"
        });
        let msg = parse_qr_auth_message(&payload);
        assert_eq!(
            msg,
            QrAuthMessage::PendingLogin {
                ticket: "ticket_xyz789".to_string(),
            }
        );
    }

    #[test]
    fn parse_qr_auth_cancel() {
        let payload = serde_json::json!({"op": "cancel"});
        let msg = parse_qr_auth_message(&payload);
        assert_eq!(msg, QrAuthMessage::Cancel);
    }

    #[test]
    fn parse_qr_auth_unknown_op() {
        let payload = serde_json::json!({"op": "something_new"});
        let msg = parse_qr_auth_message(&payload);
        assert_eq!(
            msg,
            QrAuthMessage::Unknown {
                op: "something_new".to_string(),
            }
        );
    }
}
