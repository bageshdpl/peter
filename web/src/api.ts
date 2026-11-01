// Client for the Peter API. Keeps the session in localStorage and refreshes
// the access token once when it has expired.

export type Role = "owner" | "admin" | "member" | "viewer";
export const ROLES: Role[] = ["owner", "admin", "member", "viewer"];
const RANK: Record<Role, number> = { viewer: 0, member: 1, admin: 2, owner: 3 };
export const atLeast = (role: Role, min: Role) => RANK[role] >= RANK[min];

export interface User { id: string; email: string; name: string }
export interface Workspace { id: string; name: string; role: Role }
export interface Session {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  user: User;
  workspace: Workspace;
  workspaces: Workspace[];
}
export interface Member { user_id: string; email: string; name: string; role: Role; joined_at: number }
export interface Invite { id: string; email: string; role: Role; created_at: number; expires_at: number }
export interface AuditEvent {
  seq: number;
  id: string;
  actor_user_id: string | null;
  actor_email: string | null;
  action: string;
  target: string | null;
  detail: Record<string, unknown> | null;
  at: number;
}

export interface Project {
  id: string;
  name: string;
  system_name: string;
  vendor: string;
  description: string;
  created_by_name: string;
  created_at: number;
  updated_at: number;
  archived_at: number | null;
  session_count: number;
  source_count: number;
}
export interface ValidationSession {
  id: string;
  project_id: string | null;
  project_name: string | null;
  title: string;
  validation_type: string;
  current_step: string | null;
  created_by_name: string;
  created_at: number;
  updated_at: number;
  archived_at: number | null;
}
export interface Step { id: string; label: string; summary: string }
export interface ValidationType { id: string; title: string; summary: string; available: boolean; steps: Step[] }
export interface Source {
  id: string;
  filename: string;
  content_type: string;
  size_bytes: number;
  sha256: string;
  uploaded_by_name?: string;
  created_at: number;
}

export type ObservationKind = "navigate" | "click" | "input" | "select" | "submit" | "note" | "capture";
export interface Observation {
  id: string;
  seq: number;
  kind: ObservationKind;
  url: string;
  page_title: string;
  target_label: string;
  target_selector: string;
  value: string;
  masked: boolean;
  note: string;
  has_screenshot: boolean;
  screenshot_type: string | null;
  captured_at_ms: number;
  created_by_name: string;
  created_at: number;
}

export type StepStatus = "not_started" | "in_progress" | "reviewed" | "skipped";
export interface StepState { step_id: string; status: StepStatus; updated_by_name: string; updated_at: number }
export interface DocumentSummary { step_id: string; title: string; status: "drafting" | "ready" | "failed"; current_version: number; updated_at: number }
export interface Provider { id: string; name: string; model: string; is_ai: boolean }
export interface Citations {
  cited?: string[];
  unknown?: string[];
  gaps?: number;
  sources?: Record<string, { id: string; filename: string }>;
}
export interface DocumentVersion {
  version: number;
  body: string;
  origin: "ai" | "person";
  provider: string | null;
  model: string | null;
  prompt_tokens: number | null;
  completion_tokens: number | null;
  citations: Citations;
  observations: Record<string, { id: string; seq: number; kind: string; label: string; has_screenshot: boolean }>;
  created_by_name: string;
  created_at: number;
}
export interface DocumentView {
  document: { id: string; step_id: string; title: string; status: DocumentSummary["status"]; error: string | null; current_version: number } | null;
  current?: DocumentVersion | null;
  versions?: { version: number; origin: string; provider: string | null; model: string | null; created_by_name: string; created_at: number }[];
  title: string | null;
  draftable: boolean;
}

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string) {
    super(message);
  }
}

const STORAGE_KEY = "peter.session";

function load(): Session | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as Session) : null;
  } catch {
    return null;
  }
}

let session: Session | null = load();
const listeners = new Set<(s: Session | null) => void>();

export function getSession() {
  return session;
}

export function setSession(next: Session | null) {
  session = next;
  try {
    if (next) localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
    else localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Storage can be unavailable (private windows); the session then lasts for this tab only.
  }
  listeners.forEach((fn) => fn(next));
}

export function onSessionChange(fn: (s: Session | null) => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

async function send(method: string, path: string, body?: unknown, token?: string): Promise<Response> {
  const headers: Record<string, string> = {};
  if (token) headers["Authorization"] = `Bearer ${token}`;
  let payload: BodyInit | undefined;
  if (body instanceof FormData) {
    payload = body; // The browser sets the multipart boundary itself.
  } else if (body !== undefined) {
    headers["Content-Type"] = "application/json";
    payload = JSON.stringify(body);
  }
  return fetch(path, { method, headers, body: payload });
}

async function parse<T>(res: Response): Promise<T> {
  if (res.status === 204) return undefined as T;
  const data = await res.json().catch(() => null);
  if (!res.ok) {
    const err = data?.error;
    throw new ApiError(res.status, err?.code ?? "unknown", err?.message ?? `Request failed (${res.status}).`);
  }
  return data as T;
}

// Several requests can expire at once; they share one refresh.
let refreshing: Promise<boolean> | null = null;

async function refresh(): Promise<boolean> {
  const current = session;
  if (!current) return false;
  refreshing ??= (async () => {
    try {
      const res = await send("POST", "/v1/auth/refresh", { refresh_token: current.refresh_token });
      if (!res.ok) {
        setSession(null);
        return false;
      }
      setSession(await res.json());
      return true;
    } finally {
      refreshing = null;
    }
  })();
  return refreshing;
}

/** Sends an authenticated request, refreshing the access token once if needed. */
async function authed(method: string, path: string, body?: unknown): Promise<Response> {
  let res = await send(method, path, body, session?.access_token);
  if (res.status === 401 && session && (await refresh())) {
    res = await send(method, path, body, session?.access_token);
  }
  if (res.status === 401 && session) setSession(null);
  return res;
}

export async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  return parse<T>(await authed(method, path, body));
}

export async function uploadFile<T>(path: string, file: File): Promise<T> {
  const form = new FormData();
  form.append("file", file);
  return parse<T>(await authed("POST", path, form));
}

/** Fetches a protected image and returns an object URL for it. The caller revokes it. */
export async function fetchBlobUrl(path: string): Promise<string> {
  const res = await authed("GET", path);
  if (!res.ok) await parse(res);
  return URL.createObjectURL(await res.blob());
}

/**
 * Follows a Server-Sent Events stream. EventSource cannot send the access
 * token, so this reads the stream with fetch. Reconnects with a growing pause
 * until `signal` is aborted; `onOpen` runs on every (re)connection.
 */
export async function followEvents(
  path: string,
  handlers: { onOpen: () => void; onEvent: (name: string, data: unknown) => void },
  signal: AbortSignal,
) {
  let pause = 1000;
  while (!signal.aborted) {
    try {
      let res = await fetch(path, { headers: { Authorization: `Bearer ${session?.access_token}` }, signal });
      if (res.status === 401 && (await refresh())) {
        res = await fetch(path, { headers: { Authorization: `Bearer ${session?.access_token}` }, signal });
      }
      if (!res.ok || !res.body) throw new Error(`stream ${res.status}`);
      pause = 1000;
      handlers.onOpen();
      const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
      let buffer = "";
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += value;
        let end: number;
        while ((end = buffer.indexOf("\n\n")) >= 0) {
          const block = buffer.slice(0, end);
          buffer = buffer.slice(end + 2);
          let name = "message";
          const data: string[] = [];
          for (const line of block.split("\n")) {
            if (line.startsWith("event:")) name = line.slice(6).trim();
            else if (line.startsWith("data:")) data.push(line.slice(5).trimStart());
          }
          if (data.length) {
            try {
              handlers.onEvent(name, JSON.parse(data.join("\n")));
            } catch {
              // Ignore a malformed event rather than dropping the stream.
            }
          }
        }
      }
    } catch {
      if (signal.aborted) return;
    }
    await new Promise((r) => setTimeout(r, pause));
    pause = Math.min(pause * 2, 15000);
  }
}

/** Downloads with the access token (a plain link cannot send it) and saves the file. */
export async function downloadFile(path: string, filename: string) {
  const res = await authed("GET", path);
  if (!res.ok) await parse(res);
  const url = URL.createObjectURL(await res.blob());
  const a = Object.assign(document.createElement("a"), { href: url, download: filename });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** Calls that start a session (sign-in, sign-up, accept invite, switch). */
export async function startSession(path: string, body: unknown): Promise<Session> {
  const s = await api<Session>("POST", path, body);
  setSession(s);
  return s;
}

/** Re-reads your name, role and workspaces after they may have changed. */
export async function reloadSession() {
  const me = await api<{ user: User; workspace: Workspace | null; workspaces: Workspace[] }>("GET", "/v1/auth/me");
  const current = session;
  if (current && me.workspace) {
    setSession({ ...current, user: me.user, workspace: me.workspace, workspaces: me.workspaces });
  }
}

export async function logout() {
  try {
    await api("POST", "/v1/auth/logout");
  } finally {
    setSession(null);
  }
}

/** Reads `#token=...` from the address (links from invite and reset emails). */
export function readHashToken(): string | null {
  return /token=([A-Za-z0-9]+)/.exec(window.location.hash)?.[1] ?? null;
}

/** Removes the token from the address bar and history once it has been read. */
export function clearHash() {
  if (window.location.hash) history.replaceState(null, "", window.location.pathname + window.location.search);
}

export function formatSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export const formatTime = (secs: number) =>
  new Date(secs * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
