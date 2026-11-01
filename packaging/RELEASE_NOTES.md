Ready-to-run Peter. No installation and no admin rights needed.

**Windows:** download `Peter-…-windows.zip`. Before unzipping, right-click it, choose Properties, tick **Unblock**, click OK. Unzip into your Documents folder and double-click **start-peter.cmd**.

**Mac** (Apple chip or Intel, macOS 11+): download `Peter-…-mac.zip`, double-click to unzip, then right-click **start-peter.command** and choose **Open** (the first time only).

Peter opens in your browser at http://localhost:5173. Read **README-FIRST.txt** in the zip for the Chrome extension and AI keys. Full guides: `docs/SETUP-WINDOWS.md` and `docs/SETUP-MAC.md` in the repository.

**What's new in 0.1.3**
- **Controls:** find the "shall" and "must" statements in a project's SOPs as numbered controls, and see in each session which controls (and which regulation clauses) are met, failed, untested or not covered.
- **Sharing and notifications:** share a session or document with a colleague, ask them to review and sign, @mention people in comments, and see it all under Notifications.
- **AI and usage:** add your workspace's own OpenRouter or Gemini key (stored encrypted) and see every AI draft with its tokens and cost.
- **PDF export** of any document, with your logo, footer and page numbers.
- **Writing style:** Peter learns your company's house style from a few past documents, and drafts follow it.

When updating, copy your old `data` folder (and `my-keys` file) into the new one. The `data` folder now also holds `secret.key`, which unlocks saved AI keys: keep it with your data.

**Earlier, in 0.1.2**
- Uploaded files, screenshots, logos and exports are now kept inside Peter's database, so one file holds everything (older files on disk are still read).
- Sessions offer a **Download it** link for the Chrome extension, already set to the right address.
- The sign-in page explains invite-only and first-admin sign-up.
- Unknown API addresses answer with a clear "No such API endpoint" message.
- Online hosting for testers (Render + Supabase, free): see `docs/DEPLOY.md` in the repository.
