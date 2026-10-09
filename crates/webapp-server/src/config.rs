use anyhow::{Context, Result, bail};

/// Framework settings, read from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub upstash_url: String,
    pub upstash_token: String,
    /// Prepended to every Redis key (`REDIS_KEY_PREFIX`), so several apps can
    /// share one Upstash database. Empty when unset.
    pub key_prefix: String,
    /// `None` disables Discord activity login.
    pub discord: Option<DiscordConfig>,
    /// Shown to users, e.g. in the login email subject (`APP_NAME`).
    pub app_name: String,
    /// Origin the emailed login link points at (`PUBLIC_BASE_URL`), e.g.
    /// `https://example.com`. Required when email login is enabled.
    pub public_base_url: Option<String>,
    pub email: EmailDelivery,
}

/// Discord application credentials (`DISCORD_CLIENT_ID`,
/// `DISCORD_CLIENT_SECRET`).
#[derive(Debug, Clone)]
pub struct DiscordConfig {
    pub client_id: String,
    pub client_secret: String,
}

/// How login emails are delivered.
#[derive(Debug, Clone)]
pub enum EmailDelivery {
    /// Send via Resend's REST API (`RESEND_API_KEY`, `RESEND_FROM_EMAIL`).
    Resend { api_key: String, from: String },
    /// `DEV_MODE=1` without a Resend key: print the login link to the console.
    Console,
    /// Email login turned off; its routes return 404.
    Disabled,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let email = match optional("RESEND_API_KEY") {
            Some(api_key) => EmailDelivery::Resend {
                api_key,
                from: required("RESEND_FROM_EMAIL")?,
            },
            None if optional("DEV_MODE").as_deref() == Some("1") => EmailDelivery::Console,
            None => EmailDelivery::Disabled,
        };
        let discord = match (optional("DISCORD_CLIENT_ID"), optional("DISCORD_CLIENT_SECRET")) {
            (Some(client_id), Some(client_secret)) => Some(DiscordConfig {
                client_id,
                client_secret,
            }),
            (None, None) => None,
            _ => bail!("DISCORD_CLIENT_ID and DISCORD_CLIENT_SECRET must be set together"),
        };
        let public_base_url = optional("PUBLIC_BASE_URL").map(|url| url.trim_end_matches('/').to_string());
        if !matches!(email, EmailDelivery::Disabled) && public_base_url.is_none() {
            bail!("PUBLIC_BASE_URL is required when email login is enabled");
        }

        Ok(Self {
            upstash_url: required("UPSTASH_REDIS_REST_URL")?,
            upstash_token: required("UPSTASH_REDIS_REST_TOKEN")?,
            key_prefix: optional("REDIS_KEY_PREFIX").unwrap_or_default(),
            discord,
            app_name: optional("APP_NAME").unwrap_or_else(|| "webapp".to_string()),
            public_base_url,
            email,
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
