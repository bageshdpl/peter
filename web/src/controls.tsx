import { useCallback, useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { api, downloadFile, formatTime, type Source } from "./api";
import type { SessionEvents } from "./events";
import { messageOf, Notice, useAction } from "./ui";

type ControlStatus = "met" | "failed" | "not_executed" | "untested" | "not_covered" | "not_applicable";

interface ControlRow {
  key: string;
  group: string;
  reference: string;
  title: string;
  text: string;
  source: string | null;
  found: string[];
  linked: string[];
  tests: string[];
  not_applicable: boolean;
  reason: string;
  status: ControlStatus;
}

interface CoverageData {
  coverage: {
    rows: ControlRow[];
    summary: { controls: number; met: number; failed: number; not_executed: number; untested: number; not_covered: number; not_applicable: number };
    warnings: string[];
  };
  requirements: { id: string; text: string; status: string }[];
  standards: string[];
}

const STATUS: Record<ControlStatus, string> = {
  met: "Met",
  failed: "Failed",
  not_executed: "Not yet executed",
  untested: "No test",
  not_covered: "Not covered",
  not_applicable: "Not applicable",
};
// Reuses the traceability pill colours.
const PILL: Record<ControlStatus, string> = {
  met: "passed",
  failed: "failed",
  not_executed: "not_executed",
  untested: "not_tested",
  not_covered: "not_tested",
  not_applicable: "",
};

/** A session's coverage of its controls: regulations in scope and the project's SOP controls. */
export function ControlsPanel(props: { sessionId: string; sessionTitle: string; projectId: string | null; canWrite: boolean; events: SessionEvents }) {
  const { sessionId, sessionTitle, projectId, canWrite, events } = props;
  const [data, setData] = useState<CoverageData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<"all" | "open">("open");
  const [editing, setEditing] = useState<string | null>(null);
  const base = `/v1/sessions/${sessionId}/controls`;

  const load = useCallback(async () => {
    try {
      setData(await api("GET", base));
      setError(null);
    } catch (err) {
      setError(messageOf(err));
    }
  }, [base]);
  useEffect(() => {
    load();
  }, [load]);
  const { subscribe } = events;
  useEffect(() => subscribe((name) => (name === "document" || name === "controls" || name === "open") && load()), [subscribe, load]);

  const groups = useMemo(() => {
    if (!data) return [];
    const rows = filter === "open" ? data.coverage.rows.filter((r) => r.status !== "met" && r.status !== "not_applicable") : data.coverage.rows;
    const out: { name: string; rows: ControlRow[] }[] = [];
    for (const r of rows) {
      const g = out.find((g) => g.name === r.group);
      if (g) g.rows.push(r);
      else out.push({ name: r.group, rows: [r] });
    }
    return out;
  }, [data, filter]);

  if (!data) return <section className="card">{error ? <Notice kind="error">{error}</Notice> : <p className="muted">Loading controls…</p>}</section>;
  const s = data.coverage.summary;
  const open = s.controls - s.met - s.not_applicable;

  return (
    <section className="card trace controls">
      <div className="card-head">
        <div>
          <h2>Controls</h2>
          <p className="muted">
            What the regulations in scope and the project's SOPs require, followed through the requirements to their tests.
            A requirement covers a control when its row names it, such as "(21 CFR 11.10(e))", "(Annex 11, clause 9)" or "(CTL-003)",
            or when you link it here.
          </p>
        </div>
        {s.controls > 0 && (
          <div className="doc-tools">
            <button className="btn ghost" onClick={() => downloadFile(`${base}/export`, `${sessionTitle} controls.xlsx`).catch((e) => setError(messageOf(e)))}>
              ↓ Excel
            </button>
          </div>
        )}
      </div>
      {error && <Notice kind="error">{error}</Notice>}

      {s.controls === 0 ? (
        <div className="trace-empty">
          <p>
            No controls yet. Choose the regulations you work to in the <Link to="/library">Library</Link>
            {projectId ? <>, or add your SOPs' controls on the <Link to={`/projects/${projectId}`}>project page</Link></> : null}.
          </p>
        </div>
      ) : (
        <>
          <div className="trace-stats">
            <Stat n={s.controls} label="controls" />
            <Stat n={s.met} label="met" kind="ok" />
            <Stat n={s.failed} label="failed" kind="bad" />
            <Stat n={s.not_executed} label="not yet executed" kind="warn" />
            <Stat n={s.untested} label="without a test" kind="warn" />
            <Stat n={s.not_covered} label="not covered" kind="bad" />
            <Stat n={s.not_applicable} label="not applicable" />
          </div>
          <div className="progress big" aria-label={`${s.met} of ${s.controls - s.not_applicable} applicable controls met`}>
            <span style={{ width: `${s.controls - s.not_applicable ? (s.met / (s.controls - s.not_applicable)) * 100 : 100}%` }} />
          </div>
          <div className="tabs" role="tablist">
            <button role="tab" aria-selected={filter === "open"} onClick={() => setFilter("open")}>Open ({open})</button>
            <button role="tab" aria-selected={filter === "all"} onClick={() => setFilter("all")}>All ({s.controls})</button>
          </div>
          {groups.length === 0 && <p className="muted">Every applicable control is met.</p>}
          {groups.map((g) => (
            <div key={g.name} className="control-group">
              <h3>{g.name}</h3>
              <div className="table-wrap">
                <table className="trace-table">
                  <thead>
                    <tr>
                      <th>Control</th>
                      <th>Requirements</th>
                      <th className="hide-sm">Tests</th>
                      <th>Status</th>
                      {canWrite && <th aria-label="Actions" />}
                    </tr>
                  </thead>
                  <tbody>
                    {g.rows.map((r) =>
                      editing === r.key ? (
                        <tr key={r.key}>
                          <td colSpan={canWrite ? 5 : 4}>
                            <ControlEditor
                              row={r}
                              requirements={data.requirements}
                              url={`${base}/${encodeURIComponent(r.key)}`}
                              onDone={() => {
                                setEditing(null);
                                load();
                              }}
                            />
                          </td>
                        </tr>
                      ) : (
                        <tr key={r.key} className={`st-${r.status}`}>
                          <td>
                            <strong className="nowrap">{r.reference}</strong>
                            {r.title && <> · {r.title}</>}
                            <div className="small">{r.text}</div>
                            {r.source && <div className="small muted">From {r.source}</div>}
                          </td>
                          <td className="small">
                            {r.found.length + r.linked.length === 0 ? "—" : null}
                            {r.found.map((id) => <div key={id} className="nowrap">{id}</div>)}
                            {r.linked.map((id) => <div key={id} className="nowrap" title="Linked by hand">{id} <span className="muted">(linked)</span></div>)}
                          </td>
                          <td className="hide-sm small">{r.tests.join(", ") || "—"}</td>
                          <td>
                            <span className={`status-pill ${PILL[r.status]}`}>{STATUS[r.status]}</span>
                            {r.not_applicable && <div className="small muted">{r.reason}</div>}
                          </td>
                          {canWrite && (
                            <td>
                              <button className="btn ghost" onClick={() => setEditing(r.key)}>Edit</button>
                            </td>
                          )}
                        </tr>
                      ),
                    )}
                  </tbody>
                </table>
              </div>
            </div>
          ))}
        </>
      )}

      {data.coverage.warnings.length > 0 && (
        <div className="trace-warnings">
          <h3>Check these</h3>
          <ul>{data.coverage.warnings.map((w) => <li key={w}>{w}</li>)}</ul>
        </div>
      )}
    </section>
  );
}

function ControlEditor(props: { row: ControlRow; requirements: CoverageData["requirements"]; url: string; onDone: () => void }) {
  const { row, requirements, url, onDone } = props;
  const [linked, setLinked] = useState<string[]>(row.linked);
  const [na, setNa] = useState(row.not_applicable);
  const [reason, setReason] = useState(row.reason);
  const { busy, error, run } = useAction();
  const toggle = (id: string) => setLinked((l) => (l.includes(id) ? l.filter((x) => x !== id) : [...l, id]));

  return (
    <form
      className="control-editor"
      onSubmit={(e) => {
        e.preventDefault();
        run(async () => {
          await api("PUT", url, { requirements: linked, not_applicable: na, reason });
          onDone();
        });
      }}
    >
      <p>
        <strong>{row.reference}</strong> {row.title && <>· {row.title}</>}
        <br />
        <span className="small">{row.text}</span>
      </p>
      {row.found.length > 0 && <p className="small muted">Already covered by {row.found.join(", ")}, which name this control.</p>}
      <fieldset disabled={na}>
        <legend>Requirements that meet this control</legend>
        {requirements.length === 0 && <p className="small muted">There are no requirements yet. Draft the Requirements and risk step first.</p>}
        <div className="req-picks">
          {requirements.map((r) => (
            <label key={r.id} className={`req-pick${row.found.includes(r.id) ? " found" : ""}`} title={r.text}>
              <input
                type="checkbox"
                checked={row.found.includes(r.id) || linked.includes(r.id)}
                disabled={row.found.includes(r.id)}
                onChange={() => toggle(r.id)}
              />
              <span><strong>{r.id}</strong> {r.text}</span>
            </label>
          ))}
          {linked.filter((id) => !requirements.some((r) => r.id === id)).map((id) => (
            <label key={id} className="req-pick missing">
              <input type="checkbox" checked onChange={() => toggle(id)} />
              <span><strong>{id}</strong> is no longer in the requirements</span>
            </label>
          ))}
        </div>
      </fieldset>
      <label className="check-row">
        <input type="checkbox" checked={na} onChange={(e) => setNa(e.target.checked)} />
        Not applicable to this system
      </label>
      {na && (
        <label className="field">
          <span className="field-label">Why it does not apply</span>
          <textarea value={reason} onChange={(e) => setReason(e.target.value)} rows={2} maxLength={500} required />
          <span className="field-hint">Kept in the audit trail and shown in the export.</span>
        </label>
      )}
      {error && <Notice kind="error">{error}</Notice>}
      <div className="row-actions">
        <button className="btn primary" disabled={busy}>Save</button>
        <button type="button" className="btn ghost" onClick={onDone}>Cancel</button>
      </div>
    </form>
  );
}

interface ProjectControl {
  id: string;
  reference: string;
  number: number;
  text: string;
  section: string;
  source_id: string | null;
  source_filename: string | null;
  created_by_name: string;
  created_at: number;
}

/** The project's SOP controls: found in its source files or added by hand. */
export function ProjectControls({ projectId, canWrite, sourceCount }: { projectId: string; canWrite: boolean; sourceCount: number }) {
  const [controls, setControls] = useState<ProjectControl[] | null>(null);
  const [sources, setSources] = useState<Source[]>([]);
  const [pick, setPick] = useState("");
  const [adding, setAdding] = useState(false);
  const [text, setText] = useState("");
  const [section, setSection] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const { busy, error, setError, run } = useAction();
  const base = `/v1/projects/${projectId}/controls`;

  const load = useCallback(async () => {
    try {
      const [c, s] = await Promise.all([
        api<{ controls: ProjectControl[] }>("GET", base),
        api<{ sources: Source[] }>("GET", `/v1/projects/${projectId}/sources`),
      ]);
      setControls(c.controls);
      setSources(s.sources);
    } catch (err) {
      setError(messageOf(err));
    }
  }, [base, projectId, setError]);
  useEffect(() => {
    load();
  }, [load, sourceCount]);

  const readable = sources.filter((s) => !s.content_type.startsWith("image/"));
  const chosen = readable.find((s) => s.id === pick) ?? readable[0];

  return (
    <section className="card">
      <div className="card-head">
        <div>
          <h2>Controls</h2>
          <p className="muted">
            What your SOPs require of this system. Peter numbers them CTL-001, CTL-002 and so on; drafts name them in the requirements,
            and each session shows which are covered.
          </p>
        </div>
        {canWrite && !adding && <button className="btn ghost" onClick={() => setAdding(true)}>Add by hand</button>}
      </div>

      {canWrite && readable.length > 0 && (
        <div className="find-controls">
          <label className="field">
            <span className="field-label">Find "shall" and "must" statements in</span>
            <select value={chosen?.id ?? ""} onChange={(e) => setPick(e.target.value)}>
              {readable.map((s) => <option key={s.id} value={s.id}>{s.filename}</option>)}
            </select>
          </label>
          <button
            className="btn primary"
            disabled={busy || !chosen}
            onClick={() =>
              run(async () => {
                setMessage(null);
                const r = await api<{ added: string[]; skipped: number; truncated: boolean; controls: ProjectControl[] }>(
                  "POST", `/v1/projects/${projectId}/sources/${chosen!.id}/controls`);
                setControls(r.controls);
                const parts = [`Added ${r.added.length} control${r.added.length === 1 ? "" : "s"}`];
                if (r.skipped) parts.push(`${r.skipped} already in the list`);
                if (r.truncated) parts.push("only the first 300 statements were read");
                setMessage(`${parts.join("; ")}. Remove any that are not controls for this system.`);
              })
            }
          >
            Find controls
          </button>
        </div>
      )}

      {adding && (
        <form
          className="stack"
          onSubmit={(e) => {
            e.preventDefault();
            run(async () => {
              const r = await api<{ controls: ProjectControl[] }>("POST", base, { text, section });
              setControls(r.controls);
              setText("");
              setSection("");
              setAdding(false);
            });
          }}
        >
          <label className="field">
            <span className="field-label">Control</span>
            <textarea value={text} onChange={(e) => setText(e.target.value)} rows={2} maxLength={1000} required autoFocus
              placeholder="Backups shall be restored and checked every quarter." />
          </label>
          <label className="field">
            <span className="field-label">SOP section (optional)</span>
            <input value={section} onChange={(e) => setSection(e.target.value)} maxLength={40} placeholder="7.2" />
          </label>
          <div className="row-actions">
            <button className="btn primary" disabled={busy}>Add control</button>
            <button type="button" className="btn ghost" onClick={() => setAdding(false)}>Cancel</button>
          </div>
        </form>
      )}

      {error && <Notice kind="error">{error}</Notice>}
      {message && <Notice kind="ok">{message}</Notice>}
      {controls && controls.length === 0 && (
        <p className="muted">No controls yet. {readable.length ? "Find them in an SOP above, or add them by hand." : "Add an SOP under Sources, or add controls by hand."}</p>
      )}
      {controls && controls.length > 0 && (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>ID</th>
                <th>Control</th>
                <th className="hide-sm">From</th>
                {canWrite && <th aria-label="Actions" />}
              </tr>
            </thead>
            <tbody>
              {controls.map((c) => (
                <tr key={c.id}>
                  <td className="nowrap"><strong>{c.reference}</strong></td>
                  <td>
                    {c.text}
                    {c.section && <div className="small muted">Section {c.section}</div>}
                  </td>
                  <td className="hide-sm small muted">
                    {c.source_filename ?? "Added by hand"}
                    <div>{c.created_by_name}, {formatTime(c.created_at)}</div>
                  </td>
                  {canWrite && (
                    <td>
                      <button
                        className="btn ghost danger"
                        disabled={busy}
                        onClick={() => {
                          if (!confirm(`Remove ${c.reference}? Its number is not reused.`)) return;
                          run(async () => {
                            await api("DELETE", `${base}/${c.id}`);
                            setControls((list) => list?.filter((x) => x.id !== c.id) ?? null);
                          });
                        }}
                      >
                        Remove
                      </button>
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function Stat({ n, label, kind }: { n: number; label: string; kind?: "ok" | "bad" | "warn" }) {
  return (
    <div className={`stat ${n > 0 ? kind ?? "" : ""}`}>
      <span className="n">{n}</span>
      <span className="l">{label}</span>
    </div>
  );
}
