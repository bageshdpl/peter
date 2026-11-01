# Peter

A GxP computer-system validation workspace. See [SCOPE.md](SCOPE.md) for what
it does and the build plan.

| Part | Folder | Status |
| --- | --- | --- |
| API server (Rust) | `server/` | M0 Foundation done |
| Web app (React) | `web/` | sign-in, members, audit trail, settings |
| Observer extension | `extension/` | not started |

## Run the API locally

Install Rust once (Windows: `winget install Rustlang.Rustup`; macOS and Linux:
see https://rustup.rs). Then:

```bash
cd apps/peter/server
cargo run
```

The API listens on `http://127.0.0.1:8080` and keeps its data in `peter.db`
in that folder. Emails (invites, password resets) are printed to the console
instead of being sent.

Run the tests:

```bash
cargo test
```

## Run the web app locally

Needs Node.js 22 or later. With the API running in another terminal:

```bash
cd apps/peter/web
npm install
npm run dev
```

Open http://localhost:5173 and create an account. The web app forwards `/v1`
calls to the API at `http://127.0.0.1:8080`; set `PETER_API_URL` to change
that.

Screens: sign in, create account, forgot and reset password, accept an invite,
home, members (invite, change roles, remove, withdraw invites), audit trail,
and settings (change password, rename the workspace, create another
workspace). The workspace switcher is in the sidebar.

Invites show their link on screen, so you can test with a second browser
window (or a private window) without email.

### Settings

| Variable | Default | Meaning |
| --- | --- | --- |
| `PETER_DATABASE_URL` | `sqlite://peter.db` | SQLite database file |
| `PETER_BIND` | `127.0.0.1:8080` | address the API listens on |
| `PETER_APP_URL` | `http://localhost:5173` | web app address, for CORS and email links |
| `PETER_ACCESS_TTL_SECS` | `900` | access token lifetime |
| `PETER_REFRESH_TTL_SECS` | `2592000` | refresh token lifetime (30 days) |
| `PETER_INVITE_TTL_SECS` | `604800` | invite lifetime (7 days) |
| `PETER_RESET_TTL_SECS` | `3600` | password reset link lifetime |
| `PETER_ALLOW_SIGNUP` | `true` | `false` makes the API invite-only |

## API (M0)

Send `Authorization: Bearer <access_token>` on every call except sign-up,
login, refresh, accept-invite and password reset. Errors look like
`{"error": {"code": "...", "message": "..."}}`.

| Method and path | Who | What |
| --- | --- | --- |
| `POST /v1/auth/signup` | anyone | create an account and its first workspace (you are owner) |
| `POST /v1/auth/login` | anyone | sign in; optional `workspace_id`, else the last one used |
| `POST /v1/auth/refresh` | anyone | new tokens for a refresh token; the old pair stops working |
| `POST /v1/auth/logout` | signed in | end this session |
| `GET /v1/auth/me` | signed in | you, this workspace, all your workspaces |
| `GET /v1/auth/workspaces` | signed in | your workspaces and your role in each |
| `POST /v1/auth/switch` | signed in | tokens for another of your workspaces |
| `POST /v1/auth/change-password` | signed in | other devices are signed out |
| `POST /v1/auth/reset/request` | anyone | email a reset link (always answers 204) |
| `POST /v1/auth/reset/confirm` | anyone | set a new password; all sessions end |
| `POST /v1/auth/accept-invite` | anyone | join a workspace from an invite link |
| `POST /v1/workspaces` | signed in | create another workspace |
| `GET /v1/workspace` | member | this workspace |
| `PATCH /v1/workspace` | admin | rename it |
| `GET /v1/members` | member | members; admins also see open invites |
| `POST /v1/members` | admin | invite by email with a role |
| `PATCH /v1/members/{user_id}` | admin | change a role |
| `DELETE /v1/members/{user_id}` | admin, or yourself | remove a member, or leave |
| `DELETE /v1/invites/{id}` | admin | withdraw an invite |
| `GET /v1/audit` | admin | audit trail, newest first; page with `?before=<seq>` |

### Roles

`owner` > `admin` > `member` > `viewer`. Only an owner can make or change an
owner, and a workspace always keeps at least one owner. Removing a member ends
their sessions in that workspace at once.

### Security notes

- Passwords: Argon2id, at least 10 characters.
- Tokens are random, and only their SHA-256 hashes are stored.
- Sign-in does the same work for unknown emails, so timing does not reveal
  which emails have accounts. Password reset always answers 204 for the same
  reason.
- The audit trail is append-only: database triggers reject updates and deletes.
- Not yet: rate limiting on sign-in, two-factor sign-in, SSO (see SCOPE.md).
