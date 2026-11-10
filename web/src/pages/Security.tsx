import { useCallback, useEffect, useState } from "react";
import QRCode from "qrcode";
import { api, atLeast, formatTime, reloadSession } from "../api";
import { useSession } from "../App";
import { messageOf, Notice, useAction } from "../ui";
import { SsoSettings } from "./Sso";

interface MfaStatus { enabled: boolean; enabled_at: number | null; recovery_codes_left: number; workspace_requires: boolean }
interface ActiveSession { id: string; client: string; user_agent: string; workspace_name: string; created_at: number; last_used_at: number | null; current: boolean }

export default function Security() {
  const session = useSession()!;
  const [mfa, setMfa] = useState<MfaStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setMfa(await api<MfaStatus>("GET", "/v1/auth/mfa"));
    } catch (err) {
      setError(messageOf(err));
    }
  }, []);
  useEffect(() => {
    load();
  }, [load, session.workspace.id]);

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <p className="eyebrow">{session.user.email}</p>
          <h1>Security</h1>
        </div>
      </header>
      {session.mfa_setup_required && (
        <Notice kind="error">
          {session.workspace.name} requires two-factor sign-in. Set it up below to use the workspace.
        </Notice>
      )}
      {error && <Notice kind="error">{error}</Notice>}
      {mfa && <TwoFactor status={mfa} onChange={async () => { await load(); await reloadSession().catch(() => {}); }} />}
      <Sessions />
      {atLeast(session.workspace.role, "admin") && mfa && <Policy status={mfa} onChange={load} />}
      {atLeast(session.workspace.role, "admin") && <SsoSettings />}
    </div>
  );
}

// ---------- Two-factor sign-in ----------

function TwoFactor({ status, onChange }: { status: MfaStatus; onChange: () => Promise<void> }) {
  const [setup, setSetup] = useState<{ secret: string; otpauth_url: string; qr: string } | null>(null);
  const [codes, setCodes] = useState<string[] | null>(null);
  const [code, setCode] = useState("");
  const [password, setPassword] = useState("");
  const [mode, setMode] = useState<"idle" | "disable" | "renew">("idle");
  const { busy, error, setError, run } = useAction();

  const begin = () =>
    run(async () => {
      const s = await api<{ secret: string; otpauth_url: string }>("POST", "/v1/auth/mfa/setup");
      const qr = await QRCode.toDataURL(s.otpauth_url, { margin: 1, width: 200 });
      setSetup({ ...s, qr });
      setCode("");
    });

  return (
    <section className="card">
      <div className="card-head">
        <div>
          <h2>Two-factor sign-in</h2>
          <p className="muted">
            After your password, Peter asks for a code from an authenticator app on your phone (for example Google
            Authenticator or Microsoft Authenticator).
          </p>
        </div>
        <span className={`status-pill ${status.enabled ? "passed" : "not_tested"}`}>{status.enabled ? "On" : "Off"}</span>
      </div>
      {error && <Notice kind="error">{error}</Notice>}

      {codes && (
        <div className="recovery">
          <h3>Save your recovery codes</h3>
          <p>
            If you lose your phone, each of these codes signs you in once. Store them somewhere safe, such as a password
            manager. <strong>They are shown only now.</strong>
          </p>
          <ol className="codes">{codes.map((c) => <li key={c}><code>{c}</code></li>)}</ol>
          <div className="row-actions">
            <button className="btn ghost" onClick={() => navigator.clipboard?.writeText(codes.join("\n"))}>Copy</button>
            <button className="btn ghost" onClick={() => download("peter-recovery-codes.txt", `Peter recovery codes\n\n${codes.join("\n")}\n`)}>Download</button>
            <button className="btn primary" onClick={() => setCodes(null)}>I have saved them</button>
          </div>
        </div>
      )}

      {!status.enabled && !setup && !codes && (
        <button className="btn primary" disabled={busy} onClick={begin}>Set up two-factor sign-in</button>
      )}

      {!status.enabled && setup && (
        <form
          className="mfa-setup"
          onSubmit={(e) => {
            e.preventDefault();
            run(async () => {
              const res = await api<{ recovery_codes: string[] }>("POST", "/v1/auth/mfa/enable", { code });
              setSetup(null);
              setCodes(res.recovery_codes);
              await onChange();
            });
          }}
        >
          <ol className="setup-steps">
            <li>
              Open your authenticator app and scan this code.
              <img src={setup.qr} alt="QR code for your authenticator app" width={200} height={200} />
              <details>
                <summary>Can't scan? Enter this key instead</summary>
                <code className="secret">{setup.secret.replace(/(.{4})/g, "$1 ").trim()}</code>
              </details>
            </li>
            <li>
              Enter the 6-digit code the app shows.
              <input value={code} onChange={(e) => setCode(e.target.value)} inputMode="numeric" autoComplete="one-time-code"
                placeholder="123456" maxLength={7} required aria-label="Code from the app" />
            </li>
          </ol>
          <div className="form-actions">
            <button type="button" className="btn ghost" onClick={() => setSetup(null)}>Cancel</button>
            <button className="btn primary" disabled={busy}>Turn on</button>
          </div>
        </form>
      )}

      {status.enabled && !codes && (
        <>
          <p>
            On since {status.enabled_at ? formatTime(status.enabled_at) : "—"}. Recovery codes left:{" "}
            <strong>{status.recovery_codes_left}</strong>
            {status.recovery_codes_left <= 3 && " — make new ones soon."}
          </p>
          {mode === "idle" && (
            <div className="row-actions">
              <button className="btn ghost" onClick={() => { setMode("renew"); setError(null); setCode(""); }}>New recovery codes</button>
              <button className="btn ghost danger" onClick={() => { setMode("disable"); setError(null); setCode(""); setPassword(""); }}>Turn off</button>
            </div>
          )}
          {mode !== "idle" && (
            <form
              className="stack"
              onSubmit={(e) => {
                e.preventDefault();
                run(async () => {
                  if (mode === "renew") {
                    const res = await api<{ recovery_codes: string[] }>("POST", "/v1/auth/mfa/recovery-codes", { code });
                    setCodes(res.recovery_codes);
                  } else {
                    await api("POST", "/v1/auth/mfa/disable", { password, code });
                  }
                  setMode("idle");
                  await onChange();
                });
              }}
            >
              {mode === "disable" && status.workspace_requires && (
                <Notice kind="error">This workspace requires two-factor sign-in. Turning it off blocks your access until you set it up again.</Notice>
              )}
              {mode === "disable" && (
                <label className="field">
                  <span className="field-label">Your password</span>
                  <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="current-password" required />
                </label>
              )}
              <label className="field">
                <span className="field-label">Code from your app</span>
                <input value={code} onChange={(e) => setCode(e.target.value)} inputMode="numeric" autoComplete="one-time-code" maxLength={7} required />
              </label>
              <div className="form-actions">
                <button type="button" className="btn ghost" onClick={() => setMode("idle")}>Cancel</button>
                <button className={`btn ${mode === "disable" ? "danger-solid" : "primary"}`} disabled={busy}>
                  {mode === "disable" ? "Turn off two-factor sign-in" : "Make new codes"}
                </button>
              </div>
            </form>
          )}
        </>
      )}
    </section>
  );
}

function download(name: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
  const a = Object.assign(document.createElement("a"), { href: url, download: name });
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

// ---------- Active sessions ----------

/** "Chrome on Windows" from a user-agent string, roughly. */
function describeAgent(ua: string, client: string) {
  if (client === "extension") return "Peter Observer extension";
  if (!ua) return "Unknown browser";
  const browser = /Edg\//.test(ua) ? "Edge" : /Firefox\//.test(ua) ? "Firefox" : /Chrome\//.test(ua) ? "Chrome" : /Safari\//.test(ua) ? "Safari" : "Browser";
  const os = /Windows/.test(ua) ? "Windows" : /Mac OS X|Macintosh/.test(ua) ? "macOS" : /Android/.test(ua) ? "Android" : /iPhone|iPad/.test(ua) ? "iOS" : /Linux/.test(ua) ? "Linux" : "";
  return os ? `${browser} on ${os}` : browser;
}

function Sessions() {
  const [list, setList] = useState<ActiveSession[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(async () => {
    try {
      setList((await api<{ sessions: ActiveSession[] }>("GET", "/v1/auth/sessions")).sessions);
    } catch (err) {
      setError(messageOf(err));
    }
  }, []);
  useEffect(() => {
    load();
  }, [load]);
  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      await load();
    } catch (err) {
      setError(messageOf(err));
    }
  };
  const others = list?.filter((s) => !s.current).length ?? 0;

  return (
    <section className="card">
      <div className="card-head">
        <div>
          <h2>Where you're signed in</h2>
          <p className="muted">Sign out any device or browser you don't recognise.</p>
        </div>
        {others > 0 && (
          <button className="btn ghost danger" onClick={() => act(() => api("POST", "/v1/auth/sessions/revoke-others"))}>
            Sign out all others
          </button>
        )}
      </div>
      {error && <Notice kind="error">{error}</Notice>}
      {list && (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Device</th>
                <th className="hide-sm">Workspace</th>
                <th className="hide-sm">Signed in</th>
                <th>Last active</th>
                <th aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {list.map((s) => (
                <tr key={s.id}>
                  <td>
                    <strong>{describeAgent(s.user_agent, s.client)}</strong>
                    {s.current && <span className="here">This device</span>}
                  </td>
                  <td className="hide-sm">{s.workspace_name}</td>
                  <td className="hide-sm muted nowrap">{formatTime(s.created_at)}</td>
                  <td className="muted nowrap">{formatTime(s.last_used_at ?? s.created_at)}</td>
                  <td className="actions">
                    {!s.current && (
                      <button className="btn ghost danger" onClick={() => act(() => api("DELETE", `/v1/auth/sessions/${s.id}`))}>Sign out</button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

// ---------- Workspace policy ----------

function Policy({ status, onChange }: { status: MfaStatus; onChange: () => Promise<void> }) {
  const session = useSession()!;
  const [note, setNote] = useState<string | null>(null);
  const { busy, error, run } = useAction();
  return (
    <section className="card">
      <h2>Workspace sign-in policy</h2>
      <label className="check-row">
        <input
          type="checkbox"
          checked={status.workspace_requires}
          disabled={busy || (!status.enabled && !status.workspace_requires)}
          onChange={(e) =>
            run(async () => {
              const res = await api<{ require_mfa: boolean; members_without_mfa: number }>("PUT", "/v1/workspace/security", { require_mfa: e.target.checked });
              setNote(
                res.require_mfa
                  ? res.members_without_mfa > 0
                    ? `${res.members_without_mfa} member(s) must set up two-factor sign-in before they can use ${session.workspace.name}.`
                    : "Everyone already uses two-factor sign-in."
                  : "Two-factor sign-in is now optional.",
              );
              await onChange();
            })
          }
        />
        <span>
          <strong>Require two-factor sign-in for everyone in {session.workspace.name}</strong>
          <span className="muted small block">
            {status.enabled ? "Members without it can only set it up until they do." : "Turn it on for yourself first."}
          </span>
        </span>
      </label>
      {error && <Notice kind="error">{error}</Notice>}
      {note && <Notice kind="ok">{note}</Notice>}
    </section>
  );
}
