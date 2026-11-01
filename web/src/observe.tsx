import { useCallback, useEffect, useRef, useState } from "react";
import { api, fetchBlobUrl, serverMeta, type Observation } from "./api";
import type { SessionEvents } from "./events";
import { messageOf, Notice } from "./ui";

// ---------- Talking to the Peter Observer extension ----------
// The extension's bridge script runs on this page and answers window messages.

interface ExtensionStatus {
  connected: boolean;
  user: { id: string; email: string } | null;
  workspace: { id: string } | null;
  target: { sessionId: string; title: string } | null;
  recording: { origin: string; paused: boolean } | null;
}

type Reply = { source: "peter-observer"; type: string; [key: string]: unknown };

function askExtension(message: Record<string, unknown>, expect: string, timeoutMs = 4000): Promise<Reply | null> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      window.removeEventListener("message", onMessage);
      resolve(null);
    }, timeoutMs);
    function onMessage(event: MessageEvent) {
      const data = event.data as Reply | undefined;
      if (event.source !== window || data?.source !== "peter-observer" || data.type !== expect) return;
      clearTimeout(timer);
      window.removeEventListener("message", onMessage);
      resolve(data);
    }
    window.addEventListener("message", onMessage);
    window.postMessage({ source: "peter-app", ...message }, window.location.origin);
  });
}

/** Whether the extension is installed, and what it is connected to. */
export function useExtension() {
  const [state, setState] = useState<{ checked: boolean; installed: boolean; status: ExtensionStatus | null }>({
    checked: false,
    installed: false,
    status: null,
  });

  const check = useCallback(async () => {
    const pong = await askExtension({ type: "ping" }, "pong", 1500);
    setState({ checked: true, installed: !!pong, status: (pong?.status as ExtensionStatus) ?? null });
  }, []);

  useEffect(() => {
    check();
    // Notice the extension being installed or changed while the page is open.
    const onHello = (e: MessageEvent) => {
      if (e.source === window && (e.data as Reply)?.source === "peter-observer" && (e.data as Reply).type === "hello") check();
    };
    window.addEventListener("message", onHello);
    const timer = setInterval(check, 5000);
    return () => {
      window.removeEventListener("message", onHello);
      clearInterval(timer);
    };
  }, [check]);

  return { ...state, check };
}

// ---------- The observation panel on the session page ----------

const KIND_LABEL: Record<string, string> = {
  navigate: "Page",
  click: "Click",
  input: "Typed",
  select: "Chose",
  submit: "Submit",
  note: "Note",
  capture: "Screenshot",
};

function describe(o: Observation) {
  switch (o.kind) {
    case "navigate":
      return o.target_label || o.page_title || o.url;
    case "input":
    case "select":
      return o.target_label;
    case "note":
      return o.note;
    default:
      return o.target_label || o.page_title;
  }
}

function shortUrl(url: string) {
  try {
    const u = new URL(url);
    return u.host + u.pathname;
  } catch {
    return url;
  }
}

export function ObservationPanel(props: {
  sessionId: string;
  sessionTitle: string;
  userId: string;
  workspaceId: string;
  canRecord: boolean;
  events: SessionEvents;
}) {
  const { sessionId, sessionTitle, userId, workspaceId, canRecord, events } = props;
  const { live, subscribe } = events;
  const ext = useExtension();
  const [canDownload, setCanDownload] = useState(false);
  useEffect(() => {
    serverMeta().then((m) => setCanDownload(m.extension_download));
  }, []);
  const [steps, setSteps] = useState<Observation[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [zoom, setZoom] = useState<Observation | null>(null);
  const base = `/v1/sessions/${sessionId}/observations`;

  const reload = useCallback(async () => {
    try {
      setSteps((await api<{ observations: Observation[] }>("GET", base)).observations);
    } catch (err) {
      setError(messageOf(err));
    }
  }, [base]);

  // Follow new steps live. Each (re)connection reloads, so nothing is missed
  // while the stream was down.
  useEffect(() => {
    reload();
    return subscribe((name, data) => {
      if (name === "open" || name === "resync") {
        reload();
      } else if (name === "observation") {
        const o = data as Observation;
        setSteps((prev) => (prev.some((p) => p.id === o.id) ? prev : [...prev, o].sort((a, b) => a.seq - b.seq)));
      } else if (name === "observation_removed") {
        const { id } = data as { id: string };
        setSteps((prev) => prev.filter((p) => p.id !== id));
      }
    });
  }, [subscribe, reload]);

  const status = ext.status;
  // The extension is usable here only if it is signed in as this person, in this workspace.
  const connectedHere = !!status?.connected && status.user?.id === userId && status.workspace?.id === workspaceId;
  const targetsThis = connectedHere && status?.target?.sessionId === sessionId;

  async function connect() {
    setBusy(true);
    setError(null);
    try {
      if (!connectedHere) {
        const { code } = await api<{ code: string }>("POST", "/v1/auth/extension-code");
        const res = await askExtension({ type: "connect", code }, "connect-result");
        if (!res?.ok) throw new Error((res?.error as string) ?? "The extension did not answer. Reload this page.");
      }
      const sel = await askExtension({ type: "select-session", sessionId, title: sessionTitle }, "select-result");
      if (!sel?.ok) throw new Error((sel?.error as string) ?? "The extension did not answer.");
      await ext.check();
    } catch (err) {
      setError(err instanceof Error ? err.message : messageOf(err));
    } finally {
      setBusy(false);
    }
  }

  async function remove(o: Observation) {
    if (!confirm(`Remove step ${o.seq}? It stays in the record as removed by you.`)) return;
    try {
      await api("DELETE", `${base}/${o.id}`);
      setSteps((prev) => prev.filter((p) => p.id !== o.id));
    } catch (err) {
      setError(messageOf(err));
    }
  }

  return (
    <section className="card">
      <div className="card-head">
        <div>
          <h2>Observation</h2>
          <p className="muted">
            Record the system with the Peter Observer extension. Each click, entry and page becomes a step with a
            screenshot.
          </p>
        </div>
        <span className={`live ${live ? "on" : ""}`}>{live ? "Live" : "Offline"}</span>
      </div>

      {canRecord && (
        <div className="extension-bar">
          {!ext.checked ? (
            <span className="muted">Looking for the extension…</span>
          ) : !ext.installed ? (
            canDownload ? (
              <span>
                The Peter Observer extension is not installed in this browser.{" "}
                <a href="/v1/extension/download" download>Download it</a>, unzip it, then in Chrome open{" "}
                <code>chrome://extensions</code>, turn on <b>Developer mode</b>, click <b>Load unpacked</b> and choose the
                unzipped <code>peter-observer</code> folder. Then reload this page.
              </span>
            ) : (
              <span className="muted">
                The Peter Observer extension is not installed in this browser. Load the <code>extension</code> folder from
                your Peter download at <code>chrome://extensions</code> (Developer mode, Load unpacked).
              </span>
            )
          ) : targetsThis ? (
            <span>
              <strong>Extension ready.</strong>{" "}
              {status?.recording
                ? status.recording.paused
                  ? `Paused: the recorded tab left ${status.recording.origin}.`
                  : `Recording ${status.recording.origin}.`
                : "Open the system in a tab, then press Record this tab in the extension's side panel."}
            </span>
          ) : (
            <>
              <span>
                {connectedHere
                  ? `The extension is recording into “${status?.target?.title ?? "another session"}”.`
                  : "The extension is installed but not connected to you here."}
              </span>
              <button className="btn primary" disabled={busy} onClick={connect}>
                {connectedHere ? "Record into this session" : "Connect extension"}
              </button>
            </>
          )}
        </div>
      )}
      {error && <Notice kind="error">{error}</Notice>}

      {steps.length === 0 ? (
        <p className="muted">No steps yet.</p>
      ) : (
        <ol className="obs-list">
          {steps.map((o) => (
            <li key={o.id} className={`obs obs-${o.kind}`}>
              <span className="obs-seq">{o.seq}</span>
              <div className="obs-body">
                <div className="obs-line">
                  <span className="obs-kind">{KIND_LABEL[o.kind] ?? o.kind}</span>
                  <span className="obs-text">{describe(o)}</span>
                </div>
                {(o.kind === "input" || o.kind === "select") && (
                  <div className={`obs-value${o.masked ? " masked" : ""}`}>{o.masked ? "Hidden (sensitive field)" : o.value || "(empty)"}</div>
                )}
                <div className="muted small obs-meta">
                  {o.url && <span title={o.url}>{shortUrl(o.url)}</span>}
                  <span>{new Date(o.captured_at_ms).toLocaleTimeString()}</span>
                  <span>{o.created_by_name}</span>
                </div>
              </div>
              {o.has_screenshot && <Thumb path={`${base}/${o.id}/screenshot`} onOpen={() => setZoom(o)} />}
              {canRecord && (
                <button className="btn ghost danger small-btn" aria-label={`Remove step ${o.seq}`} onClick={() => remove(o)}>
                  ×
                </button>
              )}
            </li>
          ))}
        </ol>
      )}

      {zoom && <Zoom path={`${base}/${zoom.id}/screenshot`} title={`Step ${zoom.seq}: ${describe(zoom)}`} onClose={() => setZoom(null)} />}
    </section>
  );
}

/** Loads a screenshot only when it scrolls into view. */
function Thumb({ path, onOpen }: { path: string; onOpen: () => void }) {
  const [url, setUrl] = useState<string | null>(null);
  const ref = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    let revoked = false;
    let objectUrl: string | null = null;
    const io = new IntersectionObserver(([entry]) => {
      if (!entry.isIntersecting) return;
      io.disconnect();
      fetchBlobUrl(path)
        .then((u) => {
          objectUrl = u;
          if (revoked) URL.revokeObjectURL(u);
          else setUrl(u);
        })
        .catch(() => {});
    });
    if (ref.current) io.observe(ref.current);
    return () => {
      revoked = true;
      io.disconnect();
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [path]);
  return (
    <button ref={ref} className="thumb" onClick={onOpen} aria-label="Enlarge screenshot">
      {url ? <img src={url} alt="" /> : <span className="muted small">…</span>}
    </button>
  );
}

export function Zoom({ path, title, onClose }: { path: string; title: string; onClose: () => void }) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let u: string | null = null;
    fetchBlobUrl(path).then((x) => setUrl((u = x))).catch(() => {});
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      if (u) URL.revokeObjectURL(u);
    };
  }, [path, onClose]);
  return (
    <div className="zoom" role="dialog" aria-label={title} onClick={onClose}>
      <figure onClick={(e) => e.stopPropagation()}>
        <figcaption>
          <span>{title}</span>
          <button className="btn ghost" onClick={onClose}>Close</button>
        </figcaption>
        {url ? <img src={url} alt={title} /> : <p className="muted">Loading…</p>}
      </figure>
    </div>
  );
}
