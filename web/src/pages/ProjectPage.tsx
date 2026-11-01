import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";
import {
  api, atLeast, downloadFile, formatSize, formatTime, uploadFile,
  type Project, type Source, type ValidationSession,
} from "../api";
import { useSession } from "../App";
import { NewSessionForm, SessionTable } from "../sessions";
import { Field, messageOf, Notice, useAction } from "../ui";

export default function ProjectPage() {
  const { id } = useParams<{ id: string }>();
  const session = useSession()!;
  const role = session.workspace.role;
  const [project, setProject] = useState<Project | null>(null);
  const [sessions, setSessions] = useState<ValidationSession[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [newSession, setNewSession] = useState(false);

  const load = useCallback(async () => {
    try {
      const [p, s] = await Promise.all([
        api<Project>("GET", `/v1/projects/${id}`),
        api<{ sessions: ValidationSession[] }>("GET", `/v1/sessions?project_id=${id}`),
      ]);
      setProject(p);
      setSessions(s.sessions);
      setError(null);
    } catch (err) {
      setError(messageOf(err));
    }
  }, [id]);

  useEffect(() => {
    load();
  }, [load]);

  if (error && !project) {
    return (
      <div className="page">
        <Notice kind="error">{error}</Notice>
        <Link to="/">Back to projects</Link>
      </div>
    );
  }
  if (!project) return <div className="page"><p className="muted">Loading…</p></div>;

  const archived = project.archived_at !== null;
  const canWrite = atLeast(role, "member") && !archived;
  const isAdmin = atLeast(role, "admin");

  async function setArchived(archive: boolean) {
    if (archive && !confirm(`Archive ${project!.name}? It becomes read-only until restored.`)) return;
    try {
      setProject(await api<Project>("POST", `/v1/projects/${id}/${archive ? "archive" : "restore"}`));
    } catch (err) {
      setError(messageOf(err));
    }
  }

  return (
    <div className="page">
      <nav className="crumbs"><Link to="/">Projects</Link> / {project.name}</nav>
      <header className="page-head">
        <div>
          <h1>{project.name}</h1>
          <p className="muted">
            {[project.system_name, project.vendor].filter(Boolean).join(" · ") || "No system details yet"}
          </p>
        </div>
        <div className="head-actions">
          {canWrite && !editing && <button className="btn ghost" onClick={() => setEditing(true)}>Edit details</button>}
          {isAdmin && (
            <button className="btn ghost" onClick={() => setArchived(!archived)}>{archived ? "Restore" : "Archive"}</button>
          )}
        </div>
      </header>

      {archived && <Notice kind="error">This project is archived and read-only.</Notice>}
      {error && <Notice kind="error">{error}</Notice>}

      {editing ? (
        <EditProject project={project} onDone={(p) => { if (p) setProject(p); setEditing(false); }} />
      ) : (
        project.description && (
          <section className="card">
            <h2>What it is used for</h2>
            <p className="prewrap">{project.description}</p>
          </section>
        )
      )}

      <section className="card">
        <div className="card-head">
          <h2>Sessions</h2>
          {canWrite && !newSession && (
            <button className="btn primary" onClick={() => setNewSession(true)}>New session</button>
          )}
        </div>
        {newSession && <NewSessionForm projectId={project.id} onCancel={() => setNewSession(false)} />}
        {sessions.length === 0 && !newSession ? (
          <p className="muted">No sessions yet. A session is one piece of validation work on this system.</p>
        ) : (
          sessions.length > 0 && <SessionTable sessions={sessions} />
        )}
      </section>

      <Sources projectId={project.id} canWrite={canWrite} onChange={load} />

      <p className="muted small">Created by {project.created_by_name} on {formatTime(project.created_at)}.</p>
    </div>
  );
}

function EditProject({ project, onDone }: { project: Project; onDone: (p: Project | null) => void }) {
  const [name, setName] = useState(project.name);
  const [system, setSystem] = useState(project.system_name);
  const [vendor, setVendor] = useState(project.vendor);
  const [description, setDescription] = useState(project.description);
  const { busy, error, run } = useAction();

  return (
    <section className="card">
      <h2>Edit details</h2>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          run(async () => {
            onDone(await api<Project>("PATCH", `/v1/projects/${project.id}`, {
              name, system_name: system, vendor, description,
            }));
          });
        }}
      >
        {error && <Notice kind="error">{error}</Notice>}
        <Field label="Project name" value={name} onChange={setName} />
        <div className="grid-2">
          <Field label="System and version" value={system} onChange={setSystem} required={false} />
          <Field label="Vendor" value={vendor} onChange={setVendor} required={false} />
        </div>
        <label className="field">
          <span className="field-label">What it is used for</span>
          <textarea rows={4} maxLength={4000} value={description} onChange={(e) => setDescription(e.target.value)} />
        </label>
        <div className="form-actions">
          <button type="button" className="btn ghost" onClick={() => onDone(null)}>Cancel</button>
          <button className="btn primary" disabled={busy}>Save</button>
        </div>
      </form>
    </section>
  );
}

const ACCEPT = ".pdf,.docx,.xlsx,.csv,.txt,.md,.png,.jpg,.jpeg";

function Sources({ projectId, canWrite, onChange }: { projectId: string; canWrite: boolean; onChange: () => void }) {
  const [sources, setSources] = useState<Source[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [uploading, setUploading] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const base = `/v1/projects/${projectId}/sources`;

  const load = useCallback(async () => {
    try {
      setSources((await api<{ sources: Source[] }>("GET", base)).sources);
    } catch (err) {
      setError(messageOf(err));
    }
  }, [base]);

  useEffect(() => {
    load();
  }, [load]);

  async function upload(files: FileList | null) {
    if (!files?.length) return;
    setError(null);
    for (const file of Array.from(files)) {
      setUploading(file.name);
      try {
        await uploadFile(base, file);
      } catch (err) {
        setError(`${file.name}: ${messageOf(err)}`);
      }
    }
    setUploading(null);
    if (input.current) input.current.value = "";
    await load();
    onChange();
  }

  async function remove(s: Source) {
    if (!confirm(`Remove ${s.filename} from this project?`)) return;
    try {
      await api("DELETE", `${base}/${s.id}`);
      await load();
      onChange();
    } catch (err) {
      setError(messageOf(err));
    }
  }

  return (
    <section className="card">
      <div className="card-head">
        <div>
          <h2>Sources</h2>
          <p className="muted">Vendor manuals, SOPs and specifications. Drafting will cite them.</p>
        </div>
        {canWrite && (
          <label className="btn ghost file-btn">
            {uploading ? `Uploading ${uploading}…` : "Add files"}
            <input ref={input} type="file" multiple accept={ACCEPT} disabled={!!uploading}
              onChange={(e) => upload(e.target.files)} />
          </label>
        )}
      </div>
      {error && <Notice kind="error">{error}</Notice>}
      {sources && sources.length === 0 && <p className="muted">No files yet. PDF, Word, Excel, CSV, text and images, up to 25 MB each.</p>}
      {sources && sources.length > 0 && (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>File</th>
                <th className="hide-sm">Size</th>
                <th className="hide-sm">Added</th>
                <th aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {sources.map((s) => (
                <tr key={s.id}>
                  <td>
                    <button className="link-btn" onClick={() => downloadFile(`${base}/${s.id}/file`, s.filename).catch((e) => setError(messageOf(e)))}>
                      {s.filename}
                    </button>
                    <div className="muted small" title={`SHA-256 ${s.sha256}`}>by {s.uploaded_by_name}</div>
                  </td>
                  <td className="muted hide-sm nowrap">{formatSize(s.size_bytes)}</td>
                  <td className="muted hide-sm nowrap">{formatTime(s.created_at)}</td>
                  <td className="actions">
                    {canWrite && <button className="btn ghost danger" onClick={() => remove(s)}>Remove</button>}
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
