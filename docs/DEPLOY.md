# Put Peter online for testers (free)

This puts Peter on the internet so people you invite can use it from their
own browsers. Everything here is free; only your OpenRouter (or Gemini) usage
costs money.

| Piece | Service | Cost |
| --- | --- | --- |
| Peter's server and web app | **Render** free web service | Free, no card |
| Keeping your data safe | **Supabase** Storage | Free, no card |
| AI drafting | **OpenRouter** | Your key, pay per use |

Vercel is not needed: Peter's server also serves the web app.

## What to expect on the free plans

- **Render sleeps after about 15 minutes without visitors.** The next visit
  wakes it, which takes up to a minute. Then it is quick again.
- **Render's free disk is wiped whenever Peter restarts.** Peter copies its
  database to Supabase Storage about once a second while it runs, and
  restores it automatically when it starts. At most the last second or so of
  work can be lost if the server is stopped abruptly.
- **Supabase pauses free projects after a week without use.** If Peter cannot
  start (Render logs show backup errors), sign in to Supabase and click
  **Restore** on the project.
- **Peter is invite-only.** Only the administrator email you set can create
  the first account; everyone else joins through invites. This protects your
  AI credit.
- **No emails are sent.** Invite links are shown on screen for you to pass on.
  Password reset links appear in Render's **Logs**; if a tester forgets their
  password, find their reset link there, or remove and re-invite them.

## Step 1. Supabase: a place for the backup (5 minutes)

1. Go to **https://supabase.com**, click **Start your project**, and sign up
   (GitHub sign-in is easiest).
2. Click **New project**. Any name (for example `peter`), any database
   password (Peter does not use it), the region closest to you. Click
   **Create new project** and wait a minute.
3. In the left menu open **Storage**, click **New bucket**, name it
   **`peter-backup`**, keep it **private** (Public off), and create it.
4. Open **Project Settings** (gear icon) → **Storage**. Under
   **S3 Connection**:
   - make sure the S3 connection is **enabled**;
   - note the **Endpoint** (it ends in `/storage/v1/s3`) and the **Region**;
   - under **S3 Access Keys**, click **New access key**, then copy the
     **Access key ID** and the **Secret access key**. The secret is shown
     only once.

Keep these four values for Step 2. Treat the access keys like passwords.

## Step 2. Render: run Peter (10 minutes, then a 10–15 minute first build)

1. Go to **https://render.com** and sign up with **GitHub** (no card needed).
   When GitHub asks which repositories Render may see, allow
   **`bageshdpl/peter`**.
2. In the Render dashboard click **New** → **Blueprint**, choose the
   **`peter`** repository, and click **Connect**. Render reads the repository's
   `render.yaml` and shows one free web service called **peter**.
3. Fill in the values it asks for:

   | Name | What to put |
   | --- | --- |
   | `PETER_ADMIN_EMAIL` | the email you will sign in with as administrator |
   | `OPENROUTER_API_KEY` | your OpenRouter key (leave empty to try without AI) |
   | `BACKUP_S3_BUCKET` | `peter-backup` |
   | `BACKUP_S3_ENDPOINT` | the Supabase **Endpoint** from Step 1 |
   | `BACKUP_S3_REGION` | the Supabase **Region** from Step 1 |
   | `BACKUP_S3_ACCESS_KEY_ID` | the Supabase **Access key ID** |
   | `BACKUP_S3_SECRET_ACCESS_KEY` | the Supabase **Secret access key** |

   To use Gemini as well, after the first deploy open the service's
   **Environment** page and add `GEMINI_API_KEY`.

   Two more values are filled in for you: `PETER_SECRET_KEY` (made up by
   Render; it encrypts workspaces' own AI keys, so never change it) and
   `PETER_SHARED_AI_MONTHLY_TOKENS` (each workspace may use 300,000 tokens a
   month on your OpenRouter key; change it to suit, or set
   `PETER_SHARED_AI=false` so only workspaces with their own key can draft).
4. Click **Apply**. The first build compiles Peter and takes 10–15 minutes;
   you can watch it under **Logs**. Later deploys are similar.
5. When it says **Live**, your address is at the top of the service page,
   like `https://peter-abcd.onrender.com`. In **Logs** you should see
   `Peter: starting, with continuous backup to bucket peter-backup.`

Every push to `main` on GitHub redeploys Peter automatically. Your data
survives redeploys through the backup.

If you deployed before `PETER_SECRET_KEY` was added, check the service's
**Environment** page after the next deploy. If it is missing, add it yourself
with any long random text (40 characters or more) and keep it unchanged.

## Step 3. Set up Peter and invite testers

1. Open your Render address. On the sign-in page click
   **Create the administrator account** and use the **`PETER_ADMIN_EMAIL`**
   address.
2. Recommended: open **Security** and turn on two-factor sign-in for yourself.
3. Optional: in **Library**, choose your regulations, templates and branding.
4. For each tester: **Members** → enter their email and a role (**member** to
   do validation work) → **Send invite** → copy the invite link and send it to
   them (email, chat). The link works once, for 7 days.

## Step 4. What testers do

1. Open the invite link, choose a password, and join.
2. To record a system, they need the Peter Observer extension (Chrome or
   Edge on a computer). In any session, step 1 shows **Download it**:
   download the zip, unzip it, open `chrome://extensions`, turn on
   **Developer mode**, click **Load unpacked**, and choose the unzipped
   `peter-observer` folder. It is already set up for your Peter's address.
   Then click **Connect extension** in the session.

## Checking it is healthy

- `https://<your-address>/healthz` answers `ok`.
- Render **Logs** show `starting, with continuous backup`. If they say
  `BACKUP_S3_BUCKET is not set`, data will be lost on the next restart: fix
  the environment values.
- In Supabase **Storage → peter-backup** you see a `peter` folder appear a
  minute after the first sign-up.

## If the backup to Supabase does not work

Peter's backup speaks the standard S3 protocol, which Supabase Storage offers.
The automatic checks test backup and restore against an S3 test server, not
against Supabase itself. If Render's logs show backup errors that a correct
endpoint, region and keys do not fix, use **Backblaze B2** instead (free up to
10 GB, no card): create a private bucket and an application key, then set
`BACKUP_S3_ENDPOINT` to the bucket's S3 endpoint (with `https://`),
`BACKUP_S3_REGION` to the region in it (for example `us-west-004`), and the
key ID and key as the access key values.

## Running elsewhere

The repository's `Dockerfile` runs on any Docker host (Fly.io, Railway, a VPS).
It listens on `$PORT` (default 8080) and keeps data in `/data`. Hosts with a
persistent disk do not need the `BACKUP_S3_*` values; set `PETER_APP_URL` to
the public address and `PETER_ADMIN_EMAIL` for the first account.
