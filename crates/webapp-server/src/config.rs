use anyhow::{Context, Result};

/// Framework settings, read from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub upstash_url: String,
    pub upstash_token: String,
    /// Prepended to every Redis key (`REDIS_KEY_PREFIX`), so several apps can
    /// share one Upstash database. Empty when unset.
    pub key_prefix: String,
    /// `None` disables Discord activity login.
    pub discord_client_id: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            upstash_url: required("UPSTASH_REDIS_REST_URL")?,
            upstash_token: required("UPSTASH_REDIS_REST_TOKEN")?,
            key_prefix: optional("REDIS_KEY_PREFIX").unwrap_or_default(),
            discord_client_id: optional("DISCORD_CLIENT_ID"),
        })
    }
}

fn required(name: &str) -> Result<String> {
    optional(name).with_context(|| format!("{name} is not set"))
}

/// An unset or blank variable is treated as absent, so `.env.example`'s
/// empty `NAME=` lines don't count as configured.
fn optional(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.trim().is_empty())
}
