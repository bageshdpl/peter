import { useCallback, useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api, atLeast, formatTime, type Project, type ValidationSession, type ValidationType } from "../api";
import { useSession } from "../App";
import { ObservationPanel } from "../observe";
import { messageOf, Notice } from "../ui";

interface Loaded {
  session: ValidationSession;
  type: ValidationType | null;
}

export default function SessionPage() {
  const { id } = useParams<{ id: string }>();
  const me = useSession()!;
  const [data, setData] = useState<Loaded | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [title, setTitle] = useState("");

  const load = useCallback(async () => {
    try {
      const [d, p] = await Promise.all([
        api<Loaded>("GET", `/v1/sessions/${id}`),
        api<{ projects: Project[] }>("GET", "/v1/projects"),
      ]);
      setData(d);
      setTitle(d.session.title);
      setProjects(p.projects);
      setError(null);
    } catch (err) {
      setError(messageOf(err));
    }
  }, [id]);

  useEffect(() => {
    load();
  }, [load]);

  async function save(body: Record<string, unknown>) {
    try {
      const d = await api<Loaded>("PATCH", `/v1/sessions/${id}`, body);
      setData(d);
      setTitle(d.session.title);
      setError(null);
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function setArchived(archive: boolean) {
    try {
      setData(await api<Loaded>("POST", `/v1/sessions/${id}/${archive ? "archive" : "restore"}`));
    } catch (err) {
      setError(messageOf(err));
    }
  }

  if (!data) {
    return (
      <div className="page">
        {error ? <Notice kind="error">{error}</Notice> : <p className="muted">Loading…</p>}
        <Link to="/">Back to projects</Link>
      </div>
    );
  }

  const { session, type } = data;
  const archived = session.archived_at !== null;
  const canWrite = atLeast(me.workspace.role, "member") && !archived;
  const currentIndex = type?.steps.findIndex((s) => s.id === session.current_step) ?? -1;

  return (
    <div className="page">
      <nav className="crumbs">
        <Link to="/">Projects</Link>
        {session.project_id && <> / <Link to={`/projects/${session.project_id}`}>{session.project_name}</Link></>}
        {" "}/ {session.title}
      </nav>

      <header className="page-head">
        <div className="grow">
          <p className="eyebrow">{type?.title ?? session.validation_type}</p>
          {canWrite ? (
            <input
              className="title-input"
              aria-label="Session title"
              value={title}
              maxLength={160}
              onChange={(e) => setTitle(e.target.value)}
              onBlur={() => title.trim() && title !== session.title && save({ title })}
              onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
            />
          ) : (
            <h1>{session.title}</h1>
          )}
          <p className="muted small">Started by {session.created_by_name} on {formatTime(session.created_at)}</p>
        </div>
        {atLeast(me.workspace.role, "member") && (
          <button className="btn ghost" onClick={() => setArchived(!archived)}>{archived ? "Restore" : "Archive"}</button>
        )}
      </header>

      {archived && <Notice kind="error">This session is archived and read-only.</Notice>}
      {error && <Notice kind="error">{error}</Notice>}

      <section className="card">
        <label className="field compact inline-field">
          <span className="field-label">Project</span>
          <select
            value={session.project_id ?? ""}
            disabled={!canWrite}
            onChange={(e) => save({ project_id: e.target.value || null })}
          >
            <option value="">Unfiled</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
            {/* Keep showing the current project even if it is archived. */}
            {session.project_id && !projects.some((p) => p.id === session.project_id) && (
              <option value={session.project_id}>{session.project_name}</option>
            )}
          </select>
        </label>
      </section>

      <ObservationPanel
        sessionId={session.id}
        sessionTitle={session.title}
        userId={me.user.id}
        workspaceId={me.workspace.id}
        canRecord={canWrite}
      />

      <section className="card">
        <h2>Steps</h2>
        <p className="muted">
          Guided steps and AI drafting arrive in the next milestones. For now this shows the path the session will
          follow.
        </p>
        <ol className="steps">
          {type?.steps.map((s, i) => (
            <li key={s.id} className={i === currentIndex ? "current" : i < currentIndex ? "done" : ""}>
              <span className="step-num" aria-hidden>{i + 1}</span>
              <div>
                <strong>{s.label}</strong>
                {i === currentIndex && <span className="here">Current</span>}
                <p className="muted">{s.summary}</p>
              </div>
            </li>
          ))}
        </ol>
      </section>
    </div>
  );
}
