# Peter

A GxP computer-system validation workspace. See [SCOPE.md](SCOPE.md) for what
it does and the build plan.

| Part | Folder | Status |
| --- | --- | --- |
| API server (Rust) | `server/` | M0 Foundation, M1 Projects and sessions, M2 Observation, M3 Guided steps, M4 AI drafting |
| Web app (React) | `web/` | sign-in, projects, guided sessions with AI drafts, sources, members, audit trail, settings |
| Observer extension | `extension/` | records steps and screenshots into a session ([setup](extension/README.md)) |

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

## AI drafting: OpenRouter and Gemini

Peter drafts each step's document with the AI provider you choose. Set one or
both keys before starting the API; keys stay on the server and are never sent
to the browser.

macOS and Linux:

```bash
export OPENROUTER_API_KEY=sk-or-...
export GEMINI_API_KEY=AIza...
cargo run
```

Windows PowerShell:

```powershell
$env:OPENROUTER_API_KEY = "sk-or-..."
$env:GEMINI_API_KEY = "AIza..."
cargo run
```

| Variable | Default | Meaning |
| --- | --- | --- |
| `OPENROUTER_API_KEY` | none | enables OpenRouter |
| `PETER_OPENROUTER_MODEL` | `google/gemini-2.5-flash` | any model id from openrouter.ai/models |
| `GEMINI_API_KEY` | none | enables Google Gemini (key from Google AI Studio) |
| `PETER_GEMINI_MODEL` | `gemini-2.5-flash` | any Gemini model id |
| `PETER_AI_SAMPLE` | `true` | offers "Sample draft (no AI)", which lists the evidence without calling any AI |
| `PETER_AI_TIMEOUT_SECS` | `180` | how long to wait for a provider |

Model names change often; if a default is retired, set the model variable to a
current one. Each draft records the provider, model and token counts.

How drafting works:

- Each step has a document (for example, Requirements and risk produces the
  user requirements and risk assessment). Peter's prompts are in
  `server/src/prompts.rs`.
- The prompt contains the session's evidence: observed steps (`[O12]`),
  text from the project's source files (`[S2]`; PDF, Word, Excel, CSV, text and
  Markdown are read), and documents drafted in earlier steps
  (`[D:requirements]`). Hidden values are never included.
- The AI must cite evidence for every statement and write `[GAP: ...]` where
  evidence is missing. Peter checks every citation: ones that point at
  evidence that does not exist are flagged in red on the page.
- Every draft and every edit is a new version; older versions stay readable.
  Drafts, edits and failures are in the audit trail.

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
projects (with sources and sessions), sessions, members (invite, change roles, remove, withdraw invites), audit trail,
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
| `PETER_DATA_DIR` | `peter-data` | folder for uploaded files |
| `PETER_MAX_UPLOAD_MB` | `25` | largest file accepted |

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

## API (M1)

| Method and path | Who | What |
| --- | --- | --- |
| `GET /v1/validation-types` | member | the kinds of validation and their steps |
| `GET /v1/projects?archived=false` | member | projects, with session and file counts |
| `POST /v1/projects` | member+ | create: `name`, `system_name`, `vendor`, `description` |
| `GET /v1/projects/{id}` | member | one project |
| `PATCH /v1/projects/{id}` | member+ | change any of the fields above |
| `POST /v1/projects/{id}/archive`, `/restore` | admin | archived projects are read-only |
| `GET /v1/projects/{id}/sources` | member | reference files |
| `POST /v1/projects/{id}/sources` | member+ | upload, multipart field `file` |
| `GET /v1/projects/{id}/sources/{source_id}/file` | member | download |
| `DELETE /v1/projects/{id}/sources/{source_id}` | member+ | hide from the project; the stored copy is kept |
| `GET /v1/sessions?project_id=&unfiled=&archived=` | member | sessions |
| `POST /v1/sessions` | member+ | start: `title`, `validation_type`, optional `project_id` |
| `GET /v1/sessions/{id}` | member | the session with its type and steps |
| `PATCH /v1/sessions/{id}` | member+ | `title`; `project_id` moves it (`null` unfiles it) |
| `POST /v1/sessions/{id}/archive`, `/restore` | member+ | archived sessions are read-only |

"member" means any role including viewer; "member+" excludes viewers.
Uploads accept PDF, Word (.docx), Excel (.xlsx), CSV, text, Markdown, PNG and
JPEG. The file type comes from the extension, and each file's SHA-256 is
recorded. Only Initial validation can be started so far; the other types are
listed with their steps and marked unavailable.

## API (M2)

| Method and path | Who | What |
| --- | --- | --- |
| `POST /v1/auth/extension-code` | signed in | one-time code (5 minutes) to connect the extension |
| `POST /v1/auth/extension-exchange` | anyone | trade the code for the extension's own sign-in |
| `GET /v1/sessions/{id}/observations?after_seq=` | member | recorded steps, in order |
| `POST /v1/sessions/{id}/observations` | member+ | record a step; optional `screenshot` as a JPEG or PNG data URL (8 MB max) |
| `GET /v1/sessions/{id}/observations/{oid}/screenshot` | member | the screenshot |
| `DELETE /v1/sessions/{id}/observations/{oid}` | member+ | remove a step; it is kept as a record, and audited |
| `GET /v1/sessions/{id}/events` | member | Server-Sent Events: `observation`, `observation_removed`, `resync` |

Step kinds: `navigate`, `click`, `input`, `select`, `submit`, `note`,
`capture`. Steps are numbered per session in the order the server receives
them. A screenshot's bytes must match its declared type, and its SHA-256 is
stored.

## API (M3, M4)

| Method and path | Who | What |
| --- | --- | --- |
| `POST /v1/sessions/{id}/current` | member+ | open a step (`step_id`); a step not started becomes in progress |
| `PUT /v1/sessions/{id}/steps/{step_id}` | member+ | set `status`: `not_started`, `in_progress`, `reviewed`, `skipped` |
| `GET /v1/ai/providers` | member | providers set up on the server, and their models |
| `GET /v1/sessions/{id}/documents/{step_id}` | member | the step's document, current version, citations, version list |
| `POST /v1/sessions/{id}/documents/{step_id}/draft` | member+ | draft with `provider` and optional `instructions`; answers 202, then a `document` event |
| `PUT /v1/sessions/{id}/documents/{step_id}` | member+ | save an edit: `body`, `base_version` (409 if someone saved since) |
| `GET /v1/sessions/{id}/documents/{step_id}/versions/{n}` | member | an older version |

`GET /v1/sessions/{id}` also returns `step_states` and `documents`. The event
stream adds `step` and `document` events.

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
