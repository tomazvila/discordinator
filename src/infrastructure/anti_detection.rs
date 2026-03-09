use base64::Engine;
use rquest::header::{HeaderMap, HeaderValue};
use rquest_util::Emulation;

use crate::config::DiscordConfig;

/// IDENTIFY properties that mimic the Discord web client.
/// These are sent in the gateway IDENTIFY (op 2) payload.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentifyProperties {
    pub os: String,
    pub browser: String,
    pub device: String,
    pub system_locale: String,
    pub browser_user_agent: String,
    pub browser_version: String,
    pub os_version: String,
    pub referrer: String,
    pub referring_domain: String,
    pub referrer_current: String,
    pub referring_domain_current: String,
    pub release_channel: String,
    pub client_build_number: u64,
    pub client_event_source: Option<String>,
}

/// Infer the OS identity from the configured user-agent string.
/// This ensures `Sec-Ch-Ua-Platform`, `X-Super-Properties` OS fields, and `User-Agent`
/// are all consistent — a mismatch is a detection signal.
fn detect_os_from_ua(user_agent: &str) -> (&'static str, &'static str, &'static str) {
    let ua_lower = user_agent.to_lowercase();
    if ua_lower.contains("windows") {
        ("Windows", "10", "\"Windows\"")
    } else if ua_lower.contains("linux") && !ua_lower.contains("android") {
        ("Linux", "x86_64", "\"Linux\"")
    } else {
        // Default to macOS (matches the default UA which says "Macintosh")
        ("Mac OS X", "10.15.7", "\"macOS\"")
    }
}

/// Build IDENTIFY properties from the Discord config section.
/// These properties must match the official web client to avoid detection.
pub fn build_identify_properties(config: &DiscordConfig) -> IdentifyProperties {
    let (os_name, os_version, _) = detect_os_from_ua(&config.browser_user_agent);
    IdentifyProperties {
        os: os_name.to_string(),
        browser: "Chrome".to_string(),
        device: String::new(),
        system_locale: "en-US".to_string(),
        browser_user_agent: config.browser_user_agent.clone(),
        browser_version: config.browser_version.clone(),
        os_version: os_version.to_string(),
        referrer: String::new(),
        referring_domain: String::new(),
        referrer_current: String::new(),
        referring_domain_current: String::new(),
        release_channel: "stable".to_string(),
        client_build_number: config.client_build_number,
        client_event_source: None,
    }
}

/// Build X-Super-Properties header value: base64-encoded JSON of IDENTIFY properties.
pub fn build_super_properties(config: &DiscordConfig) -> String {
    let properties = build_identify_properties(config);
    // serde_json::to_string only fails if the Serialize impl is broken (unreachable for plain structs)
    let json = serde_json::to_string(&properties).unwrap_or_default();
    base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
}

/// Build HTTP headers that mimic the Discord web client.
/// These must be included on ALL REST API requests.
///
/// Returns an error if the config contains values that can't be encoded
/// as HTTP header values (e.g., non-visible ASCII characters).
pub fn build_http_headers(
    config: &DiscordConfig,
    token: &str,
) -> Result<HeaderMap, rquest::header::InvalidHeaderValue> {
    let mut headers = HeaderMap::new();

    headers.insert(
        "User-Agent",
        HeaderValue::from_str(&config.browser_user_agent)?,
    );

    let super_props = build_super_properties(config);
    headers.insert("X-Super-Properties", HeaderValue::from_str(&super_props)?);

    headers.insert("X-Discord-Locale", HeaderValue::from_static("en-US"));

    // User token without "Bot " prefix — this is critical for user accounts
    headers.insert("Authorization", HeaderValue::from_str(token)?);

    // Chrome Client Hints headers — order and brand string must match real Chrome output.
    // Chrome 120+ uses "Not_A Brand" with varying punctuation per version; 131 uses "Not(A:Brand".
    let major_version = config.browser_version.split('.').next().unwrap_or("131");
    let sec_ch_ua = format!(
        "\"Chromium\";v=\"{major_version}\", \"Not(A:Brand\";v=\"24\", \"Google Chrome\";v=\"{major_version}\""
    );
    headers.insert("Sec-Ch-Ua", HeaderValue::from_str(&sec_ch_ua)?);
    let (_, _, platform) = detect_os_from_ua(&config.browser_user_agent);
    headers.insert("Sec-Ch-Ua-Mobile", HeaderValue::from_static("?0"));
    headers.insert("Sec-Ch-Ua-Platform", HeaderValue::from_static(platform));

    // Fetch metadata headers
    headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("empty"));
    headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("cors"));
    headers.insert("Sec-Fetch-Site", HeaderValue::from_static("same-origin"));

    // Standard browser headers
    headers.insert("Origin", HeaderValue::from_static("https://discord.com"));
    headers.insert(
        "Referer",
        HeaderValue::from_static("https://discord.com/channels/@me"),
    );
    headers.insert("Accept", HeaderValue::from_static("*/*"));
    headers.insert(
        "Accept-Language",
        HeaderValue::from_static("en-US,en;q=0.9"),
    );

    // Discord-specific headers
    headers.insert(
        "X-Discord-Timezone",
        HeaderValue::from_str(&system_timezone())?,
    );
    headers.insert(
        "X-Debug-Options",
        HeaderValue::from_static("bugReporterEnabled"),
    );

    Ok(headers)
}

/// Get the system timezone as an IANA string (e.g., `America/New_York`).
fn system_timezone() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "America/New_York".to_string())
}

/// Build an HTTP client that impersonates Chrome's TLS fingerprint.
/// Uses `BoringSSL` to produce Chrome-matching JA3/JA4 fingerprints.
pub fn build_chrome_client(headers: HeaderMap) -> Result<rquest::Client, rquest::Error> {
    rquest::Client::builder()
        .emulation(Emulation::Chrome131)
        .default_headers(headers)
        .cookie_store(true)
        .build()
}

/// Build an unauthenticated HTTP client with Chrome TLS impersonation.
/// Used for pre-auth requests (login, property fetching).
pub fn build_chrome_client_simple(user_agent: &str) -> Result<rquest::Client, rquest::Error> {
    rquest::Client::builder()
        .emulation(Emulation::Chrome131)
        .user_agent(user_agent)
        .timeout(std::time::Duration::from_secs(30))
        .build()
}

/// Build a WebSocket upgrade request with browser-like headers.
/// Used for all gateway and auth WebSocket connections.
pub fn build_ws_request(
    url: &str,
    config: &DiscordConfig,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, color_eyre::eyre::Report> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    // Start from the URL-based request (which auto-sets Host, Connection, Upgrade,
    // Sec-WebSocket-Key, Sec-WebSocket-Version headers)
    let mut request = url.into_client_request()?;
    let headers = request.headers_mut();
    headers.insert("Origin", "https://discord.com".parse()?);
    headers.insert("User-Agent", config.browser_user_agent.parse()?);
    headers.insert("Accept-Language", "en-US,en;q=0.9".parse()?);
    headers.insert("Cache-Control", "no-cache".parse()?);
    headers.insert("Pragma", "no-cache".parse()?);
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> DiscordConfig {
        DiscordConfig {
            client_build_number: 346892,
            browser_version: "131.0.0.0".to_string(),
            browser_user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn identify_properties_match_web_client_format() {
        let config = test_config();
        let props = build_identify_properties(&config);

        let (expected_os, expected_os_version, _) = detect_os_from_ua(&config.browser_user_agent);
        assert_eq!(props.os, expected_os);
        assert_eq!(props.browser, "Chrome");
        assert_eq!(props.device, "");
        assert_eq!(props.system_locale, "en-US");
        assert_eq!(props.browser_user_agent, config.browser_user_agent);
        assert_eq!(props.browser_version, "131.0.0.0");
        assert_eq!(props.os_version, expected_os_version);
        assert_eq!(props.referrer, "");
        assert_eq!(props.referring_domain, "");
        assert_eq!(props.referrer_current, "");
        assert_eq!(props.referring_domain_current, "");
        assert_eq!(props.release_channel, "stable");
        assert_eq!(props.client_build_number, 346892);
    }

    #[test]
    fn identify_properties_serializes_to_valid_json() {
        let config = test_config();
        let props = build_identify_properties(&config);
        let json = serde_json::to_value(&props).unwrap();

        // All expected fields must be present (camelCase keys)
        assert_eq!(json["os"], "Mac OS X");
        assert_eq!(json["browser"], "Chrome");
        assert_eq!(json["device"], "");
        assert_eq!(json["systemLocale"], "en-US");
        assert_eq!(json["browserVersion"], "131.0.0.0");
        assert_eq!(json["osVersion"], "10.15.7");
        assert_eq!(json["releaseChannel"], "stable");
        assert_eq!(json["clientBuildNumber"], 346892);

        // Verify it's a JSON object with all 14 fields
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 14);
    }

    #[test]
    fn identify_properties_uses_camel_case_field_names() {
        let config = test_config();
        let props = build_identify_properties(&config);
        let json = serde_json::to_value(&props).unwrap();
        let obj = json.as_object().unwrap();

        // Discord web client sends camelCase field names in X-Super-Properties
        assert!(
            obj.contains_key("systemLocale"),
            "expected camelCase 'systemLocale'"
        );
        assert!(
            obj.contains_key("browserUserAgent"),
            "expected camelCase 'browserUserAgent'"
        );
        assert!(
            obj.contains_key("browserVersion"),
            "expected camelCase 'browserVersion'"
        );
        assert!(
            obj.contains_key("osVersion"),
            "expected camelCase 'osVersion'"
        );
        assert!(
            obj.contains_key("referringDomain"),
            "expected camelCase 'referringDomain'"
        );
        assert!(
            obj.contains_key("referrerCurrent"),
            "expected camelCase 'referrerCurrent'"
        );
        assert!(
            obj.contains_key("referringDomainCurrent"),
            "expected camelCase 'referringDomainCurrent'"
        );
        assert!(
            obj.contains_key("releaseChannel"),
            "expected camelCase 'releaseChannel'"
        );
        assert!(
            obj.contains_key("clientBuildNumber"),
            "expected camelCase 'clientBuildNumber'"
        );
        assert!(
            obj.contains_key("clientEventSource"),
            "expected camelCase 'clientEventSource'"
        );

        // Must NOT have snake_case versions
        assert!(
            !obj.contains_key("system_locale"),
            "should not have snake_case 'system_locale'"
        );
        assert!(
            !obj.contains_key("browser_user_agent"),
            "should not have snake_case 'browser_user_agent'"
        );
        assert!(
            !obj.contains_key("client_build_number"),
            "should not have snake_case 'client_build_number'"
        );
    }

    #[test]
    fn identify_properties_uses_config_values() {
        let config = DiscordConfig {
            client_build_number: 999999,
            browser_version: "200.0.0.0".to_string(),
            browser_user_agent: "CustomAgent/1.0".to_string(),
            ..Default::default()
        };
        let props = build_identify_properties(&config);

        assert_eq!(props.client_build_number, 999999);
        assert_eq!(props.browser_version, "200.0.0.0");
        assert_eq!(props.browser_user_agent, "CustomAgent/1.0");
    }

    #[test]
    fn super_properties_is_valid_base64_encoded_json() {
        let config = test_config();
        let encoded = build_super_properties(&config);

        // Must be valid base64
        let decoded_bytes = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .expect("X-Super-Properties must be valid base64");

        // Must decode to valid JSON
        let json_str = String::from_utf8(decoded_bytes).expect("Must be valid UTF-8");
        let json: serde_json::Value = serde_json::from_str(&json_str).expect("Must be valid JSON");

        // Must contain the expected fields (camelCase)
        assert_eq!(json["os"], "Mac OS X");
        assert_eq!(json["browser"], "Chrome");
        assert_eq!(json["clientBuildNumber"], 346892);
        assert_eq!(json["browserVersion"], "131.0.0.0");
    }

    #[test]
    fn super_properties_roundtrip_matches_identify_properties() {
        let config = test_config();
        let props = build_identify_properties(&config);
        let encoded = build_super_properties(&config);

        let decoded_bytes = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .unwrap();
        let decoded_json: serde_json::Value =
            serde_json::from_str(&String::from_utf8(decoded_bytes).unwrap()).unwrap();
        let props_json = serde_json::to_value(&props).unwrap();

        assert_eq!(decoded_json, props_json);
    }

    #[test]
    fn http_headers_are_complete() {
        let config = test_config();
        let token = "mfa.test_token_value";
        let headers = build_http_headers(&config, token).unwrap();

        // Core anti-detection headers
        assert!(headers.contains_key("User-Agent"));
        assert!(headers.contains_key("X-Super-Properties"));
        assert!(headers.contains_key("X-Discord-Locale"));
        assert!(headers.contains_key("Authorization"));

        // Browser-like headers that Chrome sends on every request
        assert!(headers.contains_key("Sec-Ch-Ua"), "missing Sec-Ch-Ua");
        assert!(
            headers.contains_key("Sec-Ch-Ua-Mobile"),
            "missing Sec-Ch-Ua-Mobile"
        );
        assert!(
            headers.contains_key("Sec-Ch-Ua-Platform"),
            "missing Sec-Ch-Ua-Platform"
        );
        assert!(
            headers.contains_key("Sec-Fetch-Dest"),
            "missing Sec-Fetch-Dest"
        );
        assert!(
            headers.contains_key("Sec-Fetch-Mode"),
            "missing Sec-Fetch-Mode"
        );
        assert!(
            headers.contains_key("Sec-Fetch-Site"),
            "missing Sec-Fetch-Site"
        );
        assert!(headers.contains_key("Origin"), "missing Origin");
        assert!(headers.contains_key("Referer"), "missing Referer");
        assert!(headers.contains_key("Accept"), "missing Accept");
        assert!(
            headers.contains_key("Accept-Language"),
            "missing Accept-Language"
        );
        assert!(
            headers.contains_key("X-Discord-Timezone"),
            "missing X-Discord-Timezone"
        );
        assert!(
            headers.contains_key("X-Debug-Options"),
            "missing X-Debug-Options"
        );
    }

    #[test]
    fn http_headers_browser_hints_match_chrome() {
        let config = test_config();
        let headers = build_http_headers(&config, "token").unwrap();

        let sec_ch_ua = headers.get("Sec-Ch-Ua").unwrap().to_str().unwrap();
        assert!(
            sec_ch_ua.contains("Google Chrome"),
            "Sec-Ch-Ua should mention Google Chrome"
        );
        assert!(
            sec_ch_ua.contains("131"),
            "Sec-Ch-Ua should contain browser version"
        );

        assert_eq!(
            headers.get("Sec-Ch-Ua-Mobile").unwrap().to_str().unwrap(),
            "?0"
        );
        assert_eq!(
            headers.get("Sec-Ch-Ua-Platform").unwrap().to_str().unwrap(),
            "\"macOS\""
        );
        assert_eq!(
            headers.get("Sec-Fetch-Dest").unwrap().to_str().unwrap(),
            "empty"
        );
        assert_eq!(
            headers.get("Sec-Fetch-Mode").unwrap().to_str().unwrap(),
            "cors"
        );
        assert_eq!(
            headers.get("Sec-Fetch-Site").unwrap().to_str().unwrap(),
            "same-origin"
        );
        assert_eq!(
            headers.get("Origin").unwrap().to_str().unwrap(),
            "https://discord.com"
        );
        assert_eq!(
            headers.get("X-Debug-Options").unwrap().to_str().unwrap(),
            "bugReporterEnabled"
        );
    }

    #[test]
    fn http_headers_user_agent_matches_config() {
        let config = test_config();
        let headers = build_http_headers(&config, "token").unwrap();

        let ua = headers.get("User-Agent").unwrap().to_str().unwrap();
        assert_eq!(ua, config.browser_user_agent);
    }

    #[test]
    fn http_headers_authorization_has_no_bot_prefix() {
        let config = test_config();
        let token = "mfa.test_token_value";
        let headers = build_http_headers(&config, token).unwrap();

        let auth = headers.get("Authorization").unwrap().to_str().unwrap();
        assert_eq!(auth, "mfa.test_token_value");
        assert!(!auth.starts_with("Bot "));
        assert!(!auth.starts_with("Bearer "));
    }

    #[test]
    fn http_headers_discord_locale_is_set() {
        let config = test_config();
        let headers = build_http_headers(&config, "token").unwrap();

        let locale = headers.get("X-Discord-Locale").unwrap().to_str().unwrap();
        assert_eq!(locale, "en-US");
    }

    #[test]
    fn ws_request_has_browser_headers() {
        let config = test_config();
        let req = build_ws_request("wss://gateway.discord.gg/?v=10", &config).unwrap();
        let headers = req.headers();

        assert_eq!(
            headers.get("Origin").unwrap().to_str().unwrap(),
            "https://discord.com"
        );
        assert_eq!(
            headers.get("User-Agent").unwrap().to_str().unwrap(),
            config.browser_user_agent
        );
        assert_eq!(
            headers.get("Accept-Language").unwrap().to_str().unwrap(),
            "en-US,en;q=0.9"
        );
    }

    #[test]
    fn http_headers_super_properties_is_valid_base64() {
        let config = test_config();
        let headers = build_http_headers(&config, "token").unwrap();

        let super_props = headers.get("X-Super-Properties").unwrap().to_str().unwrap();

        // Must decode as base64 → JSON
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(super_props)
            .expect("X-Super-Properties header must be valid base64");
        let json: serde_json::Value =
            serde_json::from_str(&String::from_utf8(decoded).unwrap()).unwrap();
        assert_eq!(json["browser"], "Chrome");
    }

    #[test]
    fn chrome_client_builds_with_emulation() {
        let config = test_config();
        let headers = build_http_headers(&config, "token").unwrap();
        let client = build_chrome_client(headers);
        assert!(
            client.is_ok(),
            "Chrome-impersonating client should build successfully"
        );
    }

    #[test]
    fn chrome_client_simple_builds() {
        let client = build_chrome_client_simple("Mozilla/5.0 Test");
        assert!(
            client.is_ok(),
            "Simple Chrome client should build successfully"
        );
    }
}
