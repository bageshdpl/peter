//! Single sign-on with OpenID Connect: the workspace's settings, starting a
//! sign-in at the identity provider, and finishing it when the person returns.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Body, session_view};
use crate::{
    auth::{Auth, audit, mark_sso, start_session, user_agent},
    error::{AppError, AppResult},
    security::{Role, hash_token, new_id, new_token, normalize_email, now},
    sso,
    state::AppState,
};

/// How long a person has to finish at the identity provider.
const LOGIN_TTL_SECS: i64 = 10 * 60;
/// How long a signature confirmation stays usable once the person is back.
const SIGN_TICKET_SECS: i64 = 5 * 60;
/// How recent the provider's sign-in must be to confirm a signature.
const SIGN_MAX_AGE_SECS: i64 = 10 * 60;
/// Stored in place of a password hash for accounts made by single sign-on. It
/// never verifies; such a person sets a password with "Forgot password".
pub const NO_PASSWORD: &str = "!sso";

#[derive(sqlx::FromRow)]
struct Settings {
    issuer: String,
    client_id: String,
    client_secret: String,
    enabled: bool,
    enforce: bool,
    auto_join: bool,
    default_role: String,
    domains: String,
}

async fn load(state: &AppState, workspace_id: &str) -> AppResult<Option<Settings>> {
    Ok(sqlx::query_as(
        "SELECT issuer, client_id, client_secret, enabled, enforce, auto_join, default_role, domains
         FROM workspace_sso WHERE workspace_id = ?",
    )
    .bind(workspace_id)
    .fetch_optional(&state.db)
    .await?)
}

fn redirect_uri(state: &AppState) -> String {
    format!("{}/sso/callback", state.config.app_url.trim_end_matches('/'))
}

fn view(state: &AppState, workspace_id: &str, s: Option<&Settings>) -> Value {
    json!({
        "configured": s.is_some(),
        "issuer": s.map(|s| s.issuer.as_str()).unwrap_or(""),
        "client_id": s.map(|s| s.client_id.as_str()).unwrap_or(""),
        "enabled": s.is_some_and(|s| s.enabled),
        "enforce": s.is_some_and(|s| s.enforce),
        "auto_join": s.is_none_or(|s| s.auto_join),
        "default_role": s.map(|s| s.default_role.as_str()).unwrap_or("member"),
        "domains": s.map(|s| s.domains.split(',').filter(|d| !d.is_empty()).collect::<Vec<_>>()).unwrap_or_default(),
        // What the administrator registers at the identity provider, and gives to colleagues.
        "redirect_uri": redirect_uri(state),
        "sign_in_link": format!("{}/sso/{workspace_id}", state.config.app_url.trim_end_matches('/')),
    })
}

pub async fn get(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    auth.require(Role::Admin)?;
    let s = load(&state, &auth.workspace_id).await?;
    Ok(Json(view(&state, &auth.workspace_id, s.as_ref())))
}

#[derive(Deserialize)]
pub struct SetBody {
    issuer: String,
    client_id: String,
    /// Left out to keep the saved secret.
    client_secret: Option<String>,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    enforce: bool,
    #[serde(default = "yes")]
    auto_join: bool,
    #[serde(default = "member")]
    default_role: String,
    #[serde(default)]
    domains: String,
}

fn yes() -> bool {
    true
}
fn member() -> String {
    "member".into()
}

fn invalid(message: String) -> AppError {
    AppError::bad_request("invalid_sso", message)
}

pub async fn set(State(state): State<AppState>, auth: Auth, Body(body): Body<SetBody>) -> AppResult<Json<Value>> {
    auth.require(Role::Admin)?;
    let issuer = sso::clean_issuer(&body.issuer, state.config.sso_allow_local).map_err(invalid)?;
    let client_id = body.client_id.trim().to_string();
    if client_id.is_empty() || client_id.len() > 300 {
        return Err(invalid("Enter the client ID the identity provider gave you.".into()));
    }
    if !["member", "viewer"].contains(&body.default_role.as_str()) {
        return Err(invalid("New people can join as member or viewer.".into()));
    }
    if body.enforce && !body.enabled {
        return Err(invalid("Turn single sign-on on before requiring it.".into()));
    }
    let domains = sso::clean_domains(&body.domains).map_err(invalid)?;
    let before = load(&state, &auth.workspace_id).await?;
    let secret = match body.client_secret.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) if s.len() > 2000 => return Err(invalid("That client secret is too long.".into())),
        Some(s) => state.secret.encrypt(s).map_err(|_| AppError::internal())?,
        None => before
            .as_ref()
            .map(|b| b.client_secret.clone())
            .ok_or_else(|| invalid("Enter the client secret the identity provider gave you.".into()))?,
    };
    // Never switch on something that cannot work.
    if body.enabled {
        sso::check(&issuer, state.config.sso_allow_local).await.map_err(|m| AppError::bad_request("sso_check_failed", m))?;
    }
    // Requiring it must not lock out the administrator doing so: they must have proved it works by using it.
    if body.enforce && !before.as_ref().is_some_and(|b| b.enforce) && !auth.sso && auth.role != Role::Owner {
        return Err(AppError::conflict(
            "sso_untested",
            "Sign in with single sign-on yourself before requiring it, so you know it works.",
        ));
    }

    let mut tx = state.db.begin().await?;
    let t = now();
    sqlx::query(
        "INSERT INTO workspace_sso
         (workspace_id, issuer, client_id, client_secret, enabled, enforce, auto_join, default_role, domains, updated_by, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (workspace_id) DO UPDATE SET issuer = excluded.issuer, client_id = excluded.client_id,
           client_secret = excluded.client_secret, enabled = excluded.enabled, enforce = excluded.enforce,
           auto_join = excluded.auto_join, default_role = excluded.default_role, domains = excluded.domains,
           updated_by = excluded.updated_by, updated_at = excluded.updated_at",
    )
    .bind(&auth.workspace_id)
    .bind(&issuer)
    .bind(&client_id)
    .bind(&secret)
    .bind(body.enabled)
    .bind(body.enforce)
    .bind(body.auto_join)
    .bind(&body.default_role)
    .bind(&domains)
    .bind(&auth.user_id)
    .bind(t)
    .execute(&mut *tx)
    .await?;
    // A changed provider is a different authority: forget who its subjects were.
    if before.as_ref().is_some_and(|b| b.issuer != issuer || b.client_id != client_id) {
        sqlx::query("DELETE FROM sso_identities WHERE workspace_id = ?").bind(&auth.workspace_id).execute(&mut *tx).await?;
    }
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "workspace.sso_changed", Some(&auth.workspace_id),
        json!({ "issuer": issuer, "client_id": client_id, "secret_changed": body.client_secret.is_some(),
                "enabled": body.enabled, "enforce": body.enforce, "auto_join": body.auto_join,
                "default_role": body.default_role, "domains": domains })).await?;
    tx.commit().await?;
    let s = load(&state, &auth.workspace_id).await?;
    Ok(Json(view(&state, &auth.workspace_id, s.as_ref())))
}

pub async fn remove(State(state): State<AppState>, auth: Auth) -> AppResult<StatusCode> {
    auth.require(Role::Admin)?;
    let mut tx = state.db.begin().await?;
    let gone = sqlx::query("DELETE FROM workspace_sso WHERE workspace_id = ?").bind(&auth.workspace_id).execute(&mut *tx).await?;
    if gone.rows_affected() > 0 {
        sqlx::query("DELETE FROM sso_identities WHERE workspace_id = ?").bind(&auth.workspace_id).execute(&mut *tx).await?;
        audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "workspace.sso_removed", Some(&auth.workspace_id), json!({})).await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Tries the saved issuer: it answers, describes itself correctly and publishes signing keys.
pub async fn check(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    auth.require(Role::Admin)?;
    let s = load(&state, &auth.workspace_id).await?.ok_or_else(|| AppError::not_found("Save the single sign-on settings first."))?;
    Ok(Json(match sso::check(&s.issuer, state.config.sso_allow_local).await {
        Ok(d) => json!({ "ok": true, "message": format!("The provider answered. People will sign in at {}.", host_of(&d.authorization_endpoint)) }),
        Err(message) => json!({ "ok": false, "message": message }),
    }))
}

fn host_of(address: &str) -> String {
    url::Url::parse(address).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| address.to_string())
}

fn unavailable() -> AppError {
    AppError::not_found("Single sign-on is not set up for this. Use your workspace's sign-in link, or your email and password.")
}

/// Records a sign-in in progress and returns where to send the person.
async fn begin(
    state: &AppState,
    workspace_id: &str,
    s: &Settings,
    purpose: &str,
    user_id: Option<&str>,
    login_hint: Option<&str>,
) -> AppResult<Json<Value>> {
    let d = sso::discover(&s.issuer, state.config.sso_allow_local).await.map_err(|m| {
        tracing::warn!(workspace_id, error = %m, "single sign-on discovery failed");
        AppError::new(StatusCode::BAD_GATEWAY, "sso_unreachable", "The identity provider could not be reached. Try again, or ask an administrator.")
    })?;
    let (sso_state, nonce, verifier) = (new_token(), new_token(), new_token());
    let redirect = redirect_uri(state);
    let url = sso::authorization_url(&d, &sso::AuthRequest {
        client_id: &s.client_id,
        redirect_uri: &redirect,
        state: &sso_state,
        nonce: &nonce,
        code_verifier: &verifier,
        login_hint,
        fresh: purpose == "sign",
    })
    .map_err(|_| AppError::internal())?;
    let t = now();
    // Old attempts are of no use to anyone.
    sqlx::query("DELETE FROM sso_logins WHERE expires_at < ?").bind(t - 3600).execute(&state.db).await?;
    sqlx::query(
        "INSERT INTO sso_logins (id, state_hash, workspace_id, nonce, code_verifier, purpose, user_id, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(hash_token(&sso_state))
    .bind(workspace_id)
    .bind(&nonce)
    .bind(&verifier)
    .bind(purpose)
    .bind(user_id)
    .bind(t)
    .bind(t + LOGIN_TTL_SECS)
    .execute(&state.db)
    .await?;
    Ok(Json(json!({ "url": url })))
}

#[derive(Deserialize)]
pub struct StartBody {
    /// From the workspace's sign-in link.
    workspace_id: Option<String>,
    /// Or the person's email: Peter finds the workspace they belong to that uses single sign-on.
    email: Option<String>,
}

pub async fn start(State(state): State<AppState>, Body(body): Body<StartBody>) -> AppResult<Json<Value>> {
    let (workspace_id, hint) = match (&body.workspace_id, &body.email) {
        (Some(id), _) => (id.clone(), None),
        (None, Some(email)) => {
            let email = normalize_email(email)?;
            let found: Option<(String,)> = sqlx::query_as(
                "SELECT m.workspace_id FROM users u
                 JOIN memberships m ON m.user_id = u.id
                 JOIN workspace_sso s ON s.workspace_id = m.workspace_id AND s.enabled = 1
                 LEFT JOIN sessions se ON se.workspace_id = m.workspace_id AND se.user_id = u.id
                 WHERE u.email = ? GROUP BY m.workspace_id
                 ORDER BY MAX(COALESCE(se.created_at, 0)) DESC, MIN(m.created_at) LIMIT 1",
            )
            .bind(&email)
            .fetch_optional(&state.db)
            .await?;
            (found.ok_or_else(unavailable)?.0, Some(email))
        }
        (None, None) => return Err(AppError::bad_request("invalid_body", "Enter your work email.")),
    };
    let s = load(&state, &workspace_id).await?.filter(|s| s.enabled).ok_or_else(unavailable)?;
    begin(&state, &workspace_id, &s, "login", None, hint.as_deref()).await
}

/// Sends a signed-in person back to the identity provider to confirm a signature.
pub async fn sign_start(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    auth.require(Role::Member)?;
    let s = load(&state, &auth.workspace_id).await?.filter(|s| s.enabled).ok_or_else(unavailable)?;
    let (email,): (String,) = sqlx::query_as("SELECT email FROM users WHERE id = ?").bind(&auth.user_id).fetch_one(&state.db).await?;
    begin(&state, &auth.workspace_id, &s, "sign", Some(&auth.user_id), Some(&email)).await
}

#[derive(Deserialize)]
pub struct CallbackBody {
    code: String,
    state: String,
}

fn refused(message: impl Into<String>) -> AppError {
    AppError::new(StatusCode::FORBIDDEN, "sso_refused", message)
}

pub async fn callback(State(state): State<AppState>, headers: HeaderMap, Body(body): Body<CallbackBody>) -> AppResult<Json<Value>> {
    let expired = || AppError::bad_request("sso_expired", "This sign-in took too long or was already used. Start again.");
    // Claim the attempt first, so a code and state can be used once only.
    let t = now();
    let row: Option<(String, String, String, String, Option<String>)> = sqlx::query_as(
        "UPDATE sso_logins SET used_at = ? WHERE state_hash = ? AND used_at IS NULL AND expires_at > ?
         RETURNING workspace_id, nonce, code_verifier, purpose, user_id",
    )
    .bind(t)
    .bind(hash_token(&body.state))
    .bind(t)
    .fetch_optional(&state.db)
    .await?;
    let (workspace_id, nonce, verifier, purpose, expected_user) = row.ok_or_else(expired)?;
    let s = load(&state, &workspace_id).await?.filter(|s| s.enabled).ok_or_else(unavailable)?;
    let secret = state.secret.decrypt(&s.client_secret).map_err(|why| {
        tracing::error!(workspace_id, why, "the single sign-on client secret cannot be read");
        AppError::new(StatusCode::CONFLICT, "sso_secret_unreadable", "The saved client secret can no longer be read. An administrator must enter it again.")
    })?;
    let failed = |m: String| AppError::new(StatusCode::BAD_GATEWAY, "sso_failed", m);
    let d = sso::discover(&s.issuer, state.config.sso_allow_local).await.map_err(failed)?;
    let claims = sso::exchange(&d, &sso::Exchange {
        issuer: &s.issuer,
        client_id: &s.client_id,
        client_secret: &secret,
        redirect_uri: &redirect_uri(&state),
        code: &body.code,
        code_verifier: &verifier,
        nonce: &nonce,
    })
    .await
    .map_err(failed)?;
    let email = claims.sign_in_email().map_err(refused)?;
    if !sso::domain_allowed(&email, &s.domains) {
        return Err(refused("Your email address is not from a domain this workspace accepts."));
    }

    let mut tx = state.db.begin().await?;
    let bound: Option<(String,)> = sqlx::query_as("SELECT user_id FROM sso_identities WHERE workspace_id = ? AND subject = ?")
        .bind(&workspace_id)
        .bind(&claims.sub)
        .fetch_optional(&mut *tx)
        .await?;

    if purpose == "sign" {
        // The person confirming must be the one who asked, by the provider's own id or by email.
        let by_email: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE email = ?").bind(&email).fetch_optional(&mut *tx).await?;
        let who = bound.or(by_email).map(|(id,)| id);
        if who.is_none() || who != expected_user {
            return Err(refused("A different person signed in at the identity provider. The signature was not confirmed."));
        }
        if claims.auth_time.is_some_and(|at| at < now() - SIGN_MAX_AGE_SECS) {
            return Err(refused("The identity provider did not ask you to sign in again. The signature was not confirmed."));
        }
        let ticket = new_token();
        sqlx::query("INSERT INTO sso_sign_tickets (token_hash, user_id, workspace_id, expires_at) VALUES (?, ?, ?, ?)")
            .bind(hash_token(&ticket))
            .bind(who.as_deref())
            .bind(&workspace_id)
            .bind(now() + SIGN_TICKET_SECS)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(Json(json!({ "purpose": "sign", "sign_token": ticket, "expires_in": SIGN_TICKET_SECS })));
    }

    // Who this is: someone the provider has signed in before, else the account with this email, else a new account.
    let (user_id, created) = match bound {
        Some((id,)) => (id, false),
        None => {
            let existing: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE email = ?").bind(&email).fetch_optional(&mut *tx).await?;
            let (id, created) = match existing {
                Some((id,)) => (id, false),
                None if s.auto_join => {
                    let id = new_id();
                    let name: String = claims.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&email).chars().take(120).collect();
                    sqlx::query("INSERT INTO users (id, email, name, password_hash, created_at) VALUES (?, ?, ?, ?, ?)")
                        .bind(&id)
                        .bind(&email)
                        .bind(&name)
                        .bind(NO_PASSWORD)
                        .bind(now())
                        .execute(&mut *tx)
                        .await?;
                    (id, true)
                }
                None => return Err(refused("You do not have a Peter account in this workspace. Ask an administrator to invite you.")),
            };
            sqlx::query("INSERT INTO sso_identities (workspace_id, subject, user_id, created_at) VALUES (?, ?, ?, ?)")
                .bind(&workspace_id)
                .bind(&claims.sub)
                .bind(&id)
                .bind(now())
                .execute(&mut *tx)
                .await?;
            (id, created)
        }
    };
    let member: Option<(String,)> = sqlx::query_as("SELECT role FROM memberships WHERE workspace_id = ? AND user_id = ?")
        .bind(&workspace_id)
        .bind(&user_id)
        .fetch_optional(&mut *tx)
        .await?;
    if member.is_none() {
        if !s.auto_join {
            return Err(refused("You are not a member of this workspace. Ask an administrator to invite you."));
        }
        sqlx::query("INSERT INTO memberships (workspace_id, user_id, role, created_at) VALUES (?, ?, ?, ?)")
            .bind(&workspace_id)
            .bind(&user_id)
            .bind(&s.default_role)
            .bind(now())
            .execute(&mut *tx)
            .await?;
        audit(&mut tx, Some(&workspace_id), Some(&user_id), "member.joined_by_sso", Some(&user_id),
            json!({ "email": email, "role": s.default_role, "account_created": created })).await?;
    }
    let tokens = start_session(&mut tx, &state, &user_id, &workspace_id, &user_agent(&headers)).await?;
    mark_sso(&mut tx, &tokens).await?;
    audit(&mut tx, Some(&workspace_id), Some(&user_id), "auth.login", None, json!({ "sso": true })).await?;
    let mut session = session_view(&mut tx, tokens, &user_id, &workspace_id).await?;
    (session.sso, session.mfa_setup_required) = (true, false);
    tx.commit().await?;
    let mut out = serde_json::to_value(session).map_err(|_| AppError::internal())?;
    out["purpose"] = json!("login");
    Ok(Json(out))
}

/// Spends a signature confirmation from the identity provider. True if it was valid for this person and workspace.
pub async fn spend_sign_ticket(conn: &mut sqlx::SqliteConnection, token: &str, user_id: &str, workspace_id: &str) -> AppResult<bool> {
    let spent = sqlx::query(
        "UPDATE sso_sign_tickets SET used_at = ? WHERE token_hash = ? AND user_id = ? AND workspace_id = ? AND used_at IS NULL AND expires_at > ?",
    )
    .bind(now())
    .bind(hash_token(token))
    .bind(user_id)
    .bind(workspace_id)
    .bind(now())
    .execute(&mut *conn)
    .await?;
    Ok(spent.rows_affected() == 1)
}
