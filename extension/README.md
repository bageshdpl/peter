# Peter Observer (Chrome extension)

Records the system you are validating into a Peter session: every click,
typed value, choice, form submission and page, with screenshots. Steps appear
live on the session page in Peter.

Plain JavaScript, no build step. Chrome 116 or later.

## Install (unpacked)

1. Start the Peter API and web app (see `apps/peter/README.md`).
2. In Chrome, open `chrome://extensions`.
3. Turn on **Developer mode** (top right).
4. Click **Load unpacked** and choose this folder (`apps/peter/extension`).
5. Pin **Peter Observer** to the toolbar if you like.

After changing the files, click the reload icon on the extension's card, then
reload any open Peter tabs.

## Use

1. In Peter, open a session and click **Connect extension**. Peter hands the
   extension a one-time code (valid 5 minutes); your password is never shared.
   The session becomes the one the extension records into.
2. Open the system you are validating in a tab.
3. Click the Peter Observer icon to open the side panel, and press
   **Record this tab**.
4. Use the system. Each step appears in the side panel and on the Peter
   session page. Add notes or extra screenshots from the side panel.
5. Press **Stop** when done.

## What is recorded

| Step | When | Screenshot |
| --- | --- | --- |
| Page | recording starts, or the tab loads a new page on the same site | yes |
| Click | buttons, links, checkboxes, radio buttons, menu items | yes, with the element boxed in orange |
| Typed | a text field changes (recorded when you leave the field) | no |
| Chose | a dropdown changes | yes |
| Submit | a form is submitted other than by clicking its button | yes |
| Note | you add one in the side panel | no |
| Screenshot | you press **Take screenshot** | yes |

A click that leaves the page (a link, a sign-in button) is recorded without a
screenshot, because by then the old page is gone; the next **Page** step shows
where it led.

## Privacy and scope

- Only the tab you started recording is recorded, and only while it stays on
  the site you started on. If it goes elsewhere, recording pauses until it
  comes back.
- Values of password fields, hidden fields, and fields that look secret (by
  type, autocomplete, or names such as password, token, OTP, PIN, card, CVV,
  IBAN) are never sent; the step says "Hidden". Those fields are also shown as
  dots on screen while recording, so screenshots cannot show them.
- Add `data-peter-mask` to any element to mask the fields inside it.

## Permissions

| Permission | Why |
| --- | --- |
| `sidePanel` | the recording panel |
| `storage` | your connection and chosen session |
| `scripting` | add the recorder to the tab you record |
| `tabs` | follow the recorded tab between pages, show its address |
| `localhost:5173` | talk to Peter |
| all sites (optional, asked when you press Record) | Chrome allows screenshots only with access to all sites |

If you untick **Include screenshots** before pressing Record, the extension
asks for access to that one site only and records steps without screenshots.
Either way, only the tab and site you chose are recorded.

## Pointing at another Peter address

Peter's address is `http://localhost:5173` by default. To change it, edit
`PETER_URL` in `config.js`, and the `host_permissions` and `content_scripts`
`matches` in `manifest.json`.

## Known limits

- Typing is recorded per field when the field changes, not per keystroke.
- Steps waiting to be sent are kept in memory; if Chrome stops the extension
  while it has unsent steps (rare, as sending takes well under a second), they
  are lost.
- Pages inside iframes from other sites are not recorded.
