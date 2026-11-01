# Peter: scope

Peter is a GxP computer-system validation workspace. A tester runs the system
under validation in their browser while a browser extension observes it. Peter
keeps the observed steps and screenshots as evidence and uses AI to draft
validation documents that cite that evidence.

Peter is modelled on the publicly observable behaviour of Catalyst
(askcatalyst.com). It is written from scratch: no Catalyst code, styles, text,
templates or assets are copied.

## How the reference product is put together

Learned from its public web bundle and its public Firefox extension manifest.

| Part | Reference product | Peter |
| --- | --- | --- |
| Web app | React SPA, built with Vite | React + Vite + TypeScript |
| API | REST under `/v1`, live updates over Server-Sent Events | Rust (Axum), REST under `/v1`, SSE |
| Data | unknown (server side) | SQLite locally, Postgres later (SQLx) |
| Extension | Manifest V3, side panel, step recorder, screenshot capture, redaction, per-site permission | Manifest V3 in TypeScript, Chrome first |
| AI | AWS Bedrock, or the customer's own Bedrock (BYOK) | provider layer: OpenRouter and Gemini first |
| Exports | Word, Excel, PDF, ZIP | Word and Excel from Rust, PDF later |

## Validation types

Each project picks a validation type. Each type has its own guided steps, and
each step produces one or more documents.

| Type | Guided steps |
| --- | --- |
| Initial validation | Observe & ground, System assessment, Requirements & risk, Functional & design spec, Configuration spec, Validation plan, Build & installation (IQ), Testing, Traceability, Summary report |
| Change validation (configuration, new function, upgrade or patch, migration) | What changed, Earlier validation, Impact assessment, Test scope, Update documents, Change summary |
| Periodic review | Changes & events, Controls check, Review report |
| Data migration | Migration plan, Field mapping, Verification plan, Migration report |
| Retirement | Retirement plan, Archive plan, Shutdown, Retirement report |
| AI validation | Intended use, Context & risk, Data & model, Requirements & acceptance, Performance testing, Monitoring |
| Equipment | Equipment assessment, then the remaining steps for its kind |
| Dashboards & reports | Use assessment, then the remaining steps for its use |
| Procedures | Procedure list, Administration, Backup & recovery, Training, Work instructions |

## Modules

Each module is decided on its own: in, later, or out.

### M0. Foundation (done, see README.md)
- Rust workspace, configuration, database migrations, structured errors.
- Email and password sign-in (Argon2), access and refresh tokens, logout, `me`.
- Workspaces: create, switch, invite members, accept invite.
- Roles: owner, admin, member, viewer.

### M1. Projects and sessions (done)
- Projects (the system being validated), archive.
- Sessions: one validation run in a project, with its validation type.
- Unfiled sessions, project sources (uploaded files).

### M2. Observer extension (done)
- Side panel: sign in to Peter (a short-lived extension token), pick a session, start and stop recording.
- Records clicks, typed values, navigation and page titles as steps.
- Screenshot per step, with sensitive fields masked (passwords always).
- Only records on sites the user allows (per-site permission).
- Sends observations to the API; the web app shows them live (SSE).
- Web app detects whether the extension is installed.

### M3. Guided workflow (done)
- The validation types and their steps above.
- Move forward, back, or jump to a step; mark a step reviewed or skipped.

### M4. AI drafting (done; testing-style preferences later)
- Provider layer: OpenRouter and Gemini first, any OpenAI-compatible API.
- Start, stream, and stop a draft for a step; every claim cites an observation.
- Chat with the session and with a single step.
- Workspace "testing style" preferences learned from sample documents.

### M5. Documents (done; structured traceability done; PDF later)
- Document editor with versions, comments (resolve, reopen) and review marks.
- Traceability: every requirement linked to a test and its evidence.
- Export to Word and Excel; PDF later.

### M6. Library (done: regulations, templates, branding)
- Templates, reference documents, standards (for example 21 CFR Part 11, EU Annex 11, GAMP 5).
- Workspace branding: logo, document header and footer template.

### M7. Controls (done)
- Extract controls from SOPs and standards; map them to requirements; coverage view.

### M8. Sharing and collaboration (done)
- Share a document or session with a person; "shared with me"; notifications count.

### M9. Administration and enterprise (sign-in limits, two-factor sign-in and active sessions done)
- Two-factor sign-in, password reset, change password.
- Audit trail of every change.
- Security settings, usage summary.
- Bring-your-own AI key (done), with a usage summary (done).
- SSO (OIDC, SAML), SCIM provisioning.
- Billing (Stripe), seats, trial.
- Support requests, feedback, release notes, help videos, tutorial with a practice system.
- Voice notes during a session, with custom vocabulary.
- Apps (a bug reporter that files to Jira), workspace memory.

## Recommended first release

| Module | Decision |
| --- | --- |
| M0 Foundation | in |
| M1 Projects and sessions | in |
| M2 Observer extension | in (Chrome only) |
| M3 Guided workflow | in, Initial validation only |
| M4 AI drafting | in, without testing-style preferences |
| M5 Documents | in, Word export only |
| M6 Library | later |
| M7 Controls | done |
| M8 Sharing | done |
| M9 Administration | password reset and audit trail in; the rest later or out |

The first milestone inside that release is the core loop: sign in, create a
project and session, record with the extension, see steps arrive live.
