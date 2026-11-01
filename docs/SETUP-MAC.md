# Run Peter on a Mac

Works on Apple-chip (M1 and later) and Intel Macs with macOS 11 or later. No
installation and no admin password needed.

## 1. Download

1. Go to **https://github.com/bageshdpl/peter/releases/latest** and sign in
   to GitHub (the repository is private: your account needs access, see
   *Using Peter from another GitHub account* in the main README).
2. Under **Assets**, download `Peter-<version>-mac.zip`.
3. Double-click the zip in Downloads to unzip it. Move the `Peter` folder
   somewhere you keep documents, for example **Documents**.

## 2. Start Peter

The first time only:

1. Open the `Peter` folder.
2. **Right-click** (or Control-click) **`start-peter.command`** and choose
   **Open**, then click **Open** in the dialog.

A Terminal window opens (keep it open: closing it stops Peter), and your
browser opens **http://localhost:5173**. Click **Create an account**; the
account exists only on this Mac.

After the first time, a normal double-click on `start-peter.command` works.

If macOS says the file "cannot be opened" even with right-click → Open: open
**System Settings → Privacy & Security**, scroll down, and click
**Open Anyway** next to `start-peter.command`.

## 3. Add the Peter Observer extension to Chrome (once)

1. In Chrome, go to `chrome://extensions`.
2. Turn on **Developer mode** (top right).
3. Click **Load unpacked** and choose the **`extension`** folder inside your
   `Peter` folder.
4. Click the puzzle-piece icon and pin **Peter Observer**.

Then follow *Part 5. Try it* in `docs/SETUP-WINDOWS.md` (the steps inside
Peter are the same on a Mac).

## 4. Turn on AI drafting (optional)

1. Get a key from **OpenRouter** (https://openrouter.ai, **Keys**) and/or
   **Google AI Studio** (https://aistudio.google.com, **Get API key**).
2. In the `Peter` folder, duplicate **`my-keys.example.sh`** (select it, press
   ⌘D) and rename the copy to **`my-keys.sh`**.
3. Right-click `my-keys.sh` → **Open With** → **TextEdit**. On the line for
   your provider, delete `# ` at the start and paste your key between the
   quotes. Save.
4. Close the Terminal window and start Peter again.

> Keep `my-keys.sh` private. Never email, upload or share it.

## Your data and updates

- Everything is saved in the `Peter` folder: `peter.db` and the `peter-data`
  folder. Back them up to keep your work.
- To update: unzip the new version to a new folder, then copy `peter.db`,
  `peter-data` and `my-keys.sh` into it from the old folder. Reload the
  extension at `chrome://extensions` from the new `extension` folder.

## If something goes wrong

| What you see | What to do |
| --- | --- |
| "cannot be opened because it is from an unidentified developer" | Right-click → **Open** instead of double-clicking, or use **Open Anyway** in Privacy & Security. |
| "peter-server cannot be opened" | Open Terminal, type `cd ` (with a space), drag the `Peter` folder onto the Terminal window, press Enter, then run `xattr -dr com.apple.quarantine .` and start Peter again. |
| `address already in use` | Peter is already running in another Terminal window. Close it. |
| The browser cannot reach the page | Wait a few seconds and refresh; check the Terminal window is still open. |

## Building from source on a Mac (developers)

Needs Apple's Command Line Tools (`xcode-select --install`, which may ask for
an admin password), Rust (https://rustup.rs) and Node.js 22. Then follow the
main README: `cd server && cargo run` and `cd web && npm install && npm run dev`.
