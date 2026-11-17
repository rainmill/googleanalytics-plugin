//! Google's side: a service account's access token and the Data API calls.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use laterite_core::serde_json::{self, json, Value};
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::sha2::Sha256;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::RsaPrivateKey;
use serde::Deserialize;

/// The scope the plugin asks for: reading reports only.
const SCOPE: &str = "https://www.googleapis.com/auth/analytics.readonly";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const DATA_API: &str = "https://analyticsdata.googleapis.com/v1beta";

/// What went wrong talking to Google.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct GoogleError(pub String);

/// The parts of a service account key file the plugin reads.
#[derive(Deserialize)]
pub struct ServiceAccount {
    /// The account's email, which Google Analytics grants access to.
    pub client_email: String,
    private_key: String,
    #[serde(default)]
    token_uri: Option<String>,
}

impl ServiceAccount {
    /// Reads a key file Google Cloud downloaded.
    pub fn parse(json: &[u8]) -> Result<Self, GoogleError> {
        let account: Self = serde_json::from_slice(json)
            .map_err(|_| GoogleError("the key file is not a Google service account key".into()))?;
        if account.client_email.is_empty() || !account.private_key.contains("PRIVATE KEY") {
            return Err(GoogleError(
                "the key file has no service account email or private key".into(),
            ));
        }
        Ok(account)
    }

    /// A signed request for an access token, valid for ten minutes.
    fn assertion(&self, now: i64) -> Result<String, GoogleError> {
        let key = RsaPrivateKey::from_pkcs8_pem(&self.private_key)
            .map_err(|_| GoogleError("the key file's private key cannot be read".into()))?;
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let claims = URL_SAFE_NO_PAD.encode(
            json!({
                "iss": self.client_email,
                "scope": SCOPE,
                "aud": self.token_uri.as_deref().unwrap_or(TOKEN_URL),
                "iat": now,
                "exp": now + 600,
            })
            .to_string(),
        );
        let signing = format!("{header}.{claims}");
        let signature = SigningKey::<Sha256>::new(key).sign(signing.as_bytes());
        Ok(format!(
            "{signing}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        ))
    }
}

/// A client for one property.
pub struct Analytics {
    account: ServiceAccount,
    property: String,
    http: reqwest::Client,
}

/// Tokens by service account, reused until a minute before they lapse.
fn tokens() -> &'static Mutex<Vec<(String, String, Instant)>> {
    static TOKENS: OnceLock<Mutex<Vec<(String, String, Instant)>>> = OnceLock::new();
    TOKENS.get_or_init(|| Mutex::new(Vec::new()))
}

impl Analytics {
    /// A client for `property` (the numeric ID) as `account`.
    pub fn new(account: ServiceAccount, property: &str) -> Result<Self, GoogleError> {
        let property = property
            .trim()
            .trim_start_matches("properties/")
            .to_string();
        if property.is_empty() || !property.chars().all(|c| c.is_ascii_digit()) {
            return Err(GoogleError(
                "the property ID is the number on the property's settings page".into(),
            ));
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| GoogleError(e.to_string()))?;
        Ok(Self {
            account,
            property,
            http,
        })
    }

    /// The service account's email.
    pub fn account(&self) -> &str {
        &self.account.client_email
    }

    /// An access token, from memory or newly asked of Google.
    pub async fn token(&self) -> Result<String, GoogleError> {
        let email = self.account.client_email.clone();
        {
            let held = tokens().lock().unwrap_or_else(|e| e.into_inner());
            if let Some((_, token, until)) = held.iter().find(|(e, _, _)| *e == email) {
                if Instant::now() < *until {
                    return Ok(token.clone());
                }
            }
        }
        let assertion = self.account.assertion(chrono::Utc::now().timestamp())?;
        let response = self
            .http
            .post(self.account.token_uri.as_deref().unwrap_or(TOKEN_URL))
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .await
            .map_err(|e| GoogleError(format!("Google could not be reached: {e}")))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        let Some(token) = body["access_token"].as_str() else {
            let reason = body["error_description"]
                .as_str()
                .or(body["error"].as_str())
                .unwrap_or("no reason given");
            return Err(GoogleError(format!(
                "Google refused the key ({status}): {reason}"
            )));
        };
        let lasts = body["expires_in"]
            .as_u64()
            .unwrap_or(3600)
            .saturating_sub(60);
        let mut held = tokens().lock().unwrap_or_else(|e| e.into_inner());
        held.retain(|(e, _, _)| *e != email);
        held.push((
            email,
            token.to_string(),
            Instant::now() + Duration::from_secs(lasts),
        ));
        Ok(token.to_string())
    }

    async fn call(&self, path: &str, body: Option<&Value>) -> Result<Value, GoogleError> {
        let token = self.token().await?;
        let url = format!("{DATA_API}/properties/{}{path}", self.property);
        let request = match body {
            Some(b) => self.http.post(url).json(b),
            None => self.http.get(url),
        };
        let response = request
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| GoogleError(format!("Google could not be reached: {e}")))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let message = body["error"]["message"]
                .as_str()
                .unwrap_or("no reason given");
            return Err(GoogleError(format!(
                "Google Analytics answered {status}: {message}"
            )));
        }
        Ok(body)
    }

    /// The property's dimensions and metrics, its custom ones included.
    pub async fn metadata(&self) -> Result<Value, GoogleError> {
        self.call("/metadata", None).await
    }

    /// Runs a report; `body` is a `runReport` request without the property.
    pub async fn run_report(&self, body: &Value) -> Result<Value, GoogleError> {
        self.call(":runReport", Some(body)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_file_needs_an_email_and_a_private_key() {
        assert!(ServiceAccount::parse(b"{}").is_err());
        assert!(ServiceAccount::parse(b"not json").is_err());
        let err = ServiceAccount::parse(
            br#"{"client_email":"a@b.iam.gserviceaccount.com","private_key":"x"}"#,
        )
        .err()
        .unwrap();
        assert!(err.0.contains("private key"));
    }

    #[test]
    fn a_property_is_its_number() {
        let account = || ServiceAccount {
            client_email: "a@b".into(),
            private_key: "-----BEGIN PRIVATE KEY-----".into(),
            token_uri: None,
        };
        assert_eq!(
            Analytics::new(account(), "properties/564441847")
                .unwrap()
                .property,
            "564441847"
        );
        assert!(Analytics::new(account(), "G-7ZHVWW03R8").is_err());
    }
}
