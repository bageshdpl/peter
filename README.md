# Peter

A GxP computer-system validation workspace. See [SCOPE.md](SCOPE.md) for what
it does and the build plan.

| Part | Folder | Status |
| --- | --- | --- |
| API server (Rust) | `server/` | M0–M5: foundation, projects and sessions, observation, guided steps, AI drafting, review and export |
| Web app (React) | `web/` | sign-in, projects, guided sessions with AI drafts, comments, signatures, exports, members, audit trail, settings |
| Observer extension | `extension/` | records steps and screenshots into a session ([setup](extension/README.md)) |

**Just want to use Peter?** Download the ready-to-run zip for Windows or Mac
from [Releases](https://github.com/bageshdpl/peter/releases/latest): no
installation and no admin rights needed. Step by step:
[Windows](docs/SETUP-WINDOWS.md) · [Mac](docs/SETUP-MAC.md).

**Want testers to use it online?** [docs/DEPLOY.md](docs/DEPLOY.md) puts
Peter on Render with backups to Supabase, free, invite-only.

### Using Peter from another GitHub account

The repository is private, so another account (for example one linked to a
different email) needs access before it can download releases or clone:

1. Signed in as the owner (`bageshdpl`), open the repository's
   **Settings → Collaborators → Add people**, enter the other account's
   username or email, and choose a role (**Write** to push changes, **Read**
   only to download and clone).
2. The other account accepts the invitation from its email or from
   https://github.com/notifications.
3. It can then download releases, and clone with
   `git clone https://github.com/bageshdpl/peter.git`.

Each computer keeps its own Peter data (`peter.db` and `peter-data`); they are
not synced. Use a different account per person in Peter itself.

## Get the code

```bash
git clone https://github.com/bageshdpl/peter.git
cd peter
```

## Run the API locally

Install Rust once (Windows: `winget install Rustlang.Rustup`; macOS and Linux:
see https://rustup.rs). Then:

```bash
cd server
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

## Review and export (M5)

- **Comments.** Anyone in the workspace, viewers included, can comment on a
  document; select text in it first to comment on that passage. Threads take
  replies and can be resolved and reopened by the person who raised them or by
  any member.
- **Signatures.** Members sign the current version as *Reviewed* or
  *Approved* after re-entering their password. The signer's name and email,
  the meaning and the time are stored with that version and can never be
  changed or removed. A new version starts unsigned. Peter records signatures
  in the spirit of 21 CFR Part 11 but has not been assessed for compliance.
- **Word export.** Cover page (system, session, step, version, who made it,
  AI model), signatures (or blank signature lines), the open-gap warning, the
  document, then appendices: observed steps cited with their screenshots and
  SHA-256, source files cited with SHA-256, and the version history. Any
  version can be exported.
- **Excel export.** Every table in the document, one sheet each, with a frozen
  header row and filters.
- **PDF export.** The same content as the Word export (cover, signatures,
  body, appendices with screenshots, version history) with the logo, company
  name, footer text, document number and page numbers on every page. Text
  can be searched and copied. It uses the Liberation Sans fonts, embedded
  under the SIL Open Font License (`server/fonts/LICENSE-Liberation.txt`).
- **Session package.** A ZIP with every drafted document (Word, plus Excel
  where it has tables), all observed steps as CSV, and a README index.

Every export is recorded in the audit trail.

## Validation types

Pick the kind of work when starting a session. Each kind has its own steps,
and each step drafts its own document. The Observer recorder appears on the
steps where you exercise the system (marked ● below).

| Type | Steps |
| --- | --- |
| Initial validation | ● Observe, System assessment, Requirements and risk, Functional and design spec, Configuration spec, Validation plan, IQ, Testing, Traceability matrix, Summary report |
| Change validation | ● Describe the change, Baseline documents, Impact assessment, ● Test scope and results, Document updates, Change summary |
| Periodic review | Changes and incidents, ● Controls check, Review report |
| Data migration | Migration plan, Data mapping, Verification plan, ● Migration report |
| Retirement | Retirement plan, Archive plan, ● Decommissioning, Retirement report |
| AI validation | ● Intended use, Context and risk, Data and model, Acceptance criteria, ● Performance testing, Monitoring |
| Equipment | ● Assessment (USP <1058> group A, B or C), Requirements, IQ, ● OQ, ● PQ, Qualification report |
| Dashboards and reports | ● Use assessment, Report specification, ● Verification, Verification report |
| Procedures | Procedure list, Administration SOP, Backup and recovery SOP, Training plan, ● Work instructions |

Skip the steps that do not apply; for example, group A equipment needs only
its assessment.

**Building on an earlier validation.** Change validation, periodic review and
retirement start by asking which earlier session of the same project they
build on (you can change it later under **Builds on** on the session page).
Drafts then see that session's current documents and cite them as
`[B:requirements]`, `[B:testing]` and so on; on the page these link to the
earlier session, and in Word and PDF exports they read "see User requirements
in "LIMS initial validation"". Without a link, add the approved documents as
project files instead. Any session type can have a baseline through the API.

The traceability matrix and controls coverage below belong to Initial
validation's Traceability step.

## Traceability

On the **Traceability matrix** step, Peter shows a live matrix read from the
session's documents: requirement → specification → tests → result → evidence.
It is built by plain rules, not AI, from these IDs:

| Document (step) | What Peter reads |
| --- | --- |
| Requirements and risk | table rows whose ID column holds `URS-001`…, with the `Requirement` and `Risk` columns |
| Functional and design spec, Configuration spec | table rows starting `FS-001`… or `CS-001`…; the `URS` ids in the row |
| Installation qualification, Testing | headings starting `IQ-001`… or `TS-001`… (or table rows starting with them); the `URS` ids under the heading; the `Pass/Fail` column |

A requirement is **Passed** when every test covering it passed, **Failed**
when any failed, **Not yet executed** otherwise, and **No test** when nothing
covers it. High-risk requirements that are not passed are counted separately.
The AI prompts ask for these formats, and edited documents work the same way.

**Write into document** saves the matrix as a new version of the
Traceability document, marked "built by Peter (no AI)". The matrix also
exports to Excel (matrix, tests, open items).

## Library

Open **Library** in the sidebar. Everyone can see it; admins change it.

- **Regulations.** Choose the regulations your company works to: 21 CFR
  Part 11, EU GMP Annex 11, GAMP 5 software categories, ALCOA+ and ICH Q9(R1).
  Drafts then name the relevant clauses, for example "(21 CFR 11.10(e))". The
  clause summaries are Peter's own wording for orientation, not the official
  text.
- **Templates.** Upload your company's template for any document (Word, PDF,
  text or Markdown). Peter reads its text and drafts follow its headings,
  order and table columns. One per document; uploading again replaces it
  (the old one is kept, hidden, as a record). You can preview exactly the
  text Peter reads.
- **Writing style.** Add up to 10 approved documents your company wrote
  before; Peter keeps only their text. **Learn the style** asks the AI to
  describe how they are written (wording, structure, test-step format, IDs,
  level of detail) without copying their facts, as a short guide you can
  edit, or write one by hand. Every draft then follows it; the rules on
  evidence and citations always come first.
- **Branding.** Company name, logo (PNG or JPEG up to 1 MB), document number
  prefix and footer text. Every Word export then shows the logo and name in
  the page header, a document number such as `ACME-CSV-7F3A9C21-URS` on the
  cover, and the footer text on every page.

## Controls

Controls are what your SOPs and the regulations require of a system. Peter
follows each one through the requirements to their tests.

- **Your SOPs' controls.** On a project page, under **Controls**, choose a
  source file and click **Find controls**. Peter reads its "shall", "must" and
  "is required to" statements (with their section numbers) and numbers them
  CTL-001, CTL-002 and so on. Remove any that do not apply, or add your own by
  hand. Numbers are never reused.
- **Regulations.** Every clause of the regulations chosen in the Library is a
  control too (GAMP 5 categories are left out: they classify software rather
  than require anything of it).
- **Coverage.** In a session, the **Traceability matrix** step shows each
  control as Met, Failed, Not yet executed, No test, Not covered or Not
  applicable. A requirement covers a control when its row names it, such as
  "(21 CFR 11.10(e))", "(Annex 11, clause 9)" or "(CTL-003)". AI drafts of the
  requirements now write these names. You can also link requirements by hand,
  or mark a control not applicable with the reason; both go in the audit
  trail. **Excel** exports the whole list.

Like the matrix, this is built by plain rules, not AI, so the same documents
always give the same result.

## Sharing and notifications

Everyone in a workspace can open every session. Sharing points a colleague at
one and tells them.

- **Share.** In a session, click **Share**, choose a person and either the
  whole session or one document, and add a message if you like.
- **Ask for a review.** Tick "Ask them to review and sign this document"
  (members, admins and owners only, since viewers cannot sign). It stays on
  their home page under **Shared with you** until they sign that document;
  signing completes it and tells you.
- **@mentions.** In a comment, type `@` and a person's full name (or email)
  to notify them. Replies notify everyone already in the thread.
- **Notifications.** The sidebar shows how many are unread; the
  **Notifications** page lists them, and clicking one opens the place it is
  about. Peter does not send emails.

Shares, review requests and their removal are in the audit trail.

## AI keys and usage

Open **AI and usage** in the sidebar.

- **The workspace's own key.** An admin can add the workspace's own
  OpenRouter or Gemini key, and optionally a model. Drafts in that workspace
  then use it, so the workspace pays its provider directly. The key is
  encrypted in the database and never shown again; only its last four
  characters are. **Check key** tries it without spending anything.
- **The server's key.** Workspaces without their own key use the server's
  (`OPENROUTER_API_KEY`, `GEMINI_API_KEY`). `PETER_SHARED_AI_MONTHLY_TOKENS`
  limits how many tokens each workspace may use on it per month (no limit if
  unset), and `PETER_SHARED_AI=false` turns sharing off.
- **Usage.** Admins see every AI draft, whether it worked or not: tokens, and
  the cost OpenRouter reports, by key, by person and one by one.
- **The secret key.** Workspace keys are encrypted with `PETER_SECRET_KEY`.
  Without it, Peter makes `secret.key` in its data folder. Keep it with the
  data and never change it, or saved keys can no longer be read.

## Security

Open **Security** in the sidebar.

- **Two-factor sign-in.** Scan the QR code with an authenticator app (Google
  Authenticator, Microsoft Authenticator, 1Password and others), then enter a
  code to turn it on. You get 10 one-time recovery codes for a lost phone;
  they are shown once, so save them. Each code works once, and a code from
  30 seconds either side of now is accepted to allow for clock drift.
- **Require it for a workspace.** Admins can require two-factor sign-in for
  everyone. Members without it can sign in but do nothing except set it up.
  An admin must have it on before requiring it, so nobody locks themselves
  out.
- **Sign-in limits.** After 5 wrong passwords (or codes) for one email, sign-in
  is refused for 15 minutes, even with the right password. Unknown emails are
  limited the same way, so a lock does not reveal whether an account exists.
  Signing a document, changing a password and turning off two-factor sign-in
  have the same limit. Resetting the password by email ends a lock.
- **Where you're signed in.** Lists every browser and the extension signed in
  to your account, when each was last active, and lets you sign out any of
  them or all others.

Everything here is recorded in the audit trail.

### Single sign-on (OpenID Connect)

Admins set it up under **Security → Single sign-on**. It works with Microsoft
Entra ID, Okta, Google Workspace, Auth0, Keycloak and any other provider that
speaks OpenID Connect.

1. At the provider, register a **web application** and give it the redirect
   address Peter shows (`<PETER_APP_URL>/sso/callback`). Ask for the `openid`,
   `email` and `profile` scopes. Note its issuer address, client ID and client
   secret.
2. In Peter, enter those three, optionally the email domains you accept, tick
   **Turn single sign-on on** and save. Peter refuses to save if it cannot
   reach the provider. The secret is encrypted (see `PETER_SECRET_KEY`) and
   never shown again.
3. Give colleagues the **sign-in link** Peter shows, or tell people already in
   the workspace to choose **Sign in with SSO** and enter their email.

How it behaves:

- **This workspace only.** A provider vouches for people in the workspace that
  set it up and nowhere else. Someone signed in through it cannot switch to
  another workspace, change Peter's own two-factor settings, or see or end
  their sessions elsewhere; for those they sign in with their password.
- **New people.** With **Let new people join on their first sign-in** on, a
  first sign-in creates the account (as member or viewer, your choice) and is
  recorded in the audit trail. Off, only people you invited get in.
- **Require single sign-on.** Passwords stop working for the workspace at
  once, including sessions already open, for everyone except owners. Owners
  keep password access so a broken provider cannot lock the workspace. An
  admin who is not an owner must have signed in through it before requiring it.
- **Two-factor sign-in.** The provider does it; Peter's own requirement does
  not apply to people who came through the provider.
- **Signing documents.** People who came through the provider confirm a
  signature by signing in again there (Peter asks it for a fresh sign-in),
  instead of a Peter password. The audit trail records which was used.
- **What Peter checks.** The provider's answer must be signed with the
  provider's published keys (RSA, ECDSA or EdDSA; never unsigned or
  shared-secret), name this workspace's client ID and issuer, be unexpired,
  and belong to the sign-in Peter started (state, nonce and PKCE). Each
  answer works once. Emails the provider marks unverified are refused.
- An account made by single sign-on has no password. To get one, use
  **Forgot your password?**.

Limits: SAML and SCIM are not built yet. "Sign in with SSO" by email tells
the person whether that email belongs to a workspace with single sign-on.
The issuer must be a public `https` address; `PETER_SSO_ALLOW_LOCAL=true`
allows a local or plain-http provider for development only.

Not yet: limits per network address, and encrypting the authenticator secrets
in the database (they are stored as plain text, so protect the database file).

## Run the web app locally

Needs Node.js 22 or later. With the API running in another terminal:

```bash
cd web
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
| `PETER_ALLOW_SIGNUP` + `PETER_ADMIN_EMAIL` | `true`, none | with sign-up off, only this email may create the first account |
| `PETER_EXTENSION_DIR` | none | the extension folder; Peter then offers it as a download set up for `PETER_APP_URL` |
| `PETER_SSO_ALLOW_LOCAL` | `false` | `true` lets single sign-on use a provider on this machine or over plain http (development only) |
| `PETER_WEB_DIR` | none | folder of the built web app (`web/dist`); the server then serves the app too, so no Node.js is needed |

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
| `POST /v1/sessions` | member+ | start: `title`, `validation_type`, optional `project_id` and `baseline_session_id` |
| `GET /v1/sessions/{id}` | member | the session with its type and steps |
| `PATCH /v1/sessions/{id}` | member+ | `title`; `project_id` moves it (`null` unfiles it); `baseline_session_id` sets or clears the earlier validation |
| `POST /v1/sessions/{id}/archive`, `/restore` | member+ | archived sessions are read-only |

"member" means any role including viewer; "member+" excludes viewers.
Uploads accept PDF, Word (.docx), Excel (.xlsx), CSV, text, Markdown, PNG and
JPEG. The file type comes from the extension, and each file's SHA-256 is
recorded. Every validation type can be started; see
[Validation types](#validation-types).

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

## API (M5)

| Method and path | Who | What |
| --- | --- | --- |
| `GET /v1/sessions/{id}/documents/{step_id}/comments` | member | comments and replies |
| `POST /v1/sessions/{id}/documents/{step_id}/comments` | any role | `body`, optional `quote`, optional `parent_id` to reply |
| `POST /v1/comments/{id}/resolve`, `/reopen` | its author, or member+ | threads only |
| `POST /v1/sessions/{id}/documents/{step_id}/signatures` | member+ | `version` (the current one), `meaning` (`reviewed`/`approved`), `password`, or `sso_token` from a fresh sign-in at the provider |
| `GET /v1/sessions/{id}/documents/{step_id}/export/docx?version=` | member | Word file |
| `GET /v1/sessions/{id}/documents/{step_id}/export/pdf?version=` | member | PDF file |
| `GET /v1/sessions/{id}/documents/{step_id}/export/xlsx?version=` | member | Excel file of the tables (422 if none) |
| `GET /v1/sessions/{id}/export` | member | ZIP package of the session |

The document view (`GET .../documents/{step_id}`) also returns `signatures`
and `open_comments`.

## API (traceability)

| Method and path | Who | What |
| --- | --- | --- |
| `GET /v1/sessions/{id}/trace` | member | the matrix, tests, summary, warnings, and which document versions it was read from |
| `POST /v1/sessions/{id}/trace/write` | member+ | save the matrix as a new version of the Traceability document |
| `GET /v1/sessions/{id}/trace/export` | member | Excel workbook: Traceability, Tests, Open items |

## API (security)

| Method and path | Who | What |
| --- | --- | --- |
| `POST /v1/auth/login` | anyone | with two-factor on, answers `{ "mfa_required": true, "mfa_token" }` instead of a session |
| `GET /v1/workspace/sso` | admin | single sign-on settings (never the secret), the redirect address and the sign-in link |
| `PUT /v1/workspace/sso` | admin | `issuer`, `client_id`, `client_secret` (left out keeps it), `enabled`, `enforce`, `auto_join`, `default_role`, `domains` |
| `DELETE /v1/workspace/sso` | admin | remove it |
| `POST /v1/workspace/sso/check` | admin | try the saved provider: `{ "ok", "message" }` |
| `POST /v1/auth/sso/start` | anyone | `workspace_id` or `email`; answers `{ "url" }` at the provider |
| `POST /v1/auth/sso/callback` | anyone | `code` and `state` from the provider; answers a session, or `{ "purpose": "sign", "sign_token" }` |
| `POST /v1/auth/sso/sign-start` | member+ | `{ "url" }` for a fresh sign-in at the provider, to confirm a signature |
| `POST /v1/auth/login/mfa` | anyone | `mfa_token` plus `code` or `recovery_code`; returns the session |
| `GET /v1/auth/mfa` | signed in | whether two-factor is on, recovery codes left, whether the workspace requires it |
| `POST /v1/auth/mfa/setup` | signed in | a new secret and `otpauth_url` (not active until enabled) |
| `POST /v1/auth/mfa/enable` | signed in | `code`; turns it on and returns 10 recovery codes |
| `POST /v1/auth/mfa/disable` | signed in | `password` and `code` |
| `POST /v1/auth/mfa/recovery-codes` | signed in | `code`; replaces the recovery codes |
| `GET /v1/auth/sessions` | signed in | your active sessions |
| `DELETE /v1/auth/sessions/{id}` | signed in | sign out one of them |
| `POST /v1/auth/sessions/revoke-others` | signed in | sign out all but this one |
| `PUT /v1/workspace/security` | admin | `require_mfa` |

Wrong passwords or codes beyond the limit answer `429` with code `locked`.
When a workspace requires two-factor sign-in and you have not set it up,
other calls answer `403` with code `mfa_setup_required`.

## API (library)

| Method and path | Who | What |
| --- | --- | --- |
| `GET /v1/library/standards` | member | the catalog and which are selected |
| `PUT /v1/library/standards` | admin | `selected`: list of ids (`part11`, `annex11`, `gamp5`, `alcoa`, `ichq9`) |
| `GET /v1/library/templates` | member | current templates, and the steps that can have one |
| `POST /v1/library/templates` | admin | multipart: `file`, `step_id`, optional `name` |
| `GET /v1/library/templates/{id}` | member | the text Peter reads from it |
| `DELETE /v1/library/templates/{id}` | admin | stop using it |
| `GET /v1/library/style` | member | the style guide and the sample documents |
| `PUT /v1/library/style` | admin | `guide`: write or edit it by hand (empty stops using one) |
| `POST /v1/library/style/samples` | admin | multipart `file`: add a past document |
| `DELETE /v1/library/style/samples/{id}` | admin | remove a sample |
| `POST /v1/library/style/learn` | admin | `provider`: learn the guide from the samples |
| `GET /v1/workspace/branding`, `PUT` | member, admin | `company_name`, `doc_prefix`, `footer_text` |
| `GET /v1/workspace/branding/logo`, `POST`, `DELETE` | member, admin | the logo (multipart `file` to upload) |

## API (controls)

| Method and path | Who | What |
| --- | --- | --- |
| `GET /v1/projects/{id}/controls` | viewer | the project's SOP controls |
| `POST /v1/projects/{id}/controls` | member | `text`, optional `section`: add one by hand |
| `POST /v1/projects/{id}/sources/{source_id}/controls` | member | find and add the file's "shall" and "must" statements |
| `DELETE /v1/projects/{id}/controls/{control_id}` | member | remove one (kept, hidden, as a record) |
| `GET /v1/sessions/{id}/controls` | viewer | coverage of every control, and the session's requirements |
| `PUT /v1/sessions/{id}/controls/{key}` | member | `requirements` (URS ids), `not_applicable`, `reason` |
| `GET /v1/sessions/{id}/controls/export` | viewer | the coverage as Excel |

## API (sharing and notifications)

| Method and path | Who | What |
| --- | --- | --- |
| `POST /v1/sessions/{id}/shares` | member | `user_id`, optional `step_id`, `review` (ask to sign), `message` |
| `GET /v1/sessions/{id}/shares` | viewer | open shares on the session |
| `DELETE /v1/shares/{id}` | who shared it, or admin | remove it |
| `POST /v1/shares/{id}/done` | who it was shared with | `done`: true or false |
| `GET /v1/shared-with-me` | viewer | open shares, then done ones from the last 30 days |
| `GET /v1/notifications` | viewer | latest 100 (`?unread=true` for unread only) and the unread count |
| `GET /v1/notifications/unread` | viewer | the unread count |
| `POST /v1/notifications/read` | viewer | `ids`, or `all: true` |

## API (AI keys and usage)

| Method and path | Who | What |
| --- | --- | --- |
| `GET /v1/workspace/ai` | viewer | which key each provider uses (never the key), and the shared allowance |
| `PUT /v1/workspace/ai/{openrouter or gemini}` | admin | `key` and/or `model` |
| `DELETE /v1/workspace/ai/{provider}` | admin | remove the workspace's key |
| `POST /v1/workspace/ai/{provider}/check` | admin | try the key in use, at no cost |
| `GET /v1/workspace/ai/usage?days=30` | admin | totals by key and by person, and the latest 50 calls |

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
- Sign-in limits and two-factor sign-in: see the Security section. Not yet: SSO (see SCOPE.md).
