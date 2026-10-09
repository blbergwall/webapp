use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;

use crate::config::Config;

#[derive(Deserialize)]
struct UpstashResponse {
    result: Option<Value>,
    error: Option<String>,
}

/// Client for Upstash Redis's REST API. Cheap to clone (the inner
/// `reqwest::Client` is reference-counted).
///
/// The typed helpers take *unprefixed* keys and apply `Config::key_prefix`
/// themselves. For any other command, build keys with [`Redis::key`] and pass
/// them to [`Redis::command`].
#[derive(Clone)]
pub struct Redis {
    http: reqwest::Client,
    url: String,
    token: String,
    prefix: String,
}

impl Redis {
    pub fn new(config: &Config) -> Self {
        Self {
            http: reqwest::Client::new(),
            url: config.upstash_url.clone(),
            token: config.upstash_token.clone(),
            prefix: config.key_prefix.clone(),
        }
    }

    /// The full Redis key for `key`, with the app's namespace prefix applied.
    pub fn key(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    /// Runs a single Redis command against Upstash's REST API, passing the
    /// command and its arguments as a JSON array body. This avoids
    /// URL-encoding concerns that come with path-style Upstash requests (e.g.
    /// email addresses or tokens containing characters that would need
    /// escaping). Arguments are sent as-is: keys must already be prefixed
    /// (see [`Redis::key`]).
    ///
    /// Upstash returns HTTP 200 with a `{"error": "..."}` body for
    /// Redis/Lua-level errors (e.g. a wrong-type command) — not a non-2xx
    /// status — so that case is checked explicitly and surfaced with its real
    /// message, rather than falling through to a generic deserialize failure.
    pub async fn command(&self, args: &[&str]) -> Result<Value> {
        let response = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .json(args)
            .send()
            .await
            .context("failed to reach Upstash Redis REST API")?;

        if !response.status().is_success() {
            bail!("Upstash Redis REST API returned status {}", response.status());
        }

        let body: UpstashResponse = response
            .json()
            .await
            .context("failed to parse Upstash Redis REST API response")?;

        if let Some(error) = body.error {
            bail!("Upstash Redis REST API error: {error}");
        }

        Ok(body.result.unwrap_or(Value::Null))
    }

    /// `GET key`
    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let result = self.command(&["GET", &self.key(key)]).await?;
        Ok(result.as_str().map(String::from))
    }

    /// `GETEX key EX ttl_secs` — fetch a value and reset its TTL in one round
    /// trip (used for sliding session expiry).
    pub async fn getex(&self, key: &str, ttl_secs: u64) -> Result<Option<String>> {
        let ttl = ttl_secs.to_string();
        let result = self.command(&["GETEX", &self.key(key), "EX", &ttl]).await?;
        Ok(result.as_str().map(String::from))
    }

    /// `GETDEL key` — atomically fetch and delete in one round trip, so a
    /// duplicate/retried request can't see the value twice.
    pub async fn getdel(&self, key: &str) -> Result<Option<String>> {
        let result = self.command(&["GETDEL", &self.key(key)]).await?;
        Ok(result.as_str().map(String::from))
    }

    /// `SET key value` — write a value with no expiration.
    pub async fn set(&self, key: &str, value: &str) -> Result<()> {
        self.command(&["SET", &self.key(key), value]).await?;
        Ok(())
    }

    /// `SET key value EX ttl_secs` — write a value with an expiration.
    pub async fn set_ex(&self, key: &str, value: &str, ttl_secs: u64) -> Result<()> {
        let ttl = ttl_secs.to_string();
        self.command(&["SET", &self.key(key), value, "EX", &ttl]).await?;
        Ok(())
    }

    /// `SET key value NX [EX ttl_secs]` — atomic check-and-write: only sets
    /// the value if the key does not already exist. Returns `true` if this
    /// call created the key, `false` if it was already present (and left
    /// untouched).
    pub async fn set_nx_ex(&self, key: &str, value: &str, ttl_secs: Option<u64>) -> Result<bool> {
        let key = self.key(key);
        let ttl;
        let mut args = vec!["SET", key.as_str(), value, "NX"];
        if let Some(ttl_secs) = ttl_secs {
            ttl = ttl_secs.to_string();
            args.extend(["EX", ttl.as_str()]);
        }
        Ok(!self.command(&args).await?.is_null())
    }

    /// `DEL key`
    pub async fn del(&self, key: &str) -> Result<()> {
        self.command(&["DEL", &self.key(key)]).await?;
        Ok(())
    }
}
