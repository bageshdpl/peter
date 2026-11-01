use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;

use super::{Body, SessionView, auth::{hash_password, verify_password}, clean_name, session_view};
use crate::{
    auth::{Auth, audit, revoke_user_sessions, start_session},
    error::{AppError, AppResult},
    security::{Role, check_new_password, hash_token, new_id, new_token, normalize_email, now},
    state::AppState,
};

#[derive(Serialize, sqlx::FromRow)]
struct MemberView {
    user_id: String,
    email: String,
    name: String,
    role: String,
    joined_at: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct InviteView {
    id: String,
    email: String,
    role: String,
    created_at: i64,
    expires_at: i64,
}

pub async fn list(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let members: Vec<MemberView> = sqlx::query_as(
        "SELECT u.id AS user_id, u.email, u.name, m.role, m.created_at AS joined_at
         FROM memberships m JOIN users u ON u.id = m.user_id
         WHERE m.workspace_id = ? ORDER BY u.name COLLATE NOCASE, u.email",
    )
    .bind(&auth.workspace_id)
    .fetch_all(&state.db)
    .await?;
    let invites: Vec<InviteView> = if auth.role >= Role::Admin {
        sqlx::query_as(
            "SELECT id, email, role, created_at, expires_at FROM invites
             WHERE workspace_id = ? AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > ?
             ORDER BY created_at DESC",
        )
        .bind(&auth.workspace_id)
        .bind(now())
        .fetch_all(&state.db)
        .await?
    } else {
        Vec::new()
    };
    Ok(Json(json!({ "members": members, "invites": invites })))
}

/// Only an owner may grant or take away the owner role.
fn check_can_grant(actor: &Auth, role: Role) -> AppResult<()> {
    if role == Role::Owner && actor.role != Role::Owner {
        return Err(AppError::forbidden("Only an owner can make someone an owner."));
    }
    Ok(())
}

async fn member_role(conn: &mut SqliteConnection, workspace_id: &str, user_id: &str) -> AppResult<Role> {
    let row: Option<(String,)> = sqlx::query_as("SELECT role FROM memberships WHERE workspace_id = ? AND user_id = ?")
        .bind(workspace_id)
        .bind(user_id)
        .fetch_optional(&mut *conn)
        .await?;
    match row {
        Some((role,)) => Role::parse(&role),
        None => Err(AppError::not_found("This person is not a member of the workspace.")),
    }
}

async fn ensure_not_last_owner(conn: &mut SqliteConnection, workspace_id: &str) -> AppResult<()> {
    let (owners,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM memberships WHERE workspace_id = ? AND role = 'owner'")
            .bind(workspace_id)
            .fetch_one(&mut *conn)
            .await?;
    if owners <= 1 {
        return Err(AppError::conflict("last_owner", "A workspace needs at least one owner. Make someone else an owner first."));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct InviteBody {
    email: String,
    role: Role,
}

pub async fn invite(
    State(state): State<AppState>,
    auth: Auth,
    Body(body): Body<InviteBody>,
) -> AppResult<(StatusCode, Json<Value>)> {
    auth.require(Role::Admin)?;
    check_can_grant(&auth, body.role)?;
    let email = normalize_email(&body.email)?;

    let mut tx = state.db.begin().await?;
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT u.id FROM users u JOIN memberships m ON m.user_id = u.id WHERE u.email = ? AND m.workspace_id = ?",
    )
    .bind(&email)
    .bind(&auth.workspace_id)
    .fetch_optional(&mut *tx)
    .await?;
    if existing.is_some() {
        return Err(AppError::conflict("already_member", "This person is already a member."));
    }
    // A new invite replaces any open one for the same email.
    sqlx::query(
        "UPDATE invites SET revoked_at = ? WHERE workspace_id = ? AND email = ? AND accepted_at IS NULL AND revoked_at IS NULL",
    )
    .bind(now())
    .bind(&auth.workspace_id)
    .bind(&email)
    .execute(&mut *tx)
    .await?;

    let (id, token, t) = (new_id(), new_token(), now());
    let expires_at = t + state.config.invite_ttl_secs;
    sqlx::query(
        "INSERT INTO invites (id, workspace_id, email, role, token_hash, invited_by, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&auth.workspace_id)
    .bind(&email)
    .bind(body.role.as_str())
    .bind(hash_token(&token))
    .bind(&auth.user_id)
    .bind(t)
    .bind(expires_at)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "member.invited", Some(&email),
        json!({ "invite_id": id, "role": body.role })).await?;
    tx.commit().await?;

    let link = format!("{}/invite#token={token}", state.config.app_url);
    state.mailer.send_link(&email, "You are invited to Peter", &link);
    let invite = json!({ "id": id, "email": email, "role": body.role, "created_at": t, "expires_at": expires_at });
    // The link is returned too, so an admin can pass it on without email.
    Ok((StatusCode::CREATED, Json(json!({ "invite": invite, "invite_link": link }))))
}

#[derive(Deserialize)]
pub struct ChangeRoleBody {
    role: Role,
}

pub async fn change_role(
    State(state): State<AppState>,
    auth: Auth,
    Path(user_id): Path<String>,
    Body(body): Body<ChangeRoleBody>,
) -> AppResult<StatusCode> {
    auth.require(Role::Admin)?;
    let mut tx = state.db.begin().await?;
    let current = member_role(&mut tx, &auth.workspace_id, &user_id).await?;
    if current == body.role {
        return Ok(StatusCode::NO_CONTENT);
    }
    check_can_grant(&auth, current)?;
    check_can_grant(&auth, body.role)?;
    if current == Role::Owner {
        ensure_not_last_owner(&mut tx, &auth.workspace_id).await?;
    }
    sqlx::query("UPDATE memberships SET role = ? WHERE workspace_id = ? AND user_id = ?")
        .bind(body.role.as_str())
        .bind(&auth.workspace_id)
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "member.role_changed", Some(&user_id),
        json!({ "from": current, "to": body.role })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes a member. Anyone may remove themselves (leave the workspace).
pub async fn remove(
    State(state): State<AppState>,
    auth: Auth,
    Path(user_id): Path<String>,
) -> AppResult<StatusCode> {
    let leaving = user_id == auth.user_id;
    if !leaving {
        auth.require(Role::Admin)?;
    }
    let mut tx = state.db.begin().await?;
    let current = member_role(&mut tx, &auth.workspace_id, &user_id).await?;
    if !leaving {
        check_can_grant(&auth, current)?;
    }
    if current == Role::Owner {
        ensure_not_last_owner(&mut tx, &auth.workspace_id).await?;
    }
    sqlx::query("DELETE FROM memberships WHERE workspace_id = ? AND user_id = ?")
        .bind(&auth.workspace_id)
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;
    revoke_user_sessions(&mut tx, &user_id, Some(&auth.workspace_id), None).await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id),
        if leaving { "member.left" } else { "member.removed" }, Some(&user_id), json!({ "role": current })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn revoke_invite(
    State(state): State<AppState>,
    auth: Auth,
    Path(invite_id): Path<String>,
) -> AppResult<StatusCode> {
    auth.require(Role::Admin)?;
    let mut tx = state.db.begin().await?;
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT email, role FROM invites WHERE id = ? AND workspace_id = ? AND accepted_at IS NULL AND revoked_at IS NULL",
    )
    .bind(&invite_id)
    .bind(&auth.workspace_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (email, role) = row.ok_or_else(|| AppError::not_found("No open invite with this id."))?;
    check_can_grant(&auth, Role::parse(&role)?)?;
    sqlx::query("UPDATE invites SET revoked_at = ? WHERE id = ?")
        .bind(now())
        .bind(&invite_id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "invite.revoked", Some(&email),
        json!({ "invite_id": invite_id })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct AcceptBody {
    token: String,
    password: String,
    /// Needed only when the invited email has no account yet.
    name: Option<String>,
}

pub async fn accept_invite(State(state): State<AppState>, Body(body): Body<AcceptBody>) -> AppResult<Json<SessionView>> {
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT id, workspace_id, email, role FROM invites
         WHERE token_hash = ? AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > ?",
    )
    .bind(hash_token(&body.token))
    .bind(now())
    .fetch_optional(&state.db)
    .await?;
    let (invite_id, workspace_id, email, role) =
        row.ok_or_else(|| AppError::bad_request("invalid_token", "This invite has expired, was withdrawn, or was already used."))?;

    let existing: Option<(String, String)> = sqlx::query_as("SELECT id, password_hash FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    // Work that can fail or is slow happens before the transaction opens.
    let new_user = match &existing {
        Some((_, stored)) => {
            if !verify_password(body.password.clone(), stored.clone()).await? {
                return Err(AppError::unauthorized("You already have an account. Enter its password to accept."));
            }
            None
        }
        None => {
            let name = clean_name(body.name.as_deref().unwrap_or(""), "name")?;
            check_new_password(&body.password)?;
            Some((name, hash_password(body.password).await?))
        }
    };

    let mut tx = state.db.begin().await?;
    // Re-check inside the transaction so one invite can be used only once.
    let claimed = sqlx::query("UPDATE invites SET accepted_at = ? WHERE id = ? AND accepted_at IS NULL AND revoked_at IS NULL")
        .bind(now())
        .bind(&invite_id)
        .execute(&mut *tx)
        .await?;
    if claimed.rows_affected() != 1 {
        return Err(AppError::bad_request("invalid_token", "This invite was already used."));
    }
    let t = now();
    let user_id = match (existing, new_user) {
        (Some((id, _)), _) => id,
        (None, Some((name, hash))) => {
            let id = new_id();
            sqlx::query("INSERT INTO users (id, email, name, password_hash, created_at) VALUES (?, ?, ?, ?, ?)")
                .bind(&id)
                .bind(&email)
                .bind(name)
                .bind(hash)
                .bind(t)
                .execute(&mut *tx)
                .await?;
            id
        }
        (None, None) => return Err(AppError::internal()),
    };
    sqlx::query(
        "INSERT INTO memberships (workspace_id, user_id, role, created_at) VALUES (?, ?, ?, ?)
         ON CONFLICT (workspace_id, user_id) DO NOTHING",
    )
    .bind(&workspace_id)
    .bind(&user_id)
    .bind(&role)
    .bind(t)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, Some(&workspace_id), Some(&user_id), "member.joined", Some(&user_id),
        json!({ "invite_id": invite_id, "role": role })).await?;
    let tokens = start_session(&mut tx, &state, &user_id, &workspace_id).await?;
    let view = session_view(&mut tx, tokens, &user_id, &workspace_id).await?;
    tx.commit().await?;
    Ok(Json(view))
}
