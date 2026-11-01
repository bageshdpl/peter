import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { api, atLeast, formatTime, type Project, type ValidationSession } from "../api";
import { useSession } from "../App";
import { NewSessionForm, SessionTable } from "../sessions";
import { SharedWithMe } from "../sharing";
import { Field, messageOf, Notice, useAction } from "../ui";

export default function Home() {
  const session = useSession()!;
  const canWrite = atLeast(session.workspace.role, "member");
  const [showArchived, setShowArchived] = useState(false);
  const [projects, setProjects] = useState<Project[] | null>(null);
  const [unfiled, setUnfiled] = useState<ValidationSession[]>([]);
  const [creating, setCreating] = useState<"project" | "session" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [p, s] = await Promise.all([
        api<{ projects: Project[] }>("GET", `/v1/projects?archived=${showArchived}`),
        api<{ sessions: ValidationSession[] }>("GET", "/v1/sessions?unfiled=true"),
      ]);
      setProjects(p.projects);
      setUnfiled(s.sessions);
      setError(null);
    } catch (err) {
      setError(messageOf(err));
    }
  }, [showArchived]);

  useEffect(() => {
    load();
  }, [load, session.workspace.id]);

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <p className="eyebrow">{session.workspace.name}</p>
          <h1>Projects</h1>
          <p className="muted">A project is one system you validate. Its sessions are the validation work.</p>
        </div>
        {canWrite && (
          <div className="head-actions">
            <button className="btn ghost" onClick={() => setCreating("session")}>Quick session</button>
            <button className="btn primary" onClick={() => setCreating("project")}>New project</button>
          </div>
        )}
      </header>

      {creating === "project" && <NewProjectForm onCancel={() => setCreating(null)} />}
      {creating === "session" && (
        <section className="card">
          <h2>Quick session</h2>
          <p className="muted">Not filed in a project. You can move it into one later.</p>
          <NewSessionForm projectId={null} onCancel={() => setCreating(null)} />
        </section>
      )}

      {error && <Notice kind="error">{error}</Notice>}
      <SharedWithMe key={session.workspace.id} />

      <div className="tabs" role="tablist">
        <button role="tab" aria-selected={!showArchived} onClick={() => setShowArchived(false)}>Active</button>
        <button role="tab" aria-selected={showArchived} onClick={() => setShowArchived(true)}>Archived</button>
      </div>

      {!projects ? (
        <p className="muted">Loading…</p>
      ) : projects.length === 0 ? (
        <section className="card empty">
          <h2>{showArchived ? "No archived projects" : "No projects yet"}</h2>
          {!showArchived && (
            <p className="muted">
              {canWrite ? "Create a project for the system you are about to validate." : "Nobody has created a project yet."}
            </p>
          )}
        </section>
      ) : (
        <section className="project-grid">
          {projects.map((p) => (
            <Link key={p.id} to={`/projects/${p.id}`} className="card link-card project-card">
              <h2>{p.name}</h2>
              {(p.system_name || p.vendor) && (
                <p className="muted">{[p.system_name, p.vendor].filter(Boolean).join(" · ")}</p>
              )}
              <div className="stats">
                <span>{p.session_count} {p.session_count === 1 ? "session" : "sessions"}</span>
                <span>{p.source_count} {p.source_count === 1 ? "file" : "files"}</span>
              </div>
              <p className="muted small">Updated {formatTime(p.updated_at)}</p>
            </Link>
          ))}
        </section>
      )}

      {!showArchived && unfiled.length > 0 && (
        <section className="card">
          <h2>Unfiled sessions</h2>
          <SessionTable sessions={unfiled} />
        </section>
      )}
    </div>
  );
}

function NewProjectForm({ onCancel }: { onCancel: () => void }) {
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [system, setSystem] = useState("");
  const [vendor, setVendor] = useState("");
  const [description, setDescription] = useState("");
  const { busy, error, run } = useAction();

  return (
    <section className="card">
      <h2>New project</h2>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          run(async () => {
            const p = await api<Project>("POST", "/v1/projects", {
              name, system_name: system, vendor, description,
            });
            navigate(`/projects/${p.id}`);
          });
        }}
      >
        {error && <Notice kind="error">{error}</Notice>}
        <Field label="Project name" value={name} onChange={setName} autoFocus hint="For example: LIMS go-live 2027." />
        <div className="grid-2">
          <Field label="System and version" value={system} onChange={setSystem} required={false} hint="For example: LabWare LIMS 8.2" />
          <Field label="Vendor" value={vendor} onChange={setVendor} required={false} />
        </div>
        <label className="field">
          <span className="field-label">What it is used for</span>
          <textarea rows={3} maxLength={4000} value={description} onChange={(e) => setDescription(e.target.value)} />
        </label>
        <div className="form-actions">
          <button type="button" className="btn ghost" onClick={onCancel}>Cancel</button>
          <button className="btn primary" disabled={busy}>Create project</button>
        </div>
      </form>
    </section>
  );
}
