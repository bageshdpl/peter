//! OpenID Connect sign-in: discovery, the authorization request, the code
//! exchange and checking the ID token. Peter is a confidential client using
//! the authorization code flow with PKCE.

use std::time::Duration;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::JwkSet};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// A message safe to show the person setting up or using sign-in.
pub type SsoResult<T> = Result<T, String>;

#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
}

/// What Peter reads from a verified ID token.
#[derive(Debug, Clone, Deserialize)]
pub struct Claims {
    pub sub: String,
    #[serde(default)]
    pub nonce: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    /// Microsoft Entra sends the sign-in name here, often without `email`.
    #[serde(default)]
    pub preferred_username: Option<String>,
    #[serde(default)]
    pub email_verified: Option<bool>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub auth_time: Option<i64>,
}

/// Signature algorithms accepted from a provider. Never `none` or a shared-secret one.
const ALGORITHMS: &[Algorithm] = &[
    Algorithm::RS256,
    Algorithm::RS384,
    Algorithm::RS512,
    Algorithm::PS256,
    Algorithm::PS384,
    Algorithm::PS512,
    Algorithm::ES256,
    Algorithm::ES384,
    Algorithm::EdDSA,
];

fn client() -> SsoResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        // A provider's endpoints are fixed addresses; following redirects would let one point Peter elsewhere.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not start a connection to the identity provider.".to_string())
}

/// Tidies an issuer address and refuses ones Peter should not call: anything
/// but https, and (unless `allow_local`) this machine or a bare network address.
pub fn clean_issuer(issuer: &str, allow_local: bool) -> SsoResult<String> {
    let issuer = issuer.trim().trim_end_matches('/');
    let url = url::Url::parse(issuer).map_err(|_| "Enter the provider's issuer address, starting with https://.".to_string())?;
    check_url(&url, allow_local)?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err("The issuer address cannot contain ? or #.".into());
    }
    Ok(issuer.to_string())
}

fn check_url(url: &url::Url, allow_local: bool) -> SsoResult<()> {
    if allow_local {
        return if matches!(url.scheme(), "https" | "http") { Ok(()) } else { Err("The address must start with https://.".into()) };
    }
    if url.scheme() != "https" {
        return Err("The address must start with https://.".into());
    }
    match url.host() {
        Some(url::Host::Domain(d)) if d != "localhost" && !d.ends_with(".localhost") && d.contains('.') => Ok(()),
        _ => Err("The address must be a public host name, not a network address or this machine.".into()),
    }
}

pub async fn discover(issuer: &str, allow_local: bool) -> SsoResult<Discovery> {
    let url = format!("{issuer}/.well-known/openid-configuration");
    let res = client()?.get(&url).send().await.map_err(|_| format!("Could not reach {url}."))?;
    if !res.status().is_success() {
        return Err(format!("{url} answered {}. Check the issuer address.", res.status().as_u16()));
    }
    let d: Discovery = res.json().await.map_err(|_| format!("{url} is not an OpenID Connect configuration."))?;
    if d.issuer.trim_end_matches('/') != issuer {
        return Err(format!("The provider says its issuer is {}, not {issuer}. Use the address exactly as the provider gives it.", d.issuer));
    }
    for endpoint in [&d.authorization_endpoint, &d.token_endpoint, &d.jwks_uri] {
        let u = url::Url::parse(endpoint).map_err(|_| "The provider's configuration has an invalid address.".to_string())?;
        check_url(&u, allow_local)?;
    }
    Ok(d)
}

async fn keys(d: &Discovery) -> SsoResult<JwkSet> {
    let res = client()?.get(&d.jwks_uri).send().await.map_err(|_| "Could not fetch the provider's signing keys.".to_string())?;
    if !res.status().is_success() {
        return Err("The provider did not return its signing keys.".into());
    }
    res.json().await.map_err(|_| "The provider's signing keys could not be read.".to_string())
}

/// Checks the issuer answers and publishes usable signing keys.
pub async fn check(issuer: &str, allow_local: bool) -> SsoResult<Discovery> {
    let d = discover(issuer, allow_local).await?;
    if keys(&d).await?.keys.is_empty() {
        return Err("The provider publishes no signing keys.".into());
    }
    Ok(d)
}

/// The PKCE challenge for a verifier (S256).
pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub struct AuthRequest<'a> {
    pub client_id: &'a str,
    pub redirect_uri: &'a str,
    pub state: &'a str,
    pub nonce: &'a str,
    pub code_verifier: &'a str,
    /// The person's email, to skip the provider's account chooser.
    pub login_hint: Option<&'a str>,
    /// Ask the provider to make the person sign in again (for signatures).
    pub fresh: bool,
}

pub fn authorization_url(d: &Discovery, r: &AuthRequest) -> SsoResult<String> {
    let mut url = url::Url::parse(&d.authorization_endpoint).map_err(|_| "The provider's sign-in address is invalid.".to_string())?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", r.client_id)
            .append_pair("redirect_uri", r.redirect_uri)
            .append_pair("scope", "openid email profile")
            .append_pair("state", r.state)
            .append_pair("nonce", r.nonce)
            .append_pair("code_challenge", &challenge(r.code_verifier))
            .append_pair("code_challenge_method", "S256");
        if let Some(hint) = r.login_hint {
            q.append_pair("login_hint", hint);
        }
        if r.fresh {
            q.append_pair("prompt", "login").append_pair("max_age", "0");
        }
    }
    Ok(url.into())
}

pub struct Exchange<'a> {
    pub issuer: &'a str,
    pub client_id: &'a str,
    pub client_secret: &'a str,
    pub redirect_uri: &'a str,
    pub code: &'a str,
    pub code_verifier: &'a str,
    pub nonce: &'a str,
}

/// Trades the code for an ID token and verifies it: signature, issuer,
/// audience, expiry and the nonce of this sign-in.
pub async fn exchange(d: &Discovery, x: &Exchange<'_>) -> SsoResult<Claims> {
    #[derive(Deserialize)]
    struct TokenResponse {
        id_token: Option<String>,
    }
    let res = client()?
        .post(&d.token_endpoint)
        .basic_auth(x.client_id, Some(x.client_secret))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", x.code),
            ("redirect_uri", x.redirect_uri),
            ("client_id", x.client_id),
            ("code_verifier", x.code_verifier),
        ])
        .send()
        .await
        .map_err(|_| "Could not reach the identity provider to finish signing in.".to_string())?;
    if !res.status().is_success() {
        let status = res.status().as_u16();
        tracing::warn!(status, "the identity provider refused the code exchange");
        return Err(format!(
            "The identity provider refused to finish the sign-in (it answered {status}). An administrator should check the client ID, secret and redirect address."
        ));
    }
    let token: TokenResponse = res.json().await.map_err(|_| "The identity provider's answer could not be read.".to_string())?;
    let id_token = token.id_token.ok_or("The identity provider did not return an ID token.")?;
    let claims = verify(&id_token, &keys(d).await?, x.issuer, x.client_id)?;
    if claims.nonce.as_deref() != Some(x.nonce) {
        return Err("The identity provider's answer does not belong to this sign-in. Try again.".into());
    }
    Ok(claims)
}

pub fn verify(id_token: &str, keys: &JwkSet, issuer: &str, client_id: &str) -> SsoResult<Claims> {
    let invalid = |why: &str| {
        tracing::warn!(why, "rejected an ID token");
        "The identity provider's answer could not be verified.".to_string()
    };
    let header = jsonwebtoken::decode_header(id_token).map_err(|_| invalid("unreadable header"))?;
    if !ALGORITHMS.contains(&header.alg) {
        return Err(invalid("algorithm not accepted"));
    }
    let jwk = match &header.kid {
        Some(kid) => keys.find(kid),
        // Without a key id, only an unambiguous single key is acceptable.
        None if keys.keys.len() == 1 => keys.keys.first(),
        None => None,
    }
    .ok_or_else(|| invalid("no matching signing key"))?;
    let key = DecodingKey::from_jwk(jwk).map_err(|_| invalid("unusable signing key"))?;
    let mut validation = Validation::new(header.alg);
    validation.set_audience(&[client_id]);
    // Providers differ on a trailing slash.
    validation.set_issuer(&[issuer.to_string(), format!("{issuer}/")]);
    validation.set_required_spec_claims(&["exp", "aud", "iss", "sub"]);
    validation.leeway = 60;
    jsonwebtoken::decode::<Claims>(id_token, &key, &validation).map(|t| t.claims).map_err(|e| invalid(&e.to_string()))
}

impl Claims {
    /// The email to sign in as, lower-cased; refused if the provider says it is unverified.
    pub fn sign_in_email(&self) -> SsoResult<String> {
        if self.email_verified == Some(false) {
            return Err("Your identity provider has not verified your email address.".into());
        }
        let candidate = self.email.as_deref().or(self.preferred_username.as_deref()).unwrap_or("");
        crate::security::normalize_email(candidate)
            .map_err(|_| "Your identity provider did not send an email address. An administrator should add the email claim.".to_string())
    }
}

/// Whether an email belongs to one of the allowed domains (an empty list allows any).
pub fn domain_allowed(email: &str, domains: &str) -> bool {
    let allowed: Vec<&str> = domains.split(',').map(str::trim).filter(|d| !d.is_empty()).collect();
    let domain = email.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
    allowed.is_empty() || allowed.iter().any(|d| d.eq_ignore_ascii_case(domain))
}

/// Tidies a list of domains typed by an administrator.
pub fn clean_domains(input: &str) -> SsoResult<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in input.split([',', ' ', '\n', ';']) {
        let d = raw.trim().trim_start_matches('@').to_lowercase();
        if d.is_empty() {
            continue;
        }
        let ok = d.len() <= 253 && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
            && d.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if !ok {
            return Err(format!("\"{raw}\" is not an email domain. Write domains like example.com."));
        }
        if !out.contains(&d) {
            out.push(d);
        }
    }
    if out.len() > 20 {
        return Err("List at most 20 domains.".into());
    }
    Ok(out.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issuers_must_be_public_https_hosts() {
        assert_eq!(clean_issuer(" https://login.example.com/tenant/v2.0/ ", false).unwrap(), "https://login.example.com/tenant/v2.0");
        for bad in ["http://login.example.com", "https://localhost", "https://127.0.0.1", "https://[::1]", "https://intranet",
            "ftp://x.example.com", "https://x.example.com?a=1", "not a url"] {
            assert!(clean_issuer(bad, false).is_err(), "{bad}");
        }
        assert!(clean_issuer("http://127.0.0.1:9000", true).is_ok());
    }

    #[test]
    fn domains_are_tidied_and_matched() {
        assert_eq!(clean_domains("@Example.com, acme.co.uk example.com").unwrap(), "example.com,acme.co.uk");
        assert!(clean_domains("not a domain!").is_err());
        assert!(domain_allowed("ana@example.com", ""));
        assert!(domain_allowed("ana@EXAMPLE.com", "example.com,acme.co.uk"));
        assert!(!domain_allowed("ana@evil-example.com", "example.com"));
        assert!(!domain_allowed("ana@sub.example.com", "example.com"));
    }

    #[test]
    fn pkce_challenge_matches_the_rfc_example() {
        // RFC 7636, appendix B.
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }
}
