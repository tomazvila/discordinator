//! Auto-fetches Discord client properties (build number, capabilities, browser version)
//! from the live Discord web client to stay in sync with the latest deployment.
//!
//! Fetched once at startup, cached to disk with a TTL. Falls back to config defaults on failure.

use color_eyre::eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, SystemTime};

/// How long cached properties remain valid before re-fetching.
const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60); // 6 hours

/// Fetched Discord client properties.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordProperties {
    pub client_build_number: u64,
    pub capabilities: u64,
    pub browser_version: String,
    pub browser_user_agent: String,
    #[serde(with = "system_time_serde")]
    pub fetched_at: SystemTime,
}

/// Cached properties wrapper with TTL check.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedProperties {
    properties: DiscordProperties,
}

impl DiscordProperties {
    /// Check if these properties are still fresh.
    pub fn is_fresh(&self) -> bool {
        self.fetched_at
            .elapsed()
            .map(|elapsed| elapsed < CACHE_TTL)
            .unwrap_or(false)
    }
}

/// Fetch fresh Discord properties from the web client.
///
/// 1. Fetches the Discord login page to extract `BUILD_NUMBER` and JS bundle URL
/// 2. Fetches the JS bundle to extract the capabilities bitmask
/// 3. Fetches the latest Chrome version from Google's version history API
///
/// Returns `None` if any step fails (caller should fall back to defaults).
pub async fn fetch_discord_properties(user_agent: &str) -> Result<DiscordProperties> {
    let client = crate::infrastructure::anti_detection::build_chrome_client_simple(user_agent)
        .wrap_err("Failed to build HTTP client for property fetch")?;

    // Step 1: Fetch Discord login page
    let login_html = client
        .get("https://discord.com/login")
        .send()
        .await
        .wrap_err("Failed to fetch Discord login page")?
        .text()
        .await
        .wrap_err("Failed to read Discord login page body")?;

    let build_number = parse_build_number(&login_html)
        .ok_or_else(|| color_eyre::eyre::eyre!("Could not find BUILD_NUMBER in login page"))?;

    let bundle_url = parse_bundle_url(&login_html)
        .ok_or_else(|| color_eyre::eyre::eyre!("Could not find JS bundle URL in login page"))?;

    // Step 2: Fetch JS bundle for capabilities
    let bundle_js = client
        .get(format!("https://discord.com{bundle_url}"))
        .send()
        .await
        .wrap_err("Failed to fetch Discord JS bundle")?
        .text()
        .await
        .wrap_err("Failed to read Discord JS bundle body")?;

    let capabilities = parse_capabilities(&bundle_js).unwrap_or(30717);

    // Step 3: Fetch latest Chrome version
    let browser_version = fetch_chrome_version(&client)
        .await
        .unwrap_or_else(|_| "131.0.0.0".to_string());

    let browser_user_agent = format!(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{browser_version} Safari/537.36"
    );

    Ok(DiscordProperties {
        client_build_number: build_number,
        capabilities,
        browser_version,
        browser_user_agent,
        fetched_at: SystemTime::now(),
    })
}

/// Try to load cached properties from disk. Returns `None` if missing, stale, or corrupt.
pub fn load_cached(cache_path: &Path) -> Option<DiscordProperties> {
    let content = std::fs::read_to_string(cache_path).ok()?;
    let cached: CachedProperties = serde_json::from_str(&content).ok()?;
    if cached.properties.is_fresh() {
        Some(cached.properties)
    } else {
        None
    }
}

/// Save properties to disk cache.
pub fn save_cached(cache_path: &Path, props: &DiscordProperties) -> Result<()> {
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).wrap_err("Failed to create cache directory")?;
    }
    let cached = CachedProperties {
        properties: props.clone(),
    };
    let json =
        serde_json::to_string_pretty(&cached).wrap_err("Failed to serialize properties cache")?;
    std::fs::write(cache_path, json).wrap_err("Failed to write properties cache")?;
    Ok(())
}

// --- Parsing functions (public for testing) ---

/// Extract `BUILD_NUMBER` from Discord's `window.GLOBAL_ENV` in the login page HTML.
pub fn parse_build_number(html: &str) -> Option<u64> {
    // Pattern: "BUILD_NUMBER":"507104" or BUILD_NUMBER:"507104"
    let re_patterns = [
        r#""BUILD_NUMBER":"(\d+)""#,
        r#"BUILD_NUMBER:"(\d+)""#,
        r#""buildNumber":"(\d+)""#,
    ];

    for pattern in &re_patterns {
        if let Some(caps) = regex_find(html, pattern) {
            if let Ok(num) = caps.parse::<u64>() {
                return Some(num);
            }
        }
    }
    None
}

/// Extract the main JS bundle URL from the login page HTML.
/// Looks for `<script defer src="/assets/web.{hash}.js">`
pub fn parse_bundle_url(html: &str) -> Option<String> {
    // Look for the web bundle script tag
    let marker = "src=\"/assets/web.";
    let start = html.find(marker)?;
    let src_start = start + "src=\"".len();
    let rest = &html[src_start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Extract the capabilities base value from the Discord JS bundle.
/// Looks for the pattern near `useChannelObfuscation`.
pub fn parse_capabilities(js: &str) -> Option<u64> {
    // Primary pattern: let r=32768,i=1734653;function s(e){let{useChannelObfuscation
    // The base capabilities is the `i` value
    let patterns = [
        // Pattern: ,i=DIGITS;function ...useChannelObfuscation
        r",i=(\d+);function",
        // Alternative: direct assignment patterns
        r"capabilities:(\d{5,})",
    ];

    // Search near "useChannelObfuscation" first
    if let Some(idx) = js.find("useChannelObfuscation") {
        // Look backwards up to 200 chars for the value
        let search_start = idx.saturating_sub(200);
        let region = &js[search_start..idx];
        if let Some(caps) = regex_find(region, r",i=(\d+)") {
            if let Ok(num) = caps.parse::<u64>() {
                return Some(num);
            }
        }
    }

    // Fallback: search the whole bundle
    for pattern in &patterns {
        if let Some(caps) = regex_find(js, pattern) {
            if let Ok(num) = caps.parse::<u64>() {
                if num > 10000 {
                    // Capabilities is always a large number
                    return Some(num);
                }
            }
        }
    }

    None
}

/// Fetch the latest stable Chrome version from Google's version history API.
async fn fetch_chrome_version(client: &rquest::Client) -> Result<String> {
    #[derive(Deserialize)]
    struct VersionResponse {
        versions: Vec<VersionEntry>,
    }
    #[derive(Deserialize)]
    struct VersionEntry {
        version: String,
    }

    let resp: VersionResponse = client
        .get("https://versionhistory.googleapis.com/v1/chrome/platforms/mac/channels/stable/versions")
        .send()
        .await
        .wrap_err("Failed to fetch Chrome version")?
        .json()
        .await
        .wrap_err("Failed to parse Chrome version response")?;

    resp.versions
        .first()
        .map(|v| v.version.clone())
        .ok_or_else(|| color_eyre::eyre::eyre!("No Chrome versions returned"))
}

/// Simple regex-like first capture group extraction without pulling in the regex crate.
/// Supports a limited subset: literal chars + `(\d+)` capture group.
fn regex_find(haystack: &str, pattern: &str) -> Option<String> {
    // Find the capture group position in the pattern
    let cap_start = pattern.find("(\\d+)")?;
    let prefix = &pattern[..cap_start];
    let suffix = &pattern[cap_start + 5..]; // "(\\d+)" is 5 chars

    // Unescape the prefix/suffix for literal matching
    let prefix_literal = prefix.replace("\\\"", "\"");
    let suffix_literal = suffix.replace("\\\"", "\"");

    // Find prefix in haystack
    let mut search_start = 0;
    loop {
        let prefix_pos = haystack[search_start..].find(&prefix_literal)?;
        let abs_pos = search_start + prefix_pos + prefix_literal.len();

        if abs_pos >= haystack.len() {
            return None;
        }

        // Extract digits starting at abs_pos
        let digit_start = abs_pos;
        let digit_end = haystack[digit_start..]
            .find(|c: char| !c.is_ascii_digit())
            .map_or(haystack.len(), |i| digit_start + i);

        if digit_end > digit_start {
            let digits = &haystack[digit_start..digit_end];
            // Check suffix matches after digits
            let after_digits = &haystack[digit_end..];
            if suffix_literal.is_empty() || after_digits.starts_with(&suffix_literal) {
                return Some(digits.to_string());
            }
        }

        // Advance past the current match attempt to avoid infinite loop
        search_start = digit_end.max(abs_pos + 1);
    }
}

/// Serde helpers for `SystemTime` (serialize as epoch seconds).
mod system_time_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let secs = time
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        serializer.serialize_u64(secs)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + Duration::from_secs(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_build_number_from_global_env() {
        let html = r#"<script>window.GLOBAL_ENV = {"API_ENDPOINT":"//discord.com/api","BUILD_NUMBER":"507104","RELEASE_CHANNEL":"stable"}</script>"#;
        assert_eq!(parse_build_number(html), Some(507104));
    }

    #[test]
    fn parse_build_number_missing_returns_none() {
        let html = "<html><body>No build number here</body></html>";
        assert_eq!(parse_build_number(html), None);
    }

    #[test]
    fn parse_bundle_url_from_script_tag() {
        let html = r#"<script defer src="/assets/web.abc123def.js"></script>"#;
        assert_eq!(
            parse_bundle_url(html),
            Some("/assets/web.abc123def.js".to_string())
        );
    }

    #[test]
    fn parse_bundle_url_missing_returns_none() {
        let html = "<html><body>No scripts</body></html>";
        assert_eq!(parse_bundle_url(html), None);
    }

    #[test]
    fn parse_capabilities_from_js_bundle() {
        let js = r#"something;let r=32768,i=1734653;function s(e){let{useChannelObfuscation:t}=e;return t?i|r:i}"#;
        assert_eq!(parse_capabilities(js), Some(1_734_653));
    }

    #[test]
    fn parse_capabilities_missing_returns_none() {
        let js = "var x = 42; function hello() {}";
        assert_eq!(parse_capabilities(js), None);
    }

    #[test]
    fn discord_properties_freshness() {
        let fresh = DiscordProperties {
            client_build_number: 507104,
            capabilities: 1734653,
            browser_version: "131.0.0.0".to_string(),
            browser_user_agent: "test".to_string(),
            fetched_at: SystemTime::now(),
        };
        assert!(fresh.is_fresh());

        let stale = DiscordProperties {
            fetched_at: SystemTime::now() - Duration::from_secs(7 * 60 * 60), // 7 hours ago
            ..fresh
        };
        assert!(!stale.is_fresh());
    }

    #[test]
    fn cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("discord_properties.json");

        let props = DiscordProperties {
            client_build_number: 507104,
            capabilities: 1734653,
            browser_version: "131.0.0.0".to_string(),
            browser_user_agent: "test".to_string(),
            fetched_at: SystemTime::now(),
        };

        save_cached(&cache_path, &props).unwrap();
        let loaded = load_cached(&cache_path).unwrap();

        assert_eq!(loaded.client_build_number, 507104);
        assert_eq!(loaded.capabilities, 1_734_653);
        assert_eq!(loaded.browser_version, "131.0.0.0");
    }

    #[test]
    fn load_cached_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("nonexistent.json");
        assert!(load_cached(&cache_path).is_none());
    }

    #[test]
    fn load_cached_corrupt_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("corrupt.json");
        std::fs::write(&cache_path, "not valid json").unwrap();
        assert!(load_cached(&cache_path).is_none());
    }

    #[test]
    fn regex_find_extracts_digits() {
        assert_eq!(
            regex_find(r#""BUILD_NUMBER":"507104""#, r#""BUILD_NUMBER":"(\d+)""#),
            Some("507104".to_string())
        );
    }

    #[test]
    fn regex_find_no_match_returns_none() {
        assert_eq!(
            regex_find("no match here", r#""BUILD_NUMBER":"(\d+)""#),
            None
        );
    }
}
