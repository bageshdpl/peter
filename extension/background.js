// Service worker: holds the connection to Peter, decides what is recorded,
// takes screenshots, and sends steps to the session in order.
import { PETER_URL } from "./config.js";

chrome.sidePanel.setPanelBehavior({ openPanelOnActionClick: true }).catch(() => {});

// ---------- Stored state ----------
// chrome.storage.local survives restarts: the sign-in and chosen session.
// chrome.storage.session lasts while the browser runs: the recording.

async function getAuth() {
  return (await chrome.storage.local.get("auth")).auth ?? null;
}
async function setAuth(auth) {
  if (auth) await chrome.storage.local.set({ auth });
  else await chrome.storage.local.remove("auth");
}
async function getTarget() {
  return (await chrome.storage.local.get("target")).target ?? null; // { sessionId, title }
}
async function getRecording() {
  return (await chrome.storage.session.get("recording")).recording ?? null; // { tabId, origin, paused }
}
async function setRecording(recording) {
  if (recording) await chrome.storage.session.set({ recording });
  else await chrome.storage.session.remove("recording");
}

// In memory only; the side panel shows it.
const activity = { sent: 0, failed: 0, lastError: null, recent: [] };

// ---------- Talking to Peter ----------

function authFrom(session) {
  return {
    access_token: session.access_token,
    refresh_token: session.refresh_token,
    user: session.user,
    workspace: session.workspace,
  };
}

let refreshing = null;
async function refresh() {
  refreshing ??= (async () => {
    try {
      const auth = await getAuth();
      if (!auth) return false;
      const res = await fetch(`${PETER_URL}/v1/auth/refresh`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ refresh_token: auth.refresh_token }),
      });
      if (!res.ok) {
        await setAuth(null);
        notify();
        return false;
      }
      await setAuth(authFrom(await res.json()));
      return true;
    } finally {
      refreshing = null;
    }
  })();
  return refreshing;
}

class PeterError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

async function peter(method, path, body) {
  for (let attempt = 0; attempt < 2; attempt++) {
    const auth = await getAuth();
    if (!auth) throw new PeterError(401, "Not connected to Peter.");
    const res = await fetch(`${PETER_URL}${path}`, {
      method,
      headers: { Authorization: `Bearer ${auth.access_token}`, ...(body ? { "Content-Type": "application/json" } : {}) },
      body: body ? JSON.stringify(body) : undefined,
    });
    if (res.status === 401 && attempt === 0 && (await refresh())) continue;
    const data = res.status === 204 ? null : await res.json().catch(() => null);
    if (!res.ok) throw new PeterError(res.status, data?.error?.message ?? `Peter answered ${res.status}.`);
    return data;
  }
  throw new PeterError(401, "Your Peter sign-in has expired. Connect again from Peter.");
}

// ---------- Screenshots ----------

// Chrome allows about two captures a second.
let lastCapture = 0;
async function capture(tabId) {
  try {
    // Without access to all sites Chrome refuses screenshots; record the step without one.
    if (!(await chrome.permissions.contains({ origins: ["<all_urls>"] }))) return null;
    const tab = await chrome.tabs.get(tabId);
    if (!tab.active) return null; // Only the visible tab can be captured.
    const wait = lastCapture + 550 - Date.now();
    if (wait > 0) await new Promise((r) => setTimeout(r, wait));
    lastCapture = Date.now();
    return await chrome.tabs.captureVisibleTab(tab.windowId, { format: "jpeg", quality: 70 });
  } catch {
    return null;
  }
}

// ---------- Sending steps, in order ----------

const queue = [];
let draining = false;

function enqueue(step) {
  queue.push({ step, tries: 0 });
  drain();
}

async function drain() {
  if (draining) return;
  draining = true;
  try {
    while (queue.length) {
      const item = queue[0];
      const target = await getTarget();
      if (!target) {
        queue.length = 0;
        break;
      }
      try {
        const saved = await peter("POST", `/v1/sessions/${target.sessionId}/observations`, item.step);
        queue.shift();
        activity.sent += 1;
        activity.lastError = null;
        activity.recent.unshift({ seq: saved.seq, kind: saved.kind, label: saved.target_label || saved.note || saved.page_title, at: Date.now() });
        activity.recent.length = Math.min(activity.recent.length, 12);
      } catch (err) {
        item.tries += 1;
        activity.lastError = err.message;
        // Refusals will not succeed on retry; network trouble might.
        const permanent = err instanceof PeterError && err.status >= 400 && err.status < 500;
        if (permanent || item.tries >= 5) {
          queue.shift();
          activity.failed += 1;
          if (err instanceof PeterError && (err.status === 401 || err.status === 409 || err.status === 404)) {
            await stopRecording();
          }
        } else {
          notify();
          await new Promise((r) => setTimeout(r, 1000 * 2 ** item.tries));
        }
      }
      notify();
    }
  } finally {
    draining = false;
  }
}

const NEEDS_SCREENSHOT = new Set(["click", "submit", "navigate", "capture", "select"]);

async function recordStep(tabId, step) {
  const recording = await getRecording();
  if (!recording || recording.paused || recording.tabId !== tabId) return;
  const { skip_screenshot: skip, ...clean } = step;
  const screenshot = NEEDS_SCREENSHOT.has(step.kind) && !skip ? await capture(tabId) : null;
  enqueue(screenshot ? { ...clean, screenshot } : clean);
}

// ---------- Recording ----------

function originOf(url) {
  try {
    const u = new URL(url);
    return u.protocol === "http:" || u.protocol === "https:" ? u.origin : null;
  } catch {
    return null;
  }
}

async function inject(tabId) {
  await chrome.scripting.executeScript({ target: { tabId }, files: ["recorder.js"] });
}

async function startRecording(tabId) {
  const tab = await chrome.tabs.get(tabId);
  const origin = originOf(tab.url);
  if (!origin) throw new Error("This page cannot be recorded. Open the system you are validating first.");
  if (origin === new URL(PETER_URL).origin) throw new Error("This is Peter itself. Open the system you are validating in this tab.");
  if (!(await chrome.permissions.contains({ origins: [`${origin}/*`] }))) {
    throw new Error(`Peter Observer needs your permission to record ${origin}.`);
  }
  if (!(await getTarget())) throw new Error("Choose a session to record into first.");
  await setRecording({ tabId, origin, paused: false });
  await inject(tabId);
  await recordStep(tabId, { kind: "navigate", url: tab.url, page_title: tab.title ?? "", target_label: "Recording started", captured_at_ms: Date.now() });
  notify();
}

async function stopRecording() {
  const recording = await getRecording();
  await setRecording(null);
  if (recording) {
    try {
      await chrome.scripting.executeScript({ target: { tabId: recording.tabId }, func: () => window.__peterRecorder?.stop() });
    } catch {
      // The tab may be gone or on a page we may not touch.
    }
  }
  notify();
}

// Follow the recorded tab as it moves between pages.
chrome.tabs.onUpdated.addListener(async (tabId, change, tab) => {
  const recording = await getRecording();
  if (!recording || recording.tabId !== tabId || change.status !== "complete") return;
  const inScope = originOf(tab.url) === recording.origin;
  if (!inScope) {
    // Left the allowed site: record nothing until it comes back.
    if (!recording.paused) await setRecording({ ...recording, paused: true });
    notify();
    return;
  }
  if (recording.paused) await setRecording({ ...recording, paused: false });
  try {
    await inject(tabId);
  } catch {
    return;
  }
  await recordStep(tabId, { kind: "navigate", url: tab.url, page_title: tab.title ?? "", captured_at_ms: Date.now() });
  notify();
});

chrome.tabs.onRemoved.addListener(async (tabId) => {
  const recording = await getRecording();
  if (recording?.tabId === tabId) await stopRecording();
});

// ---------- Status for the side panel and the Peter page ----------

async function status() {
  const [auth, target, recording] = await Promise.all([getAuth(), getTarget(), getRecording()]);
  return {
    ok: true,
    connected: !!auth,
    user: auth?.user ?? null,
    workspace: auth?.workspace ?? null,
    target,
    recording,
    queued: queue.length,
    ...activity,
  };
}

const panels = new Set();
chrome.runtime.onConnect.addListener((port) => {
  if (port.name !== "panel") return;
  panels.add(port);
  port.onDisconnect.addListener(() => panels.delete(port));
  status().then((s) => port.postMessage(s));
});

async function notify() {
  if (!panels.size) return;
  const s = await status();
  for (const port of panels) port.postMessage(s);
}

// ---------- Messages ----------

const fromPeterPage = (sender) => sender.url && originOf(sender.url) === new URL(PETER_URL).origin;
// Extension pages (the side panel) have the extension's own URL; content
// scripts report the URL of the web page they run in.
const fromExtension = (sender) => sender.id === chrome.runtime.id && !!sender.url?.startsWith(chrome.runtime.getURL(""));

async function handle(msg, sender) {
  switch (msg?.type) {
    case "status":
      return status();

    case "connect": {
      if (!fromPeterPage(sender) && !fromExtension(sender)) return { ok: false, error: "Not allowed." };
      const res = await fetch(`${PETER_URL}/v1/auth/extension-exchange`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ code: msg.code }),
      });
      const data = await res.json().catch(() => null);
      if (!res.ok) return { ok: false, error: data?.error?.message ?? "Could not connect." };
      const previous = await getAuth();
      await setAuth(authFrom(data));
      // A different person or workspace must not inherit the old session choice.
      if (previous && (previous.user.id !== data.user.id || previous.workspace.id !== data.workspace.id)) {
        await chrome.storage.local.remove("target");
        await stopRecording();
      }
      notify();
      return { ok: true, user: data.user, workspace: data.workspace };
    }

    case "select-session": {
      if (!fromPeterPage(sender) && !fromExtension(sender)) return { ok: false, error: "Not allowed." };
      if (!(await getAuth())) return { ok: false, error: "Connect the extension first." };
      await chrome.storage.local.set({ target: { sessionId: msg.sessionId, title: msg.title } });
      notify();
      return { ok: true };
    }

    case "disconnect": {
      if (!fromExtension(sender)) return { ok: false };
      await stopRecording();
      try {
        await peter("POST", "/v1/auth/logout");
      } catch {
        // Signed out locally either way.
      }
      await setAuth(null);
      await chrome.storage.local.remove("target");
      notify();
      return { ok: true };
    }

    case "list-sessions": {
      if (!fromExtension(sender)) return { ok: false };
      const data = await peter("GET", "/v1/sessions");
      return { ok: true, sessions: data.sessions };
    }

    case "start": {
      if (!fromExtension(sender)) return { ok: false };
      await startRecording(msg.tabId);
      return { ok: true };
    }

    case "stop":
      if (!fromExtension(sender)) return { ok: false };
      await stopRecording();
      return { ok: true };

    case "note": {
      if (!fromExtension(sender)) return { ok: false };
      const recording = await getRecording();
      const tab = recording ? await chrome.tabs.get(recording.tabId).catch(() => null) : null;
      enqueue({ kind: "note", note: String(msg.text ?? ""), url: tab?.url ?? "", page_title: tab?.title ?? "", captured_at_ms: Date.now() });
      return { ok: true };
    }

    case "capture": {
      if (!fromExtension(sender)) return { ok: false };
      const recording = await getRecording();
      if (!recording || recording.paused) return { ok: false, error: "Start recording on the system first." };
      const tab = await chrome.tabs.get(recording.tabId);
      await recordStep(recording.tabId, { kind: "capture", url: tab.url, page_title: tab.title ?? "", target_label: "Screenshot", captured_at_ms: Date.now() });
      return { ok: true };
    }

    case "step":
      // From the recorder inside the recorded tab.
      if (sender.tab?.id !== undefined) await recordStep(sender.tab.id, msg.step);
      return { ok: true };

    default:
      return { ok: false, error: "Unknown request." };
  }
}

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  handle(msg, sender)
    .then(sendResponse)
    .catch((err) => sendResponse({ ok: false, error: err.message }));
  return true; // Answer asynchronously.
});
