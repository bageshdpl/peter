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
  if (body !== undefined) headers["Content-Type"] = "application/json";
  if (token) headers["Authorization"] = `Bearer ${token}`;
  return fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
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

export async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  let res = await send(method, path, body, session?.access_token);
  if (res.status === 401 && session && (await refresh())) {
    res = await send(method, path, body, session?.access_token);
  }
  if (res.status === 401 && session) setSession(null);
  return parse<T>(res);
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

export const formatTime = (secs: number) =>
  new Date(secs * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
