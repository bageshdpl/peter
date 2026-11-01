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
