// Single sign-on: leaving for the identity provider, coming back from it, and
// the workspace's settings.

import { useEffect, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { api, setSession, type Session } from "../api";
import { useSession } from "../App";
import { AuthCard, messageOf, Notice, useAction } from "../ui";

/** A signature waiting for the person to confirm at the identity provider. */
const PENDING_SIGNATURE = "peter.sso.signature";
export interface PendingSignature { base: string; version: number; meaning: string; returnTo: string }

/** Sends the browser to the identity provider. Does not return on success. */
export async function goToProvider(path: string, body?: unknown) {
  const { url } = await api<{ url: string }>("POST", path, body);
  window.location.assign(url);
  await new Promise(() => {}); // The page is leaving.
}

/** Confirms a signature by signing in again at the identity provider. */
export async function confirmSignatureWithSso(pending: PendingSignature) {
  sessionStorage.setItem(PENDING_SIGNATURE, JSON.stringify(pending));
  await goToProvider("/v1/auth/sso/sign-start");
}

/** `/sso/:workspaceId`: the sign-in link an administrator gives to colleagues. */
export function SsoStart() {
  const { workspaceId } = useParams<{ workspaceId: string }>();
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    goToProvider("/v1/auth/sso/start", { workspace_id: workspaceId }).catch((e) => setError(messageOf(e)));
  }, [workspaceId]);
  return (
    <AuthCard title="Single sign-on">
      {error ? <Notice kind="error">{error}</Notice> : <p className="muted">Taking you to your company's sign-in…</p>}
      <p className="auth-links"><Link to="/signin">Sign in another way</Link></p>
    </AuthCard>
  );
}

// An answer from the provider works once, so each is handled once even if the page renders twice.
const handled = new Map<string, Promise<string>>();

type CallbackAnswer = (Session & { purpose: "login" }) | { purpose: "sign"; sign_token: string };

async function finish(code: string, state: string): Promise<string> {
  const answer = await api<CallbackAnswer>("POST", "/v1/auth/sso/callback", { code, state });
  if (answer.purpose === "login") {
    setSession(answer);
    return "/";
  }
  const raw = sessionStorage.getItem(PENDING_SIGNATURE);
  sessionStorage.removeItem(PENDING_SIGNATURE);
  if (!raw) throw new Error("Peter lost track of what you were signing. Open the document and sign again.");
  const pending = JSON.parse(raw) as PendingSignature;
  await api("POST", `${pending.base}/signatures`, { version: pending.version, meaning: pending.meaning, sso_token: answer.sign_token });
  return pending.returnTo;
}

/** `/sso/callback`: where the identity provider sends the person back. */
export function SsoCallback() {
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const signedIn = useSession() !== null;
  const [error, setError] = useState<string | null>(null);
  const [code, state] = [params.get("code"), params.get("state")];
  const providerError = params.get("error_description") ?? params.get("error");

  useEffect(() => {
    if (providerError || !code || !state) return;
    let job = handled.get(state);
    if (!job) {
      job = finish(code, state);
      handled.set(state, job);
    }
    job.then((to) => navigate(to, { replace: true })).catch((e) => setError(messageOf(e)));
  }, [code, state, providerError, navigate]);

  const problem = providerError
    ? `Your identity provider did not sign you in: ${providerError}`
    : !code || !state
      ? "This page finishes a single sign-on. Start from the sign-in page."
      : error;
  return (
    <AuthCard title="Single sign-on">
      {problem ? <Notice kind="error">{problem}</Notice> : <p className="muted">Finishing…</p>}
      {problem && <p className="auth-links">{signedIn ? <Link to="/">Back to Peter</Link> : <Link to="/signin">Back to sign in</Link>}</p>}
    </AuthCard>
  );
}

// ---------- Workspace settings (admins) ----------

interface SsoSettingsView {
  configured: boolean;
  issuer: string;
  client_id: string;
  enabled: boolean;
  enforce: boolean;
  auto_join: boolean;
  default_role: "member" | "viewer";
  domains: string[];
  redirect_uri: string;
  sign_in_link: string;
}

function Copy({ label, value }: { label: string; value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="field">
      <span className="field-label">{label}</span>
      <div className="copy-row">
        <code>{value}</code>
        <button type="button" className="btn ghost" onClick={() => navigator.clipboard.writeText(value).then(() => setCopied(true)).catch(() => {})}>
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
    </div>
  );
}

export function SsoSettings() {
  const session = useSession()!;
  const [saved, setSaved] = useState<SsoSettingsView | null>(null);
  const [form, setForm] = useState({ issuer: "", client_id: "", client_secret: "", domains: "" });
  const [flags, setFlags] = useState({ enabled: false, enforce: false, auto_join: true, default_role: "member" as "member" | "viewer" });
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);
  const { busy, error, run } = useAction();

  const show = (v: SsoSettingsView) => {
    setSaved(v);
    setForm({ issuer: v.issuer, client_id: v.client_id, client_secret: "", domains: v.domains.join(", ") });
    setFlags({ enabled: v.enabled, enforce: v.enforce, auto_join: v.auto_join, default_role: v.default_role });
  };
  useEffect(() => {
    api<SsoSettingsView>("GET", "/v1/workspace/sso").then(show).catch(() => {});
  }, [session.workspace.id]);

  if (!saved) return null;
  const save = () =>
    run(async () => {
      setNote(null);
      show(await api<SsoSettingsView>("PUT", "/v1/workspace/sso", {
        issuer: form.issuer,
        client_id: form.client_id,
        client_secret: form.client_secret || undefined,
        domains: form.domains,
        ...flags,
      }));
      setNote({ ok: true, text: flags.enabled ? "Saved. Single sign-on is on." : "Saved. Single sign-on is off until you turn it on." });
    });

  return (
    <section className="card">
      <h2>Single sign-on</h2>
      <p className="muted">
        Let people sign in to {session.workspace.name} with your company's identity provider (Microsoft Entra, Okta, Google and
        others that support OpenID Connect). It vouches for people in this workspace only.
      </p>
      <Copy label="Redirect address to register at the provider" value={saved.redirect_uri} />
      <form className="stack" onSubmit={(e) => { e.preventDefault(); save(); }}>
        <label className="field">
          <span className="field-label">Issuer address</span>
          <input value={form.issuer} required placeholder="https://login.microsoftonline.com/your-tenant-id/v2.0"
            onChange={(e) => setForm({ ...form, issuer: e.target.value })} />
        </label>
        <label className="field">
          <span className="field-label">Client ID</span>
          <input value={form.client_id} required onChange={(e) => setForm({ ...form, client_id: e.target.value })} />
        </label>
        <label className="field">
          <span className="field-label">Client secret</span>
          <input type="password" autoComplete="off" value={form.client_secret} required={!saved.configured}
            placeholder={saved.configured ? "Saved. Leave empty to keep it." : ""}
            onChange={(e) => setForm({ ...form, client_secret: e.target.value })} />
          <span className="muted small">Stored encrypted and never shown again.</span>
        </label>
        <label className="field">
          <span className="field-label">Email domains allowed (optional)</span>
          <input value={form.domains} placeholder="example.com, example.co.uk" onChange={(e) => setForm({ ...form, domains: e.target.value })} />
          <span className="muted small">Leave empty to accept anyone your provider signs in.</span>
        </label>

        <label className="check-row">
          <input type="checkbox" checked={flags.enabled}
            onChange={(e) => setFlags({ ...flags, enabled: e.target.checked, enforce: e.target.checked && flags.enforce })} />
          <span><strong>Turn single sign-on on</strong>
            <span className="muted small block">Peter checks it can reach the provider before saving.</span></span>
        </label>
        <label className="check-row">
          <input type="checkbox" checked={flags.auto_join} onChange={(e) => setFlags({ ...flags, auto_join: e.target.checked })} />
          <span><strong>Let new people join on their first sign-in</strong>
            <span className="muted small block">
              They join as{" "}
              <select value={flags.default_role} onChange={(e) => setFlags({ ...flags, default_role: e.target.value as "member" | "viewer" })}>
                <option value="member">member</option>
                <option value="viewer">viewer</option>
              </select>
              . Off means only people you have invited can use it.
            </span></span>
        </label>
        <label className={`check-row${flags.enabled ? "" : " disabled"}`}>
          <input type="checkbox" checked={flags.enforce} disabled={!flags.enabled} onChange={(e) => setFlags({ ...flags, enforce: e.target.checked })} />
          <span><strong>Require single sign-on</strong>
            <span className="muted small block">
              Passwords stop working for this workspace, at once, for everyone except owners. Owners keep password access so
              a problem at the provider cannot lock the workspace.
            </span></span>
        </label>

        {error && <Notice kind="error">{error}</Notice>}
        {note && <Notice kind={note.ok ? "ok" : "error"}>{note.text}</Notice>}
        <div className="form-actions">
          {saved.configured && (
            <>
              <button type="button" className="btn ghost danger" disabled={busy}
                onClick={() => window.confirm("Remove single sign-on from this workspace? People will need a password to sign in.") &&
                  run(async () => {
                    await api("DELETE", "/v1/workspace/sso");
                    show(await api<SsoSettingsView>("GET", "/v1/workspace/sso"));
                    setNote({ ok: true, text: "Single sign-on was removed." });
                  })}>
                Remove
              </button>
              <button type="button" className="btn ghost" disabled={busy}
                onClick={() => run(async () => {
                  const r = await api<{ ok: boolean; message: string }>("POST", "/v1/workspace/sso/check");
                  setNote({ ok: r.ok, text: r.message });
                })}>
                Check connection
              </button>
            </>
          )}
          <button className="btn primary" disabled={busy}>Save</button>
        </div>
      </form>
      {saved.enabled && (
        <>
          <Copy label="Sign-in link for your colleagues" value={saved.sign_in_link} />
          <p className="muted small">
            Try it in a private window before requiring it. People already in the workspace can also choose “Sign in with
            SSO” on the sign-in page and enter their email.
          </p>
        </>
      )}
    </section>
  );
}
