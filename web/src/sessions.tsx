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
  const { busy, error, run } = useAction();

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
