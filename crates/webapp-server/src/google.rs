//! Google sign-in ("Sign in with Google" in website mode): OpenID Connect's
//! authorization-code flow, driven by the SPA (the shared pattern is in
//! `oauth.rs`).
//!
//! The user's identity comes from the ID token in the token endpoint's
//! response. Google: when a server gets it "directly from Google over an
//! intermediary-free HTTPS channel and using your client secret … you can be
//! confident that the token you receive really comes from Google and is
//! valid", so its claims are read without verifying the JWT signature. Its
//! `aud` and `nonce` are still checked.
//!
//! A Google account is matched to an email account only when Google is
//! authoritative for the address: see [`trusted_email`].

use anyhow::{Context, anyhow, bail};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use reqwest::Url;
use serde::Deserialize;
use webapp_common::{Account, OAuthConfirmRequest, OAuthStartResponse};

use crate::{
    WebApp, account,
    config::GoogleConfig,
    email::normalize_email,
    error::ApiError,
    oauth::{self, GOOGLE},
    session,
};

const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// `POST /api/auth/google/start` — returns `{url}` (Google's sign-in page)
/// and sets the `google_state` cookie. 404 if Google isn't configured.
pub(crate) async fn start(State(app): State<WebApp>) -> Result<impl IntoResponse, ApiError> {
    let config = app.config().google.as_ref().ok_or(ApiError::NotFound)?;
    let state = GOOGLE.new_state();
    let url = authorize_url(config, &state, &oauth::redirect_uri(&app)?);
    Ok((
        [(header::SET_COOKIE, GOOGLE.state_cookie(&state))],
        Json(OAuthStartResponse { url }),
    ))
}

/// `POST /api/auth/google/confirm` — signs in with the `code` and `state`
/// Google redirected back with (finding, linking or creating the account),
/// sets the session cookie and returns the `Account`.
///
/// 400 if the state doesn't match this browser's cookie (expired, or started
/// in another browser), the code is empty, or the ID token's nonce doesn't
/// match; 401 if Google rejects the code; 404 if Google isn't configured.
/// The state cookie is cleared either way.
pub(crate) async fn confirm(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(request): Json<OAuthConfirmRequest>,
) -> Response {
    GOOGLE.confirm_response(sign_in(&app, &headers, &request).await)
}

/// Returns the new session token and the account.
async fn sign_in(
    app: &WebApp,
    headers: &HeaderMap,
    request: &OAuthConfirmRequest,
) -> Result<(String, Account), ApiError> {
    let config = app.config().google.as_ref().ok_or(ApiError::NotFound)?;
    GOOGLE.check_state(headers, &request.state)?;
    let code = request.code.trim();
    if code.is_empty() {
        return Err(ApiError::BadRequest);
    }

    let id_token = exchange_code(app, config, code, &oauth::redirect_uri(app)?).await?;
    let claims = decode_claims(&id_token)?;
    if claims.aud != config.client_id {
        return Err(anyhow!("Google ID token is for another client ({})", claims.aud).into());
    }
    // The nonce sent with the authorize request is the state.
    if claims.nonce.as_deref() != Some(request.state.as_str()) {
        return Err(ApiError::BadRequest);
    }

    let trusted = trusted_email(&claims);
    let account = account::find_or_create_by_google(
        app.redis(),
        &claims.sub,
        trusted.as_deref(),
        claims.name.as_deref(),
    )
    .await?;
    let token = session::create(app.redis(), &account.id, session::SessionKind::Website).await?;
    Ok((token, account))
}

/// Google's sign-in page: `openid email profile`, with the state doubling as
/// the OpenID Connect `nonce` (checked in the ID token).
fn authorize_url(config: &GoogleConfig, state: &str, redirect_uri: &str) -> String {
    Url::parse_with_params(
        AUTHORIZE_URL,
        [
            ("response_type", "code"),
            ("client_id", config.client_id.as_str()),
            ("scope", "openid email profile"),
            ("state", state),
            ("nonce", state),
            ("redirect_uri", redirect_uri),
        ],
    )
    .expect("AUTHORIZE_URL is a valid URL")
    .into()
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
}

/// Exchanges an authorization code for the ID token. A code Google rejects
/// (`invalid_grant`: expired, reused, or a mismatched `redirect_uri`) is a
/// 401; anything else — including `invalid_client`, i.e. wrong server
/// credentials — is a logged 500.
async fn exchange_code(
    app: &WebApp,
    config: &GoogleConfig,
    code: &str,
    redirect_uri: &str,
) -> Result<String, ApiError> {
    let response = app
        .http()
        .post(TOKEN_URL)
        .form(&[
            ("code", code),
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .context("failed to reach Google token endpoint")?;

    let status = response.status();
    let body = response
        .text()
        .await
        .context("failed to read Google token response")?;

    if status.is_success() {
        let token: TokenResponse =
            serde_json::from_str(&body).context("unexpected Google token response")?;
        return Ok(token.id_token);
    }

    let error = serde_json::from_str::<TokenError>(&body).ok().map(|e| e.error);
    if error.as_deref() == Some("invalid_grant") {
        eprintln!("Google rejected login code: {body}");
        return Err(ApiError::Unauthorized);
    }
    Err(anyhow!("Google token exchange failed with status {status}: {body}").into())
}

/// The ID-token claims this framework uses.
#[derive(Debug, Deserialize)]
struct IdClaims {
    /// The client the token was issued to; must be ours.
    aud: String,
    /// Google's stable, never-reused user ID.
    sub: String,
    email: Option<String>,
    #[serde(default)]
    email_verified: bool,
    /// The Google Workspace domain; absent for consumer accounts.
    hd: Option<String>,
    name: Option<String>,
    nonce: Option<String>,
}

/// The claims in a JWT's payload (its middle, base64url-encoded part).
fn decode_claims(id_token: &str) -> anyhow::Result<IdClaims> {
    let payload = id_token.split('.').nth(1).context("Google ID token is not a JWT")?;
    let Some(json) = base64url_decode(payload) else {
        bail!("Google ID token payload is not base64url");
    };
    serde_json::from_slice(&json).context("unexpected Google ID token claims")
}

/// Decodes unpadded (or padded) base64url. Returns `None` on any character
/// outside the alphabet.
fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for byte in input.trim_end_matches('=').bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The user's normalized email address if Google is authoritative for it,
/// so it can be matched to (or claim) an `FE` email identity. Per Google,
/// that's a verified address that is either a Gmail address (`@gmail.com`)
/// or a Google Workspace account (`hd` present). For any other address,
/// `email_verified` may be stale ("ownership of the third party email account
/// may have since changed"), so it isn't trusted.
fn trusted_email(claims: &IdClaims) -> Option<String> {
    if !claims.email_verified {
        return None;
    }
    let email = normalize_email(claims.email.as_deref()?)?;
    let workspace = claims.hd.as_deref().is_some_and(|hd| !hd.trim().is_empty());
    (email.ends_with("@gmail.com") || workspace).then_some(email)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-only base64url encoder (unpadded), to build ID tokens.
    fn encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
            for i in 0..=chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
        }
        out
    }

    fn token(claims: &str) -> String {
        format!("{}.{}.signature", encode(br#"{"alg":"RS256"}"#), encode(claims.as_bytes()))
    }

    fn claims(json: &str) -> IdClaims {
        decode_claims(&token(json)).unwrap()
    }

    #[test]
    fn base64url_decodes_known_values() {
        assert_eq!(base64url_decode("").unwrap(), b"");
        assert_eq!(base64url_decode("Zg").unwrap(), b"f");
        assert_eq!(base64url_decode("Zm8").unwrap(), b"fo");
        assert_eq!(base64url_decode("Zm9v").unwrap(), b"foo");
        assert_eq!(base64url_decode("Zm9vYg==").unwrap(), b"foob");
        // `-` and `_` are base64url's 62 and 63 (`+` and `/` in plain base64).
        assert_eq!(base64url_decode("-_8").unwrap(), [0xfb, 0xff]);
        assert_eq!(base64url_decode("ab+c"), None);
        assert_eq!(base64url_decode("ab.c"), None);
        for sample in [&b"x"[..], b"xy", b"xyz", b"\x00\xff\x10\x80hello"] {
            assert_eq!(base64url_decode(&encode(sample)).unwrap(), sample);
        }
    }

    #[test]
    fn decodes_id_token_claims() {
        let parsed = claims(
            r#"{"iss":"https://accounts.google.com","aud":"client-1","sub":"1234567890",
                "email":"Jane@Gmail.com","email_verified":true,"name":"Jane Doe",
                "nonce":"google.abc","picture":"https://x","iat":1,"exp":2}"#,
        );
        assert_eq!(parsed.aud, "client-1");
        assert_eq!(parsed.sub, "1234567890");
        assert_eq!(parsed.name.as_deref(), Some("Jane Doe"));
        assert_eq!(parsed.nonce.as_deref(), Some("google.abc"));
        assert!(parsed.email_verified);
        assert_eq!(parsed.hd, None);
    }

    #[test]
    fn rejects_malformed_id_tokens() {
        assert!(decode_claims("not-a-jwt").is_err());
        assert!(decode_claims("a.!!!.c").is_err());
        assert!(decode_claims(&token(r#"{"aud":"x"}"#)).is_err(), "sub is required");
    }

    #[test]
    fn trusts_verified_gmail_addresses() {
        let gmail = claims(r#"{"aud":"a","sub":"1","email":" Jane@GMAIL.com ","email_verified":true}"#);
        assert_eq!(trusted_email(&gmail).as_deref(), Some("jane@gmail.com"));
    }

    #[test]
    fn trusts_verified_workspace_addresses() {
        let workspace = claims(
            r#"{"aud":"a","sub":"1","email":"jane@example.com","email_verified":true,"hd":"example.com"}"#,
        );
        assert_eq!(trusted_email(&workspace).as_deref(), Some("jane@example.com"));
    }

    #[test]
    fn does_not_trust_other_addresses() {
        // Verified, but neither Gmail nor Workspace: Google isn't authoritative.
        let other = claims(r#"{"aud":"a","sub":"1","email":"jane@example.com","email_verified":true}"#);
        assert_eq!(trusted_email(&other), None);
        // Unverified, even for Gmail or Workspace.
        let unverified = claims(r#"{"aud":"a","sub":"1","email":"jane@gmail.com","email_verified":false}"#);
        assert_eq!(trusted_email(&unverified), None);
        let missing_flag = claims(r#"{"aud":"a","sub":"1","email":"jane@gmail.com","hd":"x.com"}"#);
        assert_eq!(trusted_email(&missing_flag), None);
        // A look-alike domain isn't Gmail.
        let lookalike = claims(r#"{"aud":"a","sub":"1","email":"jane@notgmail.com","email_verified":true}"#);
        assert_eq!(trusted_email(&lookalike), None);
        let gmail_sub = claims(r#"{"aud":"a","sub":"1","email":"jane@gmail.com.evil.io","email_verified":true}"#);
        assert_eq!(trusted_email(&gmail_sub), None);
        // No email at all, or an empty hd.
        let none = claims(r#"{"aud":"a","sub":"1","email_verified":true,"hd":"x.com"}"#);
        assert_eq!(trusted_email(&none), None);
        let empty_hd = claims(r#"{"aud":"a","sub":"1","email":"j@x.com","email_verified":true,"hd":""}"#);
        assert_eq!(trusted_email(&empty_hd), None);
    }

    #[test]
    fn authorize_url_has_encoded_params() {
        let config = GoogleConfig {
            client_id: "client-1.apps.googleusercontent.com".to_string(),
            client_secret: "secret".to_string(),
        };
        let url = authorize_url(&config, "google.abc", "http://localhost:8080/");
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(parsed.as_str().split('?').next(), Some(AUTHORIZE_URL));
        let params: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
        let expected = [
            ("response_type", "code"),
            ("client_id", "client-1.apps.googleusercontent.com"),
            ("scope", "openid email profile"),
            ("state", "google.abc"),
            ("nonce", "google.abc"),
            ("redirect_uri", "http://localhost:8080/"),
        ];
        let expected: Vec<(String, String)> =
            expected.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        assert_eq!(params, expected);
        assert!(!url.contains("secret"));
    }
}
