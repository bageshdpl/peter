import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, atLeast, reloadSession, startSession } from "../api";
import { useSession } from "../App";
import { Field, Notice, PASSWORD_HINT, useAction } from "../ui";

export default function Settings() {
  const session = useSession()!;
  const isAdmin = atLeast(session.workspace.role, "admin");

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <p className="eyebrow">{session.user.email}</p>
          <h1>Settings</h1>
        </div>
      </header>
      <ChangePassword />
      {isAdmin && <RenameWorkspace key={session.workspace.id} current={session.workspace.name} />}
      <NewWorkspace />
    </div>
  );
}

function ChangePassword() {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [done, setDone] = useState(false);
  const { busy, error, run } = useAction();

  return (
    <section className="card">
      <h2>Password</h2>
      <form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          run(async () => {
            await api("POST", "/v1/auth/change-password", { current_password: current, new_password: next });
            setCurrent("");
            setNext("");
            setDone(true);
          });
        }}
      >
        {error && <Notice kind="error">{error}</Notice>}
        {done && <Notice kind="ok">Password changed. Your other devices are signed out.</Notice>}
        <Field label="Current password" type="password" value={current} onChange={setCurrent} autoComplete="current-password" />
        <Field label="New password" type="password" value={next} onChange={setNext}
          autoComplete="new-password" minLength={10} hint={PASSWORD_HINT} />
        <div><button className="btn primary" disabled={busy}>Change password</button></div>
      </form>
    </section>
  );
}

function RenameWorkspace({ current }: { current: string }) {
  const [name, setName] = useState(current);
  const [done, setDone] = useState(false);
  const { busy, error, run } = useAction();

  return (
    <section className="card">
      <h2>This workspace</h2>
      <form
        className="inline-form"
        onSubmit={(e) => {
          e.preventDefault();
          run(async () => {
            await api("PATCH", "/v1/workspace", { name });
            await reloadSession();
            setDone(true);
          });
        }}
      >
        <label className="field grow">
          <span className="field-label">Name</span>
          <input value={name} required maxLength={120} onChange={(e) => { setName(e.target.value); setDone(false); }} />
        </label>
        <button className="btn primary" disabled={busy || name.trim() === current}>Rename</button>
      </form>
      {error && <Notice kind="error">{error}</Notice>}
      {done && <Notice kind="ok">Renamed.</Notice>}
    </section>
  );
}

function NewWorkspace() {
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const { busy, error, run } = useAction();

  return (
    <section className="card">
      <h2>New workspace</h2>
      <p className="muted">A separate space with its own members, for example another site or client. You will be its owner.</p>
      <form
        className="inline-form"
        onSubmit={(e) => {
          e.preventDefault();
          run(async () => {
            const created = await api<{ id: string }>("POST", "/v1/workspaces", { name });
            await startSession("/v1/auth/switch", { workspace_id: created.id });
            navigate("/");
          });
        }}
      >
        <label className="field grow">
          <span className="field-label">Name</span>
          <input value={name} required maxLength={120} onChange={(e) => setName(e.target.value)} />
        </label>
        <button className="btn primary" disabled={busy}>Create and switch</button>
      </form>
      {error && <Notice kind="error">{error}</Notice>}
    </section>
  );
}
