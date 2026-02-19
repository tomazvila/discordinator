use color_eyre::eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level application configuration, loaded from TOML.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub discord: DiscordConfig,
    #[serde(default)]
    pub appearance: AppearanceConfig,
    #[serde(default)]
    pub pane: PaneConfig,
    #[serde(default)]
    pub session: SessionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_timestamp_format")]
    pub timestamp_format: String,
    #[serde(default = "default_message_cache_size")]
    pub message_cache_size: u32,
    #[serde(default = "default_true")]
    pub show_typing_indicator: bool,
    #[serde(default = "default_true")]
    pub desktop_notifications: bool,
    #[serde(default = "default_render_fps")]
    pub render_fps: u32,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            timestamp_format: default_timestamp_format(),
            message_cache_size: default_message_cache_size(),
            show_typing_indicator: true,
            desktop_notifications: true,
            render_fps: default_render_fps(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_token_source")]
    pub token_source: String,
    pub token_file: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            token_source: default_token_source(),
            token_file: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    #[serde(default = "default_client_build_number")]
    pub client_build_number: u64,
    #[serde(default = "default_browser_version")]
    pub browser_version: String,
    #[serde(default = "default_browser_user_agent")]
    pub browser_user_agent: String,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            client_build_number: default_client_build_number(),
            browser_version: default_browser_version(),
            browser_user_agent: default_browser_user_agent(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceConfig {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_true")]
    pub show_sidebar: bool,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: u16,
    #[serde(default = "default_true")]
    pub message_date_separator: bool,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            show_sidebar: true,
            sidebar_width: default_sidebar_width(),
            message_date_separator: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneConfig {
    #[serde(default = "default_prefix_key")]
    pub prefix_key: String,
    #[serde(default = "default_border_style")]
    pub border_style: String,
    #[serde(default = "default_active_border_color")]
    pub active_border_color: String,
    #[serde(default = "default_inactive_border_color")]
    pub inactive_border_color: String,
    #[serde(default = "default_true")]
    pub show_pane_title: bool,
}

impl Default for PaneConfig {
    fn default() -> Self {
        Self {
            prefix_key: default_prefix_key(),
            border_style: default_border_style(),
            active_border_color: default_active_border_color(),
            inactive_border_color: default_inactive_border_color(),
            show_pane_title: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    #[serde(default = "default_true")]
    pub auto_save: bool,
    #[serde(default = "default_true")]
    pub restore_on_start: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            auto_save: true,
            restore_on_start: true,
        }
    }
}

// Default value functions
fn default_timestamp_format() -> String {
    "%H:%M".to_string()
}
fn default_message_cache_size() -> u32 {
    10000
}
fn default_true() -> bool {
    true
}
fn default_render_fps() -> u32 {
    60
}
fn default_token_source() -> String {
    "keyring".to_string()
}
fn default_client_build_number() -> u64 {
    346892
}
fn default_browser_version() -> String {
    "131.0.0.0".to_string()
}
fn default_browser_user_agent() -> String {
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string()
}
fn default_theme() -> String {
    "default".to_string()
}
fn default_sidebar_width() -> u16 {
    24
}
fn default_prefix_key() -> String {
    "Ctrl+b".to_string()
}
fn default_border_style() -> String {
    "rounded".to_string()
}
fn default_active_border_color() -> String {
    "cyan".to_string()
}
fn default_inactive_border_color() -> String {
    "gray".to_string()
}

/// XDG directory paths for the application.
pub struct AppDirs {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl AppDirs {
    /// Resolve XDG directories for the application.
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| color_eyre::eyre::eyre!("Could not determine config directory"))?
            .join("discordinator");
        let data_dir = dirs::data_dir()
            .ok_or_else(|| color_eyre::eyre::eyre!("Could not determine data directory"))?
            .join("discordinator");
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| color_eyre::eyre::eyre!("Could not determine cache directory"))?
            .join("discordinator");

        Ok(Self {
            config_dir,
            data_dir,
            cache_dir,
        })
    }

    /// Config file path.
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// Database file path.
    pub fn database_file(&self) -> PathBuf {
        self.data_dir.join("messages.db")
    }

    /// Log directory path.
    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
}

/// Load config from the given path, or return defaults if file doesn't exist.
pub fn load_config(path: &std::path::Path) -> Result<AppConfig> {
    if path.exists() {
        let content =
            std::fs::read_to_string(path).wrap_err_with(|| format!("Failed to read {:?}", path))?;
        let config: AppConfig = toml::from_str(&content)
            .wrap_err_with(|| format!("Failed to parse config from {:?}", path))?;
        Ok(config)
    } else {
        Ok(AppConfig::default())
    }
}

/// Load config from the given path, creating a default config file if it doesn't exist.
pub fn load_or_create_config(path: &std::path::Path) -> Result<AppConfig> {
    if path.exists() {
        load_config(path)
    } else {
        let config = AppConfig::default();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .wrap_err("Failed to create config directory")?;
        }
        let toml_string = toml::to_string_pretty(&config)
            .wrap_err("Failed to serialize default config")?;
        std::fs::write(path, &toml_string)
            .wrap_err_with(|| format!("Failed to write default config to {:?}", path))?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn default_config_has_expected_values() {
        let config = AppConfig::default();
        assert_eq!(config.general.timestamp_format, "%H:%M");
        assert_eq!(config.general.message_cache_size, 10000);
        assert!(config.general.show_typing_indicator);
        assert!(config.general.desktop_notifications);
        assert_eq!(config.general.render_fps, 60);
        assert_eq!(config.auth.token_source, "keyring");
        assert!(config.auth.token_file.is_none());
        assert_eq!(config.discord.client_build_number, 346892);
        assert_eq!(config.discord.browser_version, "131.0.0.0");
        assert!(config.discord.browser_user_agent.contains("Chrome/131"));
        assert_eq!(config.appearance.theme, "default");
        assert!(config.appearance.show_sidebar);
        assert_eq!(config.appearance.sidebar_width, 24);
        assert!(config.appearance.message_date_separator);
        assert_eq!(config.pane.prefix_key, "Ctrl+b");
        assert_eq!(config.pane.border_style, "rounded");
        assert_eq!(config.pane.active_border_color, "cyan");
        assert_eq!(config.pane.inactive_border_color, "gray");
        assert!(config.pane.show_pane_title);
        assert!(config.session.auto_save);
        assert!(config.session.restore_on_start);
    }

    #[test]
    fn parse_full_config_toml() {
        let toml_str = r#"
[general]
timestamp_format = "%H:%M:%S"
message_cache_size = 5000
show_typing_indicator = false
desktop_notifications = false
render_fps = 30

[auth]
token_source = "file"
token_file = "~/.config/discordinator/token"

[discord]
client_build_number = 999999
browser_version = "200.0.0.0"
browser_user_agent = "CustomAgent/1.0"

[appearance]
theme = "custom"
show_sidebar = false
sidebar_width = 32
message_date_separator = false

[pane]
prefix_key = "Ctrl+a"
border_style = "double"
active_border_color = "green"
inactive_border_color = "dark_gray"
show_pane_title = false

[session]
auto_save = false
restore_on_start = false
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.timestamp_format, "%H:%M:%S");
        assert_eq!(config.general.message_cache_size, 5000);
        assert!(!config.general.show_typing_indicator);
        assert_eq!(config.general.render_fps, 30);
        assert_eq!(config.auth.token_source, "file");
        assert_eq!(
            config.auth.token_file,
            Some("~/.config/discordinator/token".to_string())
        );
        assert_eq!(config.discord.client_build_number, 999999);
        assert_eq!(config.discord.browser_version, "200.0.0.0");
        assert_eq!(config.appearance.theme, "custom");
        assert!(!config.appearance.show_sidebar);
        assert_eq!(config.appearance.sidebar_width, 32);
        assert_eq!(config.pane.prefix_key, "Ctrl+a");
        assert_eq!(config.pane.border_style, "double");
        assert!(!config.pane.show_pane_title);
        assert!(!config.session.auto_save);
    }

    #[test]
    fn parse_partial_config_uses_defaults() {
        let toml_str = r#"
[general]
render_fps = 120
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.render_fps, 120);
        // Everything else should be defaults
        assert_eq!(config.general.timestamp_format, "%H:%M");
        assert_eq!(config.general.message_cache_size, 10000);
        assert_eq!(config.discord.client_build_number, 346892);
        assert_eq!(config.appearance.sidebar_width, 24);
    }

    #[test]
    fn parse_empty_config_uses_all_defaults() {
        let toml_str = "";
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.timestamp_format, "%H:%M");
        assert_eq!(config.discord.client_build_number, 346892);
        assert!(config.session.auto_save);
    }

    #[test]
    fn invalid_config_returns_error() {
        let toml_str = r#"
[general]
render_fps = "not a number"
"#;
        let result: Result<AppConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn load_config_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        let toml_str = r#"
[general]
render_fps = 30
[discord]
client_build_number = 111111
"#;
        std::fs::write(&config_path, toml_str).unwrap();

        let config = load_config(&config_path).unwrap();
        assert_eq!(config.general.render_fps, 30);
        assert_eq!(config.discord.client_build_number, 111111);
    }

    #[test]
    fn load_config_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("nonexistent.toml");

        let config = load_config(&config_path).unwrap();
        assert_eq!(config.general.render_fps, 60);
        assert_eq!(config.discord.client_build_number, 346892);
    }

    #[test]
    fn load_or_create_config_creates_default_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("subdir").join("config.toml");

        assert!(!config_path.exists());
        let config = load_or_create_config(&config_path).unwrap();
        assert!(config_path.exists());
        assert_eq!(config.general.render_fps, 60);

        // Verify the created file can be re-loaded
        let reloaded = load_config(&config_path).unwrap();
        assert_eq!(reloaded.general.render_fps, 60);
        assert_eq!(reloaded.discord.client_build_number, 346892);
    }

    #[test]
    fn config_roundtrip_serialization() {
        let config = AppConfig::default();
        let toml_string = toml::to_string_pretty(&config).unwrap();
        let deserialized: AppConfig = toml::from_str(&toml_string).unwrap();
        assert_eq!(
            config.general.timestamp_format,
            deserialized.general.timestamp_format
        );
        assert_eq!(
            config.discord.client_build_number,
            deserialized.discord.client_build_number
        );
        assert_eq!(config.pane.prefix_key, deserialized.pane.prefix_key);
    }

    #[test]
    fn app_dirs_resolves() {
        let dirs = AppDirs::new().unwrap();
        assert!(dirs.config_dir.to_str().unwrap().contains("discordinator"));
        assert!(dirs.data_dir.to_str().unwrap().contains("discordinator"));
        assert!(dirs.cache_dir.to_str().unwrap().contains("discordinator"));
        assert!(dirs.config_file().to_str().unwrap().ends_with("config.toml"));
        assert!(dirs.database_file().to_str().unwrap().ends_with("messages.db"));
        assert!(dirs.log_dir().to_str().unwrap().ends_with("logs"));
    }

    #[test]
    fn anti_detection_fields_present_in_discord_config() {
        let config = DiscordConfig::default();
        assert!(config.client_build_number > 0);
        assert!(!config.browser_version.is_empty());
        assert!(config.browser_user_agent.contains("Mozilla"));
        assert!(config.browser_user_agent.contains("Chrome"));
    }
}
