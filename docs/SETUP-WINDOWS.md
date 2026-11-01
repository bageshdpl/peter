# Run Peter on Windows

A step-by-step guide for Windows 10 or 11. Allow about 30 minutes the first
time; most of it is waiting for installs. Afterwards, starting Peter takes a
minute.

You will use **PowerShell**: press the Windows key, type `PowerShell`, and
open **Windows PowerShell**. Copy each command, paste it with a right-click,
and press Enter.

---

## Part 1. Install the tools (once)

Peter needs four free tools. `winget` is Windows' built-in installer.

**1. Git** (downloads Peter):

```powershell
winget install --id Git.Git -e
```

**2. Node.js** (runs the web app):

```powershell
winget install --id OpenJS.NodeJS.LTS -e
```

**3. Microsoft C++ Build Tools** (Rust needs these to build programs on
Windows). This one is large and can take 10 to 20 minutes:

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

**4. Rust** (runs the Peter server):

```powershell
winget install --id Rustlang.Rustup -e
```

**Close PowerShell and open it again**, so it finds the new tools. Then check
them; each line should print a version number:

```powershell
git --version
node --version
cargo --version
```

`node --version` should show v22 or higher. If a command says it is not
recognised, restart your computer and try again.

---

## Part 2. Download Peter (once)

This puts Peter in a `peter` folder in your user folder
(for example `C:\Users\you\peter`):

```powershell
cd ~
git clone https://github.com/bageshdpl/peter.git
```

Git may open a browser window asking you to sign in to GitHub, because the
repository is private. Sign in with your GitHub account.

---

## Part 3. Start Peter

Peter has two parts that run side by side, each in its own PowerShell window.

### Window 1: the server

```powershell
cd ~\peter\server
cargo run
```

The first time, this downloads and builds everything: expect 3 to 10
minutes. It is ready when you see:

```
Peter API listening on http://127.0.0.1:8080
```

Leave this window open. Peter's data is saved in `peter.db` in this folder.

### Window 2: the web app

Open a **second** PowerShell window:

```powershell
cd ~\peter\web
npm install
npm run dev
```

(`npm install` is only needed the first time and after updates.) It is
ready when you see `Local: http://localhost:5173/`.

### Open Peter

In Chrome, go to **http://localhost:5173** and click **Create an account**.
This account exists only on your computer.

---

## Part 4. Add the Peter Observer extension to Chrome (once)

1. In Chrome, go to `chrome://extensions`.
2. Turn on **Developer mode** (switch at the top right).
3. Click **Load unpacked**.
4. Choose the folder `C:\Users\you\peter\extension` (use your own user name).
5. Click the puzzle-piece icon in Chrome's toolbar and pin **Peter Observer**.

---

## Part 5. Try it

1. In Peter, click **New project** and fill in the system you are validating.
2. Click **New session**, give it a title, keep **Initial validation**, and
   click **Start session**.
3. On step 1, click **Connect extension**. It should then say
   **Extension ready**.
4. Open the system you want to record in a new tab. To practise, any website
   works.
5. Click the Peter Observer icon to open its side panel, and click
   **Record this tab**. Chrome asks for permission the first time (needed for
   screenshots); click **Allow**.
6. Use the website. Each click and entry appears as a step in Peter.
7. Click **Stop** in the side panel when done.
8. In Peter, click **Draft with AI**. Without an AI key, only
   **Sample draft (no AI)** is offered; Part 6 adds real AI.

---

## Part 6. Turn on real AI drafting

You need an API key from at least one of:

- **OpenRouter**: sign up at https://openrouter.ai, then go to
  **Keys** and create a key. It starts with `sk-or-`.
- **Google Gemini**: go to https://aistudio.google.com, then
  **Get API key** and create a key. It usually starts with `AIza`.

> Keep keys private. Never paste a key into a chat, an email, or a file you
> share or commit to GitHub. It only goes into the PowerShell window below.

Stop the server (click Window 1 and press **Ctrl+C**), then start it again
with your key(s):

```powershell
cd ~\peter\server
$env:OPENROUTER_API_KEY = "paste-your-openrouter-key-here"
$env:GEMINI_API_KEY = "paste-your-gemini-key-here"
cargo run
```

Leave out the line for a provider you do not have. The keys last until you
close that window, so you set them each time you start the server.

Reload Peter in Chrome. In a session step, **Draft with** now lists
OpenRouter and/or Google Gemini. Choose one and click **Draft with AI**.

If a draft fails with a message about the model (for example "model not
found"), that model name has been retired. Pick a current one from
openrouter.ai/models or Google AI Studio, then add it before `cargo run`:

```powershell
$env:PETER_OPENROUTER_MODEL = "a-current-model-id"
$env:PETER_GEMINI_MODEL = "a-current-gemini-model-id"
```

---

## Every day after that

Open two PowerShell windows:

```powershell
cd ~\peter\server
cargo run
```

```powershell
cd ~\peter\web
npm run dev
```

(Set your AI keys in the first window before `cargo run` if you want AI
drafting.) Then open http://localhost:5173.

## Getting updates

When there is new work in the repository, stop both windows (Ctrl+C) and run:

```powershell
cd ~\peter
git pull
cd web
npm install
```

Then start Peter as usual. If the extension changed, open
`chrome://extensions` and click the reload icon on Peter Observer.

---

## If something goes wrong

| What you see | What to do |
| --- | --- |
| `'cargo' is not recognized` (or git, node) | Close and reopen PowerShell. If it persists, restart the computer. |
| `linker 'link.exe' not found` while building | The C++ Build Tools (Part 1, step 3) are missing or incomplete. Run that command again, then restart PowerShell. |
| A build error mentioning `cmake` or `nasm` | Run `winget install --id Kitware.CMake -e` and `winget install --id NASM.NASM -e`, restart PowerShell, and run `cargo run` again. |
| `error: address already in use` / port 8080 or 5173 busy | Peter is already running in another window. Close the other window, or restart the computer. |
| The web page says it cannot reach the Peter API | Window 1 (the server) is not running or still building. Check it shows "Peter API listening". |
| Peter says the extension is not installed | Reload the Peter tab. If still not found, check `chrome://extensions` shows Peter Observer switched on. |
| "The extension was updated. Reload this page." | Reload the Peter tab. |
| A draft fails | Read the message under the draft. "No API key" means Part 6 was skipped. A model error means set a current model (Part 6). A quota or billing error comes from the provider's account. |
| Something else | Copy the error text (or take a screenshot) and send it, without any API keys in it. |

To start completely fresh, stop the server and delete `peter.db` and the
`peter-data` folder inside `peter\server`. This deletes all your Peter data.
