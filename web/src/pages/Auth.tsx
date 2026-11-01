import { useEffect, useState } from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";
import { api, clearHash, readHashToken, serverMeta, signIn, startSession, type ServerMeta } from "../api";

function useMeta() {
  const [meta, setMeta] = useState<ServerMeta | null>(null);
  useEffect(() => {
    serverMeta().then(setMeta);
  }, []);
  return meta;
}
import { AuthCard, Field, Notice, PASSWORD_HINT, useAction } from "../ui";

export function SignIn() {
  const meta = useMeta();
  const navigate = useNavigate();
  const location = useLocation();
  const from = (location.state as { from?: string } | null)?.from ?? "/";
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [mfaToken, setMfaToken] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [useRecovery, setUseRecovery] = useState(false);
  const { busy, error, setError, run } = useAction();

  if (mfaToken) {
    return (
      <AuthCard title="Two-factor sign-in">
        <form
          onSubmit={(e) => {
            e.preventDefault();
            run(async () => {
              await startSession("/v1/auth/login/mfa", useRecovery ? { mfa_token: mfaToken, recovery_code: code } : { mfa_token: mfaToken, code });
              navigate(from, { replace: true });
            });
          }}
        >
          {error && <Notice kind="error">{error}</Notice>}
          <p className="muted">
            {useRecovery ? "Enter one of the recovery codes you saved. Each works once." : "Enter the 6-digit code from your authenticator app."}
          </p>
          <label className="field">
            <span className="field-label">{useRecovery ? "Recovery code" : "Code"}</span>
            <input
              value={code}
              onChange={(e) => setCode(e.target.value)}
              autoComplete="one-time-code"
              inputMode={useRecovery ? "text" : "numeric"}
              placeholder={useRecovery ? "XXXX-XXXX-XXXX" : "123456"}
              maxLength={useRecovery ? 20 : 7}
              required
              autoFocus
            />
          </label>
          <button className="btn primary wide" disabled={busy}>{busy ? "Checking…" : "Sign in"}</button>
        </form>
        <p className="auth-links">
          <button type="button" className="link-btn" onClick={() => { setUseRecovery((v) => !v); setCode(""); setError(null); }}>
            {useRecovery ? "Use the authenticator app instead" : "Lost your phone? Use a recovery code"}
          </button>
          <button type="button" className="link-btn" onClick={() => { setMfaToken(null); setCode(""); setError(null); }}>
            Start again
          </button>
        </p>
      </AuthCard>
    );
  }

  return (
    <AuthCard title="Sign in">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          run(async () => {
            const next = await signIn(email, password);
            if (next) {
              setMfaToken(next.mfaToken);
              setPassword("");
            } else {
              navigate(from, { replace: true });
            }
          });
        }}
      >
        {error && <Notice kind="error">{error}</Notice>}
        <Field label="Email" type="email" value={email} onChange={setEmail} autoComplete="email" autoFocus />
        <Field label="Password" type="password" value={password} onChange={setPassword} autoComplete="current-password" />
        <button className="btn primary wide" disabled={busy}>{busy ? "Signing in…" : "Sign in"}</button>
      </form>
      <p className="auth-links">
        <Link to="/forgot">Forgot your password?</Link>
        {meta?.signup === "invite_only" ? (
          <span>New here? This Peter is invite-only: ask an administrator for an invite.</span>
        ) : meta?.signup === "first_admin" ? (
          <span>Setting up? <Link to="/signup">Create the administrator account</Link></span>
        ) : (
          <span>New to Peter? <Link to="/signup">Create an account</Link></span>
        )}
      </p>
    </AuthCard>
  );
}

export function SignUp() {
  const meta = useMeta();
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [workspace, setWorkspace] = useState("");
  const { busy, error, run } = useAction();

  return (
    <AuthCard title={meta?.signup === "first_admin" ? "Create the administrator account" : "Create your account"}>
      {meta?.signup === "invite_only" && (
        <Notice kind="error">This Peter is invite-only. Ask an administrator to invite you; the invite link signs you up.</Notice>
      )}
      {meta?.signup === "first_admin" && (
        <Notice kind="ok">Use the administrator email configured on the server. You can then invite everyone else.</Notice>
      )}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          run(async () => {
            await startSession("/v1/auth/signup", { name, email, password, workspace_name: workspace });
            navigate("/", { replace: true });
          });
        }}
      >
        {error && <Notice kind="error">{error}</Notice>}
        <Field label="Your name" value={name} onChange={setName} autoComplete="name" autoFocus />
        <Field label="Work email" type="email" value={email} onChange={setEmail} autoComplete="email" />
        <Field label="Password" type="password" value={password} onChange={setPassword}
          autoComplete="new-password" minLength={10} hint={PASSWORD_HINT} />
        <Field label="Workspace name" value={workspace} onChange={setWorkspace}
          hint="Usually your company or site, for example “Acme Pharma QA”." />
        <button className="btn primary wide" disabled={busy}>{busy ? "Creating…" : "Create account"}</button>
      </form>
      <p className="auth-links">
        <span>Already have an account? <Link to="/signin">Sign in</Link></span>
      </p>
    </AuthCard>
  );
}

export function ForgotPassword() {
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState(false);
  const { busy, error, run } = useAction();

  return (
    <AuthCard title="Reset your password">
      {sent ? (
        <Notice kind="ok">
          If an account uses {email}, a reset link is on its way. When running locally, the link is printed in the
          API server's console.
        </Notice>
      ) : (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            run(async () => {
              await api("POST", "/v1/auth/reset/request", { email });
              setSent(true);
            });
          }}
        >
          {error && <Notice kind="error">{error}</Notice>}
          <Field label="Email" type="email" value={email} onChange={setEmail} autoComplete="email" autoFocus />
          <button className="btn primary wide" disabled={busy}>Send reset link</button>
        </form>
      )}
      <p className="auth-links">
        <Link to="/signin">Back to sign in</Link>
      </p>
    </AuthCard>
  );
}

/** Reads the one-time token from the link once, then hides it from the address bar. */
function useLinkToken() {
  const [token] = useState(readHashToken);
  useEffect(clearHash, []);
  return token;
}

export function ResetPassword() {
  const token = useLinkToken();
  const [password, setPassword] = useState("");
  const [done, setDone] = useState(false);
  const { busy, error, run } = useAction();

  return (
    <AuthCard title="Choose a new password">
      {!token ? (
        <Notice kind="error">This link is incomplete. Open the link from the email again.</Notice>
      ) : done ? (
        <Notice kind="ok">Your password is changed and you are signed out everywhere.</Notice>
      ) : (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            run(async () => {
              await api("POST", "/v1/auth/reset/confirm", { token, password });
              setDone(true);
            });
          }}
        >
          {error && <Notice kind="error">{error}</Notice>}
          <Field label="New password" type="password" value={password} onChange={setPassword}
            autoComplete="new-password" minLength={10} hint={PASSWORD_HINT} autoFocus />
          <button className="btn primary wide" disabled={busy}>Set password</button>
        </form>
      )}
      <p className="auth-links">
        <Link to="/signin">Sign in</Link>
      </p>
    </AuthCard>
  );
}

export function AcceptInvite() {
  const navigate = useNavigate();
  const token = useLinkToken();
  const [hasAccount, setHasAccount] = useState(false);
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const { busy, error, run } = useAction();

  if (!token) {
    return (
      <AuthCard title="Join a workspace">
        <Notice kind="error">This invite link is incomplete. Open the link you were sent again.</Notice>
      </AuthCard>
    );
  }

  return (
    <AuthCard title="Join a workspace">
      <div className="segmented" role="tablist">
        <button type="button" role="tab" aria-selected={!hasAccount} onClick={() => setHasAccount(false)}>
          I'm new to Peter
        </button>
        <button type="button" role="tab" aria-selected={hasAccount} onClick={() => setHasAccount(true)}>
          I have an account
        </button>
      </div>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          run(async () => {
            await startSession("/v1/auth/accept-invite", { token, password, name: hasAccount ? undefined : name });
            navigate("/", { replace: true });
          });
        }}
      >
        {error && <Notice kind="error">{error}</Notice>}
        {!hasAccount && <Field label="Your name" value={name} onChange={setName} autoComplete="name" autoFocus />}
        <Field
          label={hasAccount ? "Your Peter password" : "Choose a password"}
          type="password"
          value={password}
          onChange={setPassword}
          autoComplete={hasAccount ? "current-password" : "new-password"}
          minLength={hasAccount ? undefined : 10}
          hint={hasAccount ? undefined : PASSWORD_HINT}
        />
        <button className="btn primary wide" disabled={busy}>Join workspace</button>
      </form>
    </AuthCard>
  );
}
