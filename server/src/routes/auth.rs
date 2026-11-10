use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Body, SessionView, clean_name, session_view, user_workspaces};
use crate::{
    auth::{Auth, audit, revoke_user_sessions, rotate_session, sso_enforced_for, start_session, user_agent},
    mfa, throttle,
    error::{AppError, AppResult},
    security::{Role, check_new_password, hash_token, new_id, new_token, normalize_email, now},
    state::AppState,
};

/// Argon2 is deliberately slow, so it runs off the async worker threads.
pub async fn hash_password(password: String) -> AppResult<String> {
    tokio::task::spawn_blocking(move || crate::security::hash_password(&password))
        .await
        .map_err(|_| AppError::internal())?
}

pub async fn verify_password(password: String, hash: String) -> AppResult<bool> {
    tokio::task::spawn_blocking(move || crate::security::verify_password(&password, &hash))
        .await
        .map_err(|_| AppError::internal())
}

#[derive(Deserialize)]
pub struct SignupBody {
    email: String,
    password: String,
    name: String,
    workspace_name: String,
}

pub async fn signup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Body(body): Body<SignupBody>,
) -> AppResult<(StatusCode, Json<SessionView>)> {
    let email = normalize_email(&body.email)?;
    if !state.config.allow_signup {
        // Invite-only: the configured admin may create the first account, nobody else.
        let (users,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users").fetch_one(&state.db).await?;
        let bootstrap = users == 0 && state.config.admin_email.as_deref() == Some(email.as_str());
        if !bootstrap {
            return Err(AppError::forbidden("This Peter is invite-only. Ask an administrator for an invite."));
        }
    }
    let name = clean_name(&body.name, "name")?;
    let workspace_name = clean_name(&body.workspace_name, "workspace name")?;
    check_new_password(&body.password)?;
    let password_hash = hash_password(body.password).await?;

    let mut tx = state.db.begin().await?;
    let taken: Option<(String,)> =
        sqlx::query_as("SELECT id FROM users WHERE email = ?").bind(&email).fetch_optional(&mut *tx).await?;
    if taken.is_some() {
        return Err(AppError::conflict("email_taken", "An account with this email already exists. Sign in instead."));
    }

    let (user_id, workspace_id, t) = (new_id(), new_id(), now());
    sqlx::query("INSERT INTO users (id, email, name, password_hash, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&user_id)
        .bind(&email)
        .bind(&name)
        .bind(&password_hash)
        .bind(t)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, name, created_at) VALUES (?, ?, ?)")
        .bind(&workspace_id)
        .bind(&workspace_name)
        .bind(t)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memberships (workspace_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?)")
        .bind(&workspace_id)
        .bind(&user_id)
        .bind(t)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, Some(&workspace_id), Some(&user_id), "workspace.created", Some(&workspace_id),
        json!({ "name": workspace_name, "by_signup": true })).await?;

    let tokens = start_session(&mut tx, &state, &user_id, &workspace_id, &user_agent(&headers)).await?;
    let view = session_view(&mut tx, tokens, &user_id, &workspace_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(view)))
}

#[derive(Deserialize)]
pub struct LoginBody {
    email: String,
    password: String,
    workspace_id: Option<String>,
}

pub async fn login(State(state): State<AppState>, headers: HeaderMap, Body(body): Body<LoginBody>) -> AppResult<Json<Value>> {
    let invalid = || AppError::unauthorized("The email or password is not right.");
    let email = normalize_email(&body.email).map_err(|_| invalid())?;
    let key = throttle::email_key(&email);
    throttle::check(&state.db, &key).await?;

    let user: Option<(String, String)> = sqlx::query_as("SELECT id, password_hash FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    let correct = match &user {
        Some((_, stored)) => verify_password(body.password, stored.clone()).await?,
        None => {
            // Same work as a real check, so timing does not reveal unknown emails.
            verify_password(body.password, state.dummy_hash.to_string()).await?;
            false
        }
    };
    let Some((user_id, _)) = user.filter(|_| correct) else {
        return Err(throttle::fail(&state.db, &key).await?.unwrap_or_else(invalid));
    };
    throttle::clear(&state.db, &key).await?;

    let mut tx = state.db.begin().await?;
    let asked = body.workspace_id.is_some();
    let mut workspace_id = choose_workspace(&mut tx, &user_id, body.workspace_id).await?;
    // A workspace that requires single sign-on does not take passwords. If the
    // person did not ask for it by name, use another of their workspaces.
    let role = ensure_member(&mut tx, &user_id, &workspace_id).await?;
    if sso_enforced_for(&mut tx, &workspace_id, role).await? {
        let other: Option<(String,)> = if asked {
            None
        } else {
            sqlx::query_as(
                "SELECT m.workspace_id FROM memberships m
                 LEFT JOIN workspace_sso s ON s.workspace_id = m.workspace_id AND s.enabled = 1 AND s.enforce = 1
                 WHERE m.user_id = ? AND (s.workspace_id IS NULL OR m.role = 'owner') ORDER BY m.created_at LIMIT 1",
            )
            .bind(&user_id)
            .fetch_optional(&mut *tx)
            .await?
        };
        workspace_id = other.map(|(id,)| id).ok_or_else(|| {
            AppError::new(StatusCode::FORBIDDEN, "sso_required", "This workspace requires single sign-on. Use \"Sign in with SSO\".")
        })?;
    }

    // With two-factor sign-in on, the password only earns a short ticket for the code step.
    let has_mfa: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM user_mfa WHERE user_id = ? AND enabled_at IS NOT NULL")
        .bind(&user_id)
        .fetch_optional(&mut *tx)
        .await?;
    if has_mfa.is_some() {
        let ticket = new_token();
        let t = now();
        sqlx::query(
            "INSERT INTO mfa_challenges (id, token_hash, user_id, workspace_id, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(hash_token(&ticket))
        .bind(&user_id)
        .bind(&workspace_id)
        .bind(t)
        .bind(t + MFA_TICKET_SECS)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(Json(json!({ "mfa_required": true, "mfa_token": ticket, "expires_in": MFA_TICKET_SECS })));
    }

    let tokens = start_session(&mut tx, &state, &user_id, &workspace_id, &user_agent(&headers)).await?;
    audit(&mut tx, Some(&workspace_id), Some(&user_id), "auth.login", None, json!({ "mfa": false })).await?;
    let view = session_view(&mut tx, tokens, &user_id, &workspace_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::to_value(view).map_err(|_| AppError::internal())?))
}

const MFA_TICKET_SECS: i64 = 5 * 60;

/// The workspace to sign in to: the one asked for, else the one used most
/// recently, else the first one joined.
async fn choose_workspace(conn: &mut sqlx::SqliteConnection, user_id: &str, requested: Option<String>) -> AppResult<String> {
    let workspace_id = match requested {
        Some(id) => id,
        None => {
            let last: Option<(String,)> = sqlx::query_as(
                "SELECT m.workspace_id FROM memberships m
                 LEFT JOIN sessions s ON s.workspace_id = m.workspace_id AND s.user_id = m.user_id
                 WHERE m.user_id = ?
                 GROUP BY m.workspace_id
                 ORDER BY MAX(COALESCE(s.created_at, 0)) DESC, MIN(m.created_at) ASC
                 LIMIT 1",
            )
            .bind(user_id)
            .fetch_optional(&mut *conn)
            .await?;
            last.map(|(id,)| id).ok_or_else(|| {
                AppError::new(StatusCode::FORBIDDEN, "no_workspace", "You are not a member of any workspace. Ask for an invite.")
            })?
        }
    };
    ensure_member(conn, user_id, &workspace_id).await?;
    Ok(workspace_id)
}

#[derive(Deserialize)]
pub struct MfaLoginBody {
    mfa_token: String,
    code: Option<String>,
    recovery_code: Option<String>,
}

/// The second step of signing in: an authenticator code or a recovery code.
pub async fn login_mfa(
    State(state): State<AppState>,
    headers: HeaderMap,
    Body(body): Body<MfaLoginBody>,
) -> AppResult<Json<SessionView>> {
    let expired = || AppError::unauthorized("This sign-in took too long. Enter your email and password again.");
    let row: Option<(String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT c.id, c.user_id, c.workspace_id, u.email FROM mfa_challenges c JOIN users u ON u.id = c.user_id
         WHERE c.token_hash = ? AND c.used_at IS NULL AND c.expires_at > ?",
    )
    .bind(hash_token(&body.mfa_token))
    .bind(now())
    .fetch_optional(&state.db)
    .await?;
    let (challenge_id, user_id, workspace_id, email) = row.ok_or_else(expired)?;
    let key = throttle::email_key(&email);
    throttle::check(&state.db, &key).await?;

    let method = match (&body.code, &body.recovery_code) {
        (Some(code), _) => {
            let (secret, last_step): (String, i64) =
                sqlx::query_as("SELECT secret, last_step FROM user_mfa WHERE user_id = ? AND enabled_at IS NOT NULL")
                    .bind(&user_id)
                    .fetch_optional(&state.db)
                    .await?
                    .ok_or_else(expired)?;
            match mfa::verify(&secret, code, now(), last_step) {
                Some(step) => {
                    sqlx::query("UPDATE user_mfa SET last_step = ? WHERE user_id = ?").bind(step).bind(&user_id).execute(&state.db).await?;
                    "app"
                }
                None => return Err(wrong_code(&state, &key).await?),
            }
        }
        (None, Some(recovery)) => {
            let used = sqlx::query("UPDATE mfa_recovery_codes SET used_at = ? WHERE user_id = ? AND code_hash = ? AND used_at IS NULL")
                .bind(now())
                .bind(&user_id)
                .bind(hash_token(&mfa::normalize_recovery(recovery)))
                .execute(&state.db)
                .await?;
            if used.rows_affected() != 1 {
                return Err(wrong_code(&state, &key).await?);
            }
            "recovery_code"
        }
        (None, None) => return Err(AppError::bad_request("invalid_body", "Enter the code from your authenticator app.")),
    };
    throttle::clear(&state.db, &key).await?;

    let mut tx = state.db.begin().await?;
    let claimed = sqlx::query("UPDATE mfa_challenges SET used_at = ? WHERE id = ? AND used_at IS NULL")
        .bind(now())
        .bind(&challenge_id)
        .execute(&mut *tx)
        .await?;
    if claimed.rows_affected() != 1 {
        return Err(expired());
    }
    let workspace_id = choose_workspace(&mut tx, &user_id, workspace_id).await?;
    let tokens = start_session(&mut tx, &state, &user_id, &workspace_id, &user_agent(&headers)).await?;
    audit(&mut tx, Some(&workspace_id), Some(&user_id), "auth.login", None, json!({ "mfa": true, "method": method })).await?;
    let view = session_view(&mut tx, tokens, &user_id, &workspace_id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn wrong_code(state: &AppState, key: &str) -> AppResult<AppError> {
    Ok(throttle::fail(&state.db, key)
        .await?
        .unwrap_or_else(|| AppError::unauthorized("That code is not right. Check the time on your phone, or use a recovery code.")))
}

async fn ensure_member(conn: &mut sqlx::SqliteConnection, user_id: &str, workspace_id: &str) -> AppResult<Role> {
    let role: Option<(String,)> = sqlx::query_as("SELECT role FROM memberships WHERE user_id = ? AND workspace_id = ?")
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(&mut *conn)
        .await?;
    match role {
        Some((role,)) => Role::parse(&role),
        None => Err(AppError::forbidden("You are not a member of this workspace.")),
    }
}

#[derive(Deserialize)]
pub struct RefreshBody {
    refresh_token: String,
}

pub async fn refresh(State(state): State<AppState>, Body(body): Body<RefreshBody>) -> AppResult<Json<SessionView>> {
    let mut tx = state.db.begin().await?;
    let row: Option<(String, String, String, bool)> = sqlx::query_as(
        "SELECT s.id, s.user_id, s.workspace_id, s.sso FROM sessions s
         JOIN memberships m ON m.workspace_id = s.workspace_id AND m.user_id = s.user_id
         WHERE s.refresh_hash = ? AND s.revoked_at IS NULL AND s.refresh_expires_at > ?",
    )
    .bind(hash_token(&body.refresh_token))
    .bind(now())
    .fetch_optional(&mut *tx)
    .await?;
    let (session_id, user_id, workspace_id, sso) =
        row.ok_or_else(|| AppError::unauthorized("Your sign-in has expired. Sign in again."))?;
    let tokens = rotate_session(&mut tx, &state, &session_id).await?;
    let mut view = session_view(&mut tx, tokens, &user_id, &workspace_id).await?;
    if sso {
        (view.sso, view.mfa_setup_required) = (true, false);
    }
    tx.commit().await?;
    Ok(Json(view))
}

pub async fn logout(State(state): State<AppState>, auth: Auth) -> AppResult<StatusCode> {
    sqlx::query("UPDATE sessions SET revoked_at = ? WHERE id = ?")
        .bind(now())
        .bind(&auth.session_id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    let user: super::UserView = sqlx::query_as("SELECT id, email, name FROM users WHERE id = ?")
        .bind(&auth.user_id)
        .fetch_one(&mut *conn)
        .await?;
    let workspaces = user_workspaces(&mut conn, &auth.user_id).await?;
    let workspace = workspaces.iter().find(|w| w.id == auth.workspace_id);
    let mfa_setup_required = !auth.sso && super::mfa_setup_required(&mut conn, &auth.user_id, &auth.workspace_id).await?;
    Ok(Json(json!({ "user": user, "workspace": workspace, "workspaces": workspaces, "mfa_setup_required": mfa_setup_required,
        "sso": auth.sso })))
}

pub async fn workspaces(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    let workspaces = user_workspaces(&mut conn, &auth.user_id).await?;
    Ok(Json(json!({ "workspaces": workspaces })))
}

#[derive(Deserialize)]
pub struct SwitchBody {
    workspace_id: String,
}

pub async fn switch(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth: Auth,
    Body(body): Body<SwitchBody>,
) -> AppResult<Json<SessionView>> {
    // The identity provider vouches for this workspace only.
    if auth.sso && body.workspace_id != auth.workspace_id {
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "sso_session",
            "You signed in to this workspace with single sign-on. To open another workspace, sign out and sign in to it.",
        ));
    }
    let mut tx = state.db.begin().await?;
    let role = ensure_member(&mut tx, &auth.user_id, &body.workspace_id).await?;
    if !auth.sso && sso_enforced_for(&mut tx, &body.workspace_id, role).await? {
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "sso_required",
            "That workspace requires single sign-on. Sign out and use \"Sign in with SSO\".",
        ));
    }
    sqlx::query("UPDATE sessions SET revoked_at = ? WHERE id = ?")
        .bind(now())
        .bind(&auth.session_id)
        .execute(&mut *tx)
        .await?;
    let tokens = start_session(&mut tx, &state, &auth.user_id, &body.workspace_id, &user_agent(&headers)).await?;
    if auth.sso {
        crate::auth::mark_sso(&mut tx, &tokens).await?;
    }
    let mut view = session_view(&mut tx, tokens, &auth.user_id, &body.workspace_id).await?;
    if auth.sso {
        (view.sso, view.mfa_setup_required) = (true, false);
    }
    tx.commit().await?;
    Ok(Json(view))
}

#[derive(Deserialize)]
pub struct ChangePasswordBody {
    current_password: String,
    new_password: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    auth: Auth,
    Body(body): Body<ChangePasswordBody>,
) -> AppResult<StatusCode> {
    check_new_password(&body.new_password)?;
    let key = throttle::user_key(&auth.user_id);
    throttle::check(&state.db, &key).await?;
    let (stored,): (String,) = sqlx::query_as("SELECT password_hash FROM users WHERE id = ?")
        .bind(&auth.user_id)
        .fetch_one(&state.db)
        .await?;
    if !verify_password(body.current_password, stored).await? {
        let wrong = AppError::bad_request("wrong_password", "Your current password is not right.");
        return Err(throttle::fail(&state.db, &key).await?.unwrap_or(wrong));
    }
    throttle::clear(&state.db, &key).await?;
    let new_hash = hash_password(body.new_password).await?;

    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(new_hash)
        .bind(&auth.user_id)
        .execute(&mut *tx)
        .await?;
    // Other devices must sign in again; this one stays signed in.
    revoke_user_sessions(&mut tx, &auth.user_id, None, Some(&auth.session_id)).await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "user.password_changed",
        Some(&auth.user_id), json!({})).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ResetRequestBody {
    email: String,
}

/// Always answers 204, so it does not reveal which emails have accounts.
pub async fn reset_request(
    State(state): State<AppState>,
    Body(body): Body<ResetRequestBody>,
) -> AppResult<StatusCode> {
    let Ok(email) = normalize_email(&body.email) else {
        return Ok(StatusCode::NO_CONTENT);
    };
    let user: Option<(String,)> =
        sqlx::query_as("SELECT id FROM users WHERE email = ?").bind(&email).fetch_optional(&state.db).await?;
    if let Some((user_id,)) = user {
        let token = new_token();
        let t = now();
        sqlx::query(
            "INSERT INTO password_resets (id, user_id, token_hash, created_at, expires_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&user_id)
        .bind(hash_token(&token))
        .bind(t)
        .bind(t + state.config.reset_ttl_secs)
        .execute(&state.db)
        .await?;
        // The token sits in the fragment so it never reaches server logs.
        let link = format!("{}/reset#token={token}", state.config.app_url);
        state.mailer.send_link(&email, "Reset your Peter password", &link);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ResetConfirmBody {
    token: String,
    password: String,
}

pub async fn reset_confirm(
    State(state): State<AppState>,
    Body(body): Body<ResetConfirmBody>,
) -> AppResult<StatusCode> {
    check_new_password(&body.password)?;
    let new_hash = hash_password(body.password).await?;

    let mut tx = state.db.begin().await?;
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, user_id FROM password_resets WHERE token_hash = ? AND used_at IS NULL AND expires_at > ?",
    )
    .bind(hash_token(&body.token))
    .bind(now())
    .fetch_optional(&mut *tx)
    .await?;
    let (reset_id, user_id) =
        row.ok_or_else(|| AppError::bad_request("invalid_token", "This reset link has expired or was already used."))?;

    // Using one link spends every outstanding link for this user.
    sqlx::query("UPDATE password_resets SET used_at = ? WHERE user_id = ? AND used_at IS NULL")
        .bind(now())
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(new_hash)
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;
    revoke_user_sessions(&mut tx, &user_id, None, None).await?;
    // Proving access to the mailbox ends any sign-in lock.
    sqlx::query("DELETE FROM login_throttle WHERE key IN (?, (SELECT 'email:' || email FROM users WHERE id = ?))")
        .bind(throttle::user_key(&user_id))
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, None, Some(&user_id), "user.password_reset", Some(&user_id), json!({ "reset_id": reset_id }))
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
