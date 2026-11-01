import { PETER_URL } from "./config.js";

const $ = (id) => document.getElementById(id);
// The same words the Peter session page uses.
const KIND_LABEL = { navigate: "Page", click: "Click", input: "Typed", select: "Chose", submit: "Submit", note: "Note", capture: "Screenshot" };
let current = null; // Latest status from the background.
let sessions = [];
// The active tab, kept up to date so the Record click can ask for permission
// at once (Chrome only shows the prompt straight after a click).
let here = { id: null, origin: null };

function send(message) {
  return new Promise((resolve) => chrome.runtime.sendMessage(message, (r) => resolve(r ?? { ok: false, error: "No answer." })));
}

function showError(message) {
  $("error").textContent = message || "";
  $("error").hidden = !message;
}

async function activeTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab;
}

function originOf(url) {
  try {
    const u = new URL(url);
    return u.protocol === "http:" || u.protocol === "https:" ? u.origin : null;
  } catch {
    return null;
  }
}

async function loadSessions() {
  const r = await send({ type: "list-sessions" });
  if (!r.ok) return showError(r.error);
  sessions = r.sessions;
  renderSessions();
}

function renderSessions() {
  const select = $("session");
  const chosen = current?.target?.sessionId ?? "";
  select.replaceChildren(
    new Option("Choose a session…", ""),
    ...sessions.map((s) => new Option(s.project_name ? `${s.title} — ${s.project_name}` : s.title, s.id)),
  );
  // Keep a session chosen from the Peter page even if it is not in the list yet.
  if (chosen && !sessions.some((s) => s.id === chosen)) select.add(new Option(current.target.title || "Chosen session", chosen));
  select.value = chosen;
  $("no-sessions").hidden = sessions.length > 0 || !!chosen;
}

async function render(status) {
  const previousUser = current?.user?.id;
  current = status;
  $("disconnected").hidden = status.connected;
  $("connected").hidden = !status.connected;
  if (!status.connected) return;

  $("who").textContent = `${status.user.name} (${status.user.email})`;
  $("workspace").textContent = status.workspace.name;
  if (previousUser !== status.user.id) loadSessions();
  else renderSessions();

  const rec = status.recording;
  const tab = await activeTab();
  const origin = originOf(tab?.url ?? "");
  here = { id: tab?.id ?? null, origin };
  if (rec) {
    const dot = rec.paused ? "paused" : "rec";
    $("state").innerHTML = `<span class="dot ${dot}"></span>${rec.paused ? "Paused" : "Recording"}`;
    $("site").textContent = rec.paused
      ? `The tab left ${rec.origin}. Recording continues when it returns.`
      : `Recording ${rec.origin} into “${status.target?.title ?? "session"}”.`;
  } else {
    $("state").innerHTML = `<span class="dot"></span>Not recording`;
    $("site").textContent = origin ? `This tab: ${origin}` : "Open the system you are validating in this tab.";
  }
  $("shots-row").hidden = !!rec;
  $("shots-hint").hidden = !!rec || !$("shots").checked;
  $("start").hidden = !!rec;
  $("stop").hidden = !rec;
  $("start").disabled = !origin || !status.target;
  $("extras").hidden = !rec;

  $("counts").textContent = `${status.sent} sent${status.queued ? `, ${status.queued} waiting` : ""}${status.failed ? `, ${status.failed} failed` : ""}`;
  $("recent").replaceChildren(
    ...status.recent.map((s) => {
      const li = document.createElement("li");
      const seq = Object.assign(document.createElement("span"), { className: "seq", textContent: `#${s.seq}` });
      const kind = Object.assign(document.createElement("span"), { className: "kind", textContent: KIND_LABEL[s.kind] ?? s.kind });
      const text = Object.assign(document.createElement("span"), { className: "text", textContent: s.label || "" });
      li.append(seq, kind, text);
      return li;
    }),
  );
  showError(status.lastError ? `Last problem: ${status.lastError}` : "");
}

// Live status from the background.
const port = chrome.runtime.connect({ name: "panel" });
port.onMessage.addListener(render);
chrome.tabs.onActivated.addListener(() => current && render(current));
chrome.tabs.onUpdated.addListener((_id, change) => change.url && current && render(current));

$("open-peter").addEventListener("click", () => chrome.tabs.create({ url: PETER_URL }));

$("disconnect").addEventListener("click", async () => {
  if (confirm("Disconnect from Peter? Recording stops.")) await send({ type: "disconnect" });
});

$("refresh").addEventListener("click", loadSessions);

$("session").addEventListener("change", async (e) => {
  const id = e.target.value;
  if (!id) return;
  const s = sessions.find((x) => x.id === id);
  const r = await send({ type: "select-session", sessionId: id, title: s?.title ?? "" });
  if (!r.ok) showError(r.error);
});

$("start").addEventListener("click", async () => {
  showError("");
  const { id, origin } = here;
  if (id === null || !origin) return showError("This page cannot be recorded.");
  const withShots = $("shots").checked;
  // No await before this call, or Chrome refuses to show the prompt.
  const granted = await chrome.permissions.request({ origins: [withShots ? "<all_urls>" : `${origin}/*`] });
  if (!granted) {
    return showError(withShots
      ? "Screenshots need access to all sites. Untick Include screenshots to record this site only."
      : `Recording needs permission for ${origin}.`);
  }
  const r = await send({ type: "start", tabId: id });
  if (!r.ok) showError(r.error);
});

$("stop").addEventListener("click", () => send({ type: "stop" }));

$("shots").addEventListener("change", () => current && render(current));

$("add-note").addEventListener("click", async () => {
  const text = $("note").value.trim();
  if (!text) return;
  const r = await send({ type: "note", text });
  if (r.ok) $("note").value = "";
  else showError(r.error);
});

$("capture").addEventListener("click", async () => {
  const r = await send({ type: "capture" });
  if (!r.ok) showError(r.error);
});
