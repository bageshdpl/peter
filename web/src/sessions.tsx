import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { api, formatTime, type ValidationSession, type ValidationType } from "./api";
import { Notice, useAction } from "./ui";

let typesCache: Promise<ValidationType[]> | null = null;

/** The validation types never change while the app runs, so load them once. */
export function useValidationTypes() {
  const [types, setTypes] = useState<ValidationType[] | null>(null);
  useEffect(() => {
    typesCache ??= api<{ types: ValidationType[] }>("GET", "/v1/validation-types").then((r) => r.types);
    typesCache.then(setTypes).catch(() => {
      typesCache = null;
    });
  }, []);
  return types;
}

export function NewSessionForm({ projectId, onCancel }: { projectId: string | null; onCancel: () => void }) {
  const navigate = useNavigate();
  const types = useValidationTypes();
  const [title, setTitle] = useState("");
  const [type, setType] = useState("initial");
  const [baseline, setBaseline] = useState("");
  const [earlier, setEarlier] = useState<ValidationSession[] | null>(null);
  const { busy, error, run } = useAction();
  const chosen = types?.find((t) => t.id === type);

  // Earlier sessions of the same project (or the whole workspace when unfiled) a session can build on.
  useEffect(() => {
    if (!chosen?.uses_baseline || earlier) return;
    const q = projectId ? `project_id=${encodeURIComponent(projectId)}` : "";
    api<{ sessions: ValidationSession[] }>("GET", `/v1/sessions?${q}`)
      .then((r) => setEarlier(r.sessions))
      .catch(() => setEarlier([]));
  }, [chosen?.uses_baseline, earlier, projectId]);

  return (
    <form
      className="new-session"
      onSubmit={(e) => {
        e.preventDefault();
        run(async () => {
          const res = await api<{ session: ValidationSession }>("POST", "/v1/sessions", {
            title,
            validation_type: type,
            project_id: projectId,
            baseline_session_id: chosen?.uses_baseline && baseline ? baseline : null,
          });
          navigate(`/sessions/${res.session.id}`);
        });
      }}
    >
      {error && <Notice kind="error">{error}</Notice>}
      <label className="field">
        <span className="field-label">Session title</span>
        <input value={title} required maxLength={160} autoFocus onChange={(e) => setTitle(e.target.value)}
          placeholder="For example: LIMS 8.2 initial validation" />
      </label>
      <fieldset className="type-picker">
        <legend className="field-label">What are you validating?</legend>
        {!types && <p className="muted">Loading…</p>}
        {types?.map((t) => (
          <label key={t.id} className={`type-option${t.available ? "" : " disabled"}${type === t.id ? " selected" : ""}`}>
            <input type="radio" name="type" value={t.id} checked={type === t.id} disabled={!t.available}
              onChange={() => setType(t.id)} />
            <span className="type-title">{t.title}{!t.available && <span className="soon">Later</span>}</span>
            <span className="muted">{t.summary}</span>
          </label>
        ))}
      </fieldset>
      {chosen?.uses_baseline && (
        <label className="field">
          <span className="field-label">Earlier validation it builds on</span>
          <select value={baseline} onChange={(e) => setBaseline(e.target.value)}>
            <option value="">None (add its approved documents as project files instead)</option>
            {earlier?.map((s) => (
              <option key={s.id} value={s.id}>{s.title} · {types?.find((t) => t.id === s.validation_type)?.title ?? s.validation_type}</option>
            ))}
          </select>
          <span className="muted small">Drafts can then cite that session's documents, for example its requirements and tests.</span>
        </label>
      )}
      <div className="form-actions">
        <button type="button" className="btn ghost" onClick={onCancel}>Cancel</button>
        <button className="btn primary" disabled={busy}>Start session</button>
      </div>
    </form>
  );
}

export function SessionTable({ sessions, showProject }: { sessions: ValidationSession[]; showProject?: boolean }) {
  const types = useValidationTypes();
  const typeTitle = (id: string) => types?.find((t) => t.id === id)?.title ?? id;
  const stepLabel = (s: ValidationSession) =>
    types?.find((t) => t.id === s.validation_type)?.steps.find((st) => st.id === s.current_step)?.label ?? "—";

  return (
    <div className="table-wrap">
      <table>
        <thead>
          <tr>
            <th>Session</th>
            {showProject && <th>Project</th>}
            <th className="hide-sm">Type</th>
            <th className="hide-sm">Current step</th>
            <th className="hide-sm">Updated</th>
          </tr>
        </thead>
        <tbody>
          {sessions.map((s) => (
            <tr key={s.id}>
              <td>
                <Link to={`/sessions/${s.id}`} className="row-link">{s.title}</Link>
                <div className="muted small">by {s.created_by_name}</div>
              </td>
              {showProject && <td>{s.project_name ?? <span className="muted">Unfiled</span>}</td>}
              <td className="hide-sm">{typeTitle(s.validation_type)}</td>
              <td className="hide-sm">{stepLabel(s)}</td>
              <td className="muted hide-sm nowrap">{formatTime(s.updated_at)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
