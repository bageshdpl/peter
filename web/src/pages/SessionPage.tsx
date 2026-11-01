import { useCallback, useEffect, useState } from "react";
import { Link, useParams, useSearchParams } from "react-router-dom";
import {
  api, atLeast, downloadFile, formatTime,
  type DocumentSummary, type Project, type StepState, type StepStatus, type ValidationSession, type ValidationType,
} from "../api";
import { useSession } from "../App";
import { DocumentPanel } from "../document";
import { useSessionEvents } from "../events";
import { ObservationPanel } from "../observe";
import { TracePanel } from "../trace";
import { messageOf, Notice } from "../ui";

interface Loaded {
  session: ValidationSession;
  type: ValidationType | null;
  step_states: StepState[];
  documents: DocumentSummary[];
}

const STATUS_LABEL: Record<StepStatus, string> = {
  not_started: "Not started",
  in_progress: "In progress",
  reviewed: "Reviewed",
  skipped: "Skipped",
};

export default function SessionPage() {
  const { id } = useParams<{ id: string }>();
  const me = useSession()!;
  const [search, setSearch] = useSearchParams();
  const [data, setData] = useState<Loaded | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const events = useSessionEvents(id!);

  const load = useCallback(async () => {
    try {
      const d = await api<Loaded>("GET", `/v1/sessions/${id}`);
      setData(d);
      setTitle((t) => t || d.session.title);
      setError(null);
    } catch (err) {
      setError(messageOf(err));
    }
  }, [id]);

  useEffect(() => {
    setTitle("");
    load();
    api<{ projects: Project[] }>("GET", "/v1/projects").then((p) => setProjects(p.projects)).catch(() => {});
  }, [load]);

  // Step and document changes made elsewhere (another person, a finished draft).
  const { subscribe } = events;
  useEffect(() => subscribe((name) => (name === "step" || name === "document" || name === "open") && load()), [subscribe, load]);

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

  async function act(fn: () => Promise<unknown>) {
    try {
      await fn();
      await load();
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
  const steps = type?.steps ?? [];
  const archived = session.archived_at !== null;
  const canWrite = atLeast(me.workspace.role, "member") && !archived;
  // The step on screen: from the address if set, else the session's current step.
  const viewing = steps.find((s) => s.id === search.get("step")) ?? steps.find((s) => s.id === session.current_step) ?? steps[0];
  const index = steps.findIndex((s) => s.id === viewing?.id);
  const statusOf = (stepId: string): StepStatus => data.step_states.find((s) => s.step_id === stepId)?.status ?? "not_started";
  const docOf = (stepId: string) => data.documents.find((d) => d.step_id === stepId);
  const done = steps.filter((s) => ["reviewed", "skipped"].includes(statusOf(s.id))).length;

  function goTo(stepId: string) {
    setSearch({ step: stepId }, { replace: false });
    window.scrollTo({ top: 0 });
    if (canWrite && stepId !== session.current_step) {
      act(() => api("POST", `/v1/sessions/${id}/current`, { step_id: stepId }));
    }
  }

  const setStatus = (status: StepStatus) =>
    act(() => api("PUT", `/v1/sessions/${id}/steps/${viewing!.id}`, { status }));

  return (
    <div className="page wide">
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
        <div className="head-actions">
          <label className="field compact">
            <span className="field-label">Project</span>
            <select value={session.project_id ?? ""} disabled={!canWrite} onChange={(e) => save({ project_id: e.target.value || null })}>
              <option value="">Unfiled</option>
              {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
              {session.project_id && !projects.some((p) => p.id === session.project_id) && (
                <option value={session.project_id}>{session.project_name}</option>
              )}
            </select>
          </label>
          {data.documents.some((d) => d.current_version > 0) && (
            <button className="btn ghost" onClick={() => downloadFile(`/v1/sessions/${id}/export`, `${session.title} validation package.zip`).catch((e) => setError(messageOf(e)))}>
              Export package
            </button>
          )}
          {atLeast(me.workspace.role, "member") && (
            <button className="btn ghost" onClick={() => act(() => api("POST", `/v1/sessions/${id}/${archived ? "restore" : "archive"}`))}>
              {archived ? "Restore" : "Archive"}
            </button>
          )}
        </div>
      </header>

      {archived && <Notice kind="error">This session is archived and read-only.</Notice>}
      {error && <Notice kind="error">{error}</Notice>}

      <div className="workspace">
        <aside className="step-rail" aria-label="Steps">
          <div className="rail-head">
            <span className="field-label">Steps</span>
            <span className="muted small">{done} of {steps.length} done</span>
          </div>
          <div className="progress" aria-hidden><span style={{ width: `${(done / Math.max(steps.length, 1)) * 100}%` }} /></div>
          <ol>
            {steps.map((s, i) => {
              const status = statusOf(s.id);
              const doc = docOf(s.id);
              return (
                <li key={s.id}>
                  <button className={`rail-step ${status}${s.id === viewing?.id ? " active" : ""}`} onClick={() => goTo(s.id)}
                    aria-current={s.id === viewing?.id ? "step" : undefined}>
                    <span className="rail-mark" aria-label={STATUS_LABEL[status]}>
                      {status === "reviewed" ? "✓" : status === "skipped" ? "–" : i + 1}
                    </span>
                    <span className="rail-text">
                      <span>{s.label}</span>
                      {doc && (
                        <span className={`rail-doc ${doc.status}`}>
                          {doc.status === "drafting" ? "Drafting…" : doc.status === "failed" ? "Draft failed" : `Draft v${doc.current_version}`}
                        </span>
                      )}
                    </span>
                  </button>
                </li>
              );
            })}
          </ol>
        </aside>

        {viewing && (
          <div className="step-main">
            <section className="card step-head">
              <div className="step-title">
                <p className="eyebrow">Step {index + 1} of {steps.length}</p>
                <h2>{viewing.label}</h2>
                <p className="muted">{viewing.summary}</p>
              </div>
              <div className="step-actions">
                <span className={`pill ${statusOf(viewing.id)}`}>{STATUS_LABEL[statusOf(viewing.id)]}</span>
                {canWrite && (
                  statusOf(viewing.id) === "reviewed" || statusOf(viewing.id) === "skipped" ? (
                    <button className="btn ghost" onClick={() => setStatus("in_progress")}>Reopen</button>
                  ) : (
                    <>
                      <button className="btn ghost" onClick={() => setStatus("skipped")}>Skip</button>
                      <button className="btn primary" onClick={() => setStatus("reviewed")}>Mark reviewed</button>
                    </>
                  )
                )}
              </div>
            </section>

            {viewing.id === "traceability" && (
              <TracePanel sessionId={session.id} sessionTitle={session.title} canWrite={canWrite} events={events} onGoToStep={goTo} />
            )}

            {viewing.id === "observe" && (
              <ObservationPanel
                sessionId={session.id}
                sessionTitle={session.title}
                userId={me.user.id}
                workspaceId={me.workspace.id}
                canRecord={canWrite}
                events={events}
              />
            )}

            <DocumentPanel
              sessionId={session.id}
              step={viewing}
              steps={steps}
              projectId={session.project_id}
              canWrite={canWrite}
              canComment={!archived}
              canResolveAny={atLeast(me.workspace.role, "member") && !archived}
              userId={me.user.id}
              events={events}
              onGoToStep={goTo}
            />

            <div className="step-nav">
              <button className="btn ghost" disabled={index <= 0} onClick={() => goTo(steps[index - 1].id)}>
                ← {index > 0 ? steps[index - 1].label : "Back"}
              </button>
              <button className="btn ghost" disabled={index >= steps.length - 1} onClick={() => goTo(steps[index + 1].id)}>
                {index < steps.length - 1 ? steps[index + 1].label : "Next"} →
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
