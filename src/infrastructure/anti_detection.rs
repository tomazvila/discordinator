use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue};

use crate::config::DiscordConfig;

/// IDENTIFY properties that mimic the Discord web client.
/// These are sent in the gateway IDENTIFY (op 2) payload.
#[derive(Debug, Clone, serde::Serialize)]
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
}

/// Build IDENTIFY properties from the Discord config section.
/// These properties must match the official web client to avoid detection.
pub fn build_identify_properties(config: &DiscordConfig) -> IdentifyProperties {
    IdentifyProperties {
        os: "Mac OS X".to_string(),
        browser: "Chrome".to_string(),
        device: String::new(),
        system_locale: "en-US".to_string(),
        browser_user_agent: config.browser_user_agent.clone(),
        browser_version: config.browser_version.clone(),
        os_version: "10.15.7".to_string(),
        referrer: String::new(),
        referring_domain: String::new(),
        referrer_current: String::new(),
        referring_domain_current: String::new(),
        release_channel: "stable".to_string(),
        client_build_number: config.client_build_number,
    }
}

/// Build X-Super-Properties header value: base64-encoded JSON of IDENTIFY properties.
pub fn build_super_properties(config: &DiscordConfig) -> String {
    let properties = build_identify_properties(config);
    let json = serde_json::to_string(&properties).expect("Failed to serialize identify properties");
    base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
}

/// Build HTTP headers that mimic the Discord web client.
/// These must be included on ALL REST API requests.
pub fn build_http_headers(config: &DiscordConfig, token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();

    headers.insert(
        "User-Agent",
        HeaderValue::from_str(&config.browser_user_agent)
            .expect("Invalid User-Agent header value"),
    );

    let super_props = build_super_properties(config);
    headers.insert(
        "X-Super-Properties",
        HeaderValue::from_str(&super_props).expect("Invalid X-Super-Properties header value"),
    );

    headers.insert(
        "X-Discord-Locale",
        HeaderValue::from_static("en-US"),
    );

    // User token without "Bot " prefix — this is critical for user accounts
    headers.insert(
        "Authorization",
        HeaderValue::from_str(token).expect("Invalid Authorization header value"),
    );

    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> DiscordConfig {
        DiscordConfig {
            client_build_number: 346892,
            browser_version: "131.0.0.0".to_string(),
            browser_user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string(),
        }
    }

    #[test]
    fn identify_properties_match_web_client_format() {
        let config = test_config();
        let props = build_identify_properties(&config);

        assert_eq!(props.os, "Mac OS X");
        assert_eq!(props.browser, "Chrome");
        assert_eq!(props.device, "");
        assert_eq!(props.system_locale, "en-US");
        assert_eq!(props.browser_user_agent, config.browser_user_agent);
        assert_eq!(props.browser_version, "131.0.0.0");
        assert_eq!(props.os_version, "10.15.7");
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

        // All expected fields must be present
        assert_eq!(json["os"], "Mac OS X");
        assert_eq!(json["browser"], "Chrome");
        assert_eq!(json["device"], "");
        assert_eq!(json["system_locale"], "en-US");
        assert_eq!(json["browser_version"], "131.0.0.0");
        assert_eq!(json["os_version"], "10.15.7");
        assert_eq!(json["release_channel"], "stable");
        assert_eq!(json["client_build_number"], 346892);

        // Verify it's a JSON object with all 13 fields
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 13);
    }

    #[test]
    fn identify_properties_uses_config_values() {
        let config = DiscordConfig {
            client_build_number: 999999,
            browser_version: "200.0.0.0".to_string(),
            browser_user_agent: "CustomAgent/1.0".to_string(),
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
        let json: serde_json::Value =
            serde_json::from_str(&json_str).expect("Must be valid JSON");

        // Must contain the expected fields
        assert_eq!(json["os"], "Mac OS X");
        assert_eq!(json["browser"], "Chrome");
        assert_eq!(json["client_build_number"], 346892);
        assert_eq!(json["browser_version"], "131.0.0.0");
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
        let headers = build_http_headers(&config, token);

        // Must have all 4 required headers
        assert!(headers.contains_key("User-Agent"));
        assert!(headers.contains_key("X-Super-Properties"));
        assert!(headers.contains_key("X-Discord-Locale"));
        assert!(headers.contains_key("Authorization"));
    }

    #[test]
    fn http_headers_user_agent_matches_config() {
        let config = test_config();
        let headers = build_http_headers(&config, "token");

        let ua = headers.get("User-Agent").unwrap().to_str().unwrap();
        assert_eq!(ua, config.browser_user_agent);
    }

    #[test]
    fn http_headers_authorization_has_no_bot_prefix() {
        let config = test_config();
        let token = "mfa.test_token_value";
        let headers = build_http_headers(&config, token);

        let auth = headers.get("Authorization").unwrap().to_str().unwrap();
        assert_eq!(auth, "mfa.test_token_value");
        assert!(!auth.starts_with("Bot "));
        assert!(!auth.starts_with("Bearer "));
    }

    #[test]
    fn http_headers_discord_locale_is_set() {
        let config = test_config();
        let headers = build_http_headers(&config, "token");

        let locale = headers.get("X-Discord-Locale").unwrap().to_str().unwrap();
        assert_eq!(locale, "en-US");
    }

    #[test]
    fn http_headers_super_properties_is_valid_base64() {
        let config = test_config();
        let headers = build_http_headers(&config, "token");

        let super_props = headers
            .get("X-Super-Properties")
            .unwrap()
            .to_str()
            .unwrap();

        // Must decode as base64 → JSON
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(super_props)
            .expect("X-Super-Properties header must be valid base64");
        let json: serde_json::Value =
            serde_json::from_str(&String::from_utf8(decoded).unwrap()).unwrap();
        assert_eq!(json["browser"], "Chrome");
    }
}
