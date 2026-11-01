import { useCallback, useEffect, useState } from "react";
import { api, downloadFile } from "./api";
import type { SessionEvents } from "./events";
import { messageOf, Notice, useAction } from "./ui";

type Status = "passed" | "failed" | "not_executed" | "not_tested";
type Outcome = "passed" | "failed" | "not_executed";

interface Matrix {
  rows: {
    requirement: { id: string; text: string; category: string; gxp_critical: string; risk: string; evidence: string[] };
    specs: string[];
    tests: string[];
    status: Status;
    high_risk: boolean;
  }[];
  tests: { id: string; step: string; title: string; requirements: string[]; outcome: Outcome; steps_total: number; steps_passed: number; steps_failed: number }[];
  summary: { requirements: number; passed: number; failed: number; not_executed: number; not_tested: number; high_risk_open: number; tests: number };
  warnings: string[];
}

const STATUS: Record<Status, string> = { passed: "Passed", failed: "Failed", not_executed: "Not yet executed", not_tested: "No test" };

export function TracePanel(props: {
  sessionId: string;
  sessionTitle: string;
  canWrite: boolean;
  events: SessionEvents;
  onGoToStep: (stepId: string) => void;
}) {
  const { sessionId, sessionTitle, canWrite, events, onGoToStep } = props;
  const [data, setData] = useState<{ matrix: Matrix; built_from: { step_id: string; title: string; version: number }[] } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<"all" | "open">("all");
  const { busy, error: writeError, run } = useAction();
  const base = `/v1/sessions/${sessionId}/trace`;

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
  // Any document change can change the matrix.
  useEffect(() => subscribe((name) => (name === "document" || name === "open") && load()), [subscribe, load]);

  if (!data) return <section className="card">{error ? <Notice kind="error">{error}</Notice> : <p className="muted">Loading…</p>}</section>;
  const { matrix: m, built_from } = data;
  const s = m.summary;
  const testOf = (id: string) => m.tests.find((t) => t.id === id);
  const rows = filter === "open" ? m.rows.filter((r) => r.status !== "passed") : m.rows;

  return (
    <section className="card trace">
      <div className="card-head">
        <div>
          <h2>Traceability</h2>
          <p className="muted">
            Read live from {built_from.length ? built_from.map((b) => `${b.title} v${b.version}`).join(", ") : "the session's documents"}.
            Peter builds this without AI, from the IDs in those documents (URS, FS, CS, TS, IQ).
          </p>
        </div>
        <div className="doc-tools">
          {s.requirements > 0 && (
            <button className="btn ghost" onClick={() => downloadFile(`${base}/export`, `${sessionTitle} traceability.xlsx`).catch((e) => setError(messageOf(e)))}>
              ↓ Excel
            </button>
          )}
          {canWrite && s.requirements > 0 && (
            <button className="btn primary" disabled={busy} onClick={() => run(async () => { await api("POST", `${base}/write`); })}>
              Write into document
            </button>
          )}
        </div>
      </div>
      {error && <Notice kind="error">{error}</Notice>}
      {writeError && <Notice kind="error">{writeError}</Notice>}

      {s.requirements === 0 ? (
        <div className="trace-empty">
          <p>{m.warnings[0] ?? "No requirements yet."}</p>
          <button className="btn ghost" onClick={() => onGoToStep("requirements")}>Go to Requirements and risk</button>
        </div>
      ) : (
        <>
          <div className="trace-stats">
            <Stat n={s.requirements} label="requirements" />
            <Stat n={s.passed} label="passed" kind="ok" />
            <Stat n={s.failed} label="failed" kind="bad" />
            <Stat n={s.not_executed} label="not yet executed" kind="warn" />
            <Stat n={s.not_tested} label="without a test" kind="warn" />
            <Stat n={s.high_risk_open} label="high risk still open" kind={s.high_risk_open ? "bad" : "ok"} />
          </div>
          <div className="progress big" aria-label={`${s.passed} of ${s.requirements} passed`}>
            <span style={{ width: `${(s.passed / s.requirements) * 100}%` }} />
          </div>

          <div className="tabs" role="tablist">
            <button role="tab" aria-selected={filter === "all"} onClick={() => setFilter("all")}>All ({m.rows.length})</button>
            <button role="tab" aria-selected={filter === "open"} onClick={() => setFilter("open")}>Open ({m.rows.length - s.passed})</button>
          </div>
          <div className="table-wrap">
            <table className="trace-table">
              <thead>
                <tr>
                  <th>Requirement</th>
                  <th>Risk</th>
                  <th className="hide-sm">Specification</th>
                  <th>Tests</th>
                  <th>Result</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.requirement.id} className={`st-${r.status}`}>
                    <td>
                      <strong className="nowrap">{r.requirement.id}</strong>
                      <div className="small">{r.requirement.text}</div>
                    </td>
                    <td><span className={`risk risk-${r.requirement.risk.split(" ")[0] || "none"}`}>{r.requirement.risk || "—"}</span></td>
                    <td className="hide-sm small">{r.specs.join(", ") || "—"}</td>
                    <td className="small">
                      {r.tests.length === 0 ? "—" : r.tests.map((id) => {
                        const t = testOf(id);
                        return (
                          <div key={id} className="nowrap" title={t ? `${t.title} · ${t.steps_passed}/${t.steps_total} steps passed` : ""}>
                            <span className={`dot-${t?.outcome ?? "not_executed"}`} /> {id}
                          </div>
                        );
                      })}
                    </td>
                    <td><span className={`status-pill ${r.status}`}>{STATUS[r.status]}</span></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}

      {m.warnings.length > 0 && s.requirements > 0 && (
        <div className="trace-warnings">
          <h3>Check these</h3>
          <ul>{m.warnings.map((w) => <li key={w}>{w}</li>)}</ul>
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
