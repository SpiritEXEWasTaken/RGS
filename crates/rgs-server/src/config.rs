//! Server settings, read from `rgs.toml` (all optional) with a few environment overrides.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Address to listen on. Behind Cloudflare Tunnel, keep this on localhost.
    pub listen: String,
    /// Folder for the database and the server's secret key.
    pub data_dir: PathBuf,
    /// Name shown in authenticator apps next to the account name.
    pub issuer: String,
    /// Trust the `CF-Connecting-IP` header for the client address (only behind Cloudflare).
    pub behind_cloudflare: bool,
    /// Origins allowed to call the API from a browser (the website).
    pub allowed_origins: Vec<String>,
    /// How long a normal login lasts, in days.
    pub session_days: i64,
    /// How long a headset (device code) login lasts, in days.
    pub device_session_days: i64,
    /// How long an admin panel login lasts, in hours.
    pub admin_session_hours: i64,
    /// Seconds between "rebooting right now" and the server actually stopping, so clients see it.
    pub restart_grace_seconds: i64,
    /// Games using this server, with the versions players must have.
    pub games: Vec<GameConfig>,
    /// A Discord channel webhook that gets a message for every new report (optional).
    pub discord_reports_webhook: Option<String>,
    /// The RGS website's API, e.g. https://spiritexe.pages.dev/rgs/api (optional).
    pub hub_url: Option<String>,
    /// This server's key from "Link a server" on the RGS website. Keep it secret.
    pub hub_key: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8650".into(),
            data_dir: PathBuf::from("data"),
            issuer: "RGS".into(),
            behind_cloudflare: false,
            allowed_origins: Vec::new(),
            session_days: 30,
            device_session_days: 180,
            admin_session_hours: 12,
            restart_grace_seconds: 20,
            games: Vec::new(),
            discord_reports_webhook: None,
            hub_url: None,
            hub_key: None,
        }
    }
}

/// A game on this server. Clients send `X-Game-Id` and `X-Game-Version`; anyone below
/// `min_version` is told to update.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct GameConfig {
    pub id: String,
    pub name: String,
    pub latest_version: String,
    pub min_version: String,
}

impl Config {
    /// Loads `rgs.toml` from the working directory (or the file in `RGS_CONFIG`) if it exists,
    /// then applies `RGS_LISTEN` and `RGS_DATA_DIR` from the environment.
    pub fn load() -> anyhow::Result<Self> {
        let path = std::env::var("RGS_CONFIG").unwrap_or_else(|_| "rgs.toml".into());
        let mut config = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)?,
            Err(_) => Config::default(),
        };
        if let Ok(v) = std::env::var("RGS_LISTEN") {
            config.listen = v;
        }
        if let Ok(v) = std::env::var("RGS_DATA_DIR") {
            config.data_dir = v.into();
        }
        Ok(config)
    }
}
