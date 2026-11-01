import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api, atLeast, formatTime } from "../api";
import { useSession } from "../App";
import { messageOf, Notice, useAction } from "../ui";

interface ProviderState {
  id: "openrouter" | "gemini";
  name: string;
  workspace_key: { hint: string; model: string; set_by_name: string; set_at: number; readable: boolean } | null;
  server_key: boolean;
  server_model: string;
  in_use: "workspace" | "server" | null;
}

interface AiState {
  providers: ProviderState[];
  shared: boolean;
  shared_monthly_tokens: number | null;
  shared_tokens_this_month: number;
}

interface Usage {
  days: number;
  totals: { provider: string; key_source: string; calls: number; failed: number; prompt_tokens: number; completion_tokens: number; cost_usd: number | null }[];
  people: { user_id: string; name: string; calls: number; tokens: number; cost_usd: number | null }[];
  recent: {
    created_at: number; user_name: string; session_id: string | null; session_title: string | null; step_id: string | null;
    provider: string; model: string; key_source: string; prompt_tokens: number | null; completion_tokens: number | null;
    cost_usd: number | null; ok: boolean; error: string | null;
  }[];
}

const WHERE: Record<string, string> = {
  openrouter: "openrouter.ai → Keys. You can set a spending limit on the key there.",
  gemini: "aistudio.google.com → Get API key.",
};

const n = (x: number) => x.toLocaleString();
const usd = (x: number | null) => (x == null ? "—" : `$${x < 0.01 && x > 0 ? x.toFixed(4) : x.toFixed(2)}`);

export default function AiSettings() {
  const session = useSession()!;
  const isAdmin = atLeast(session.workspace.role, "admin");
  const [state, setState] = useState<AiState | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setState(await api<AiState>("GET", "/v1/workspace/ai"));
      setError(null);
    } catch (err) {
      setError(messageOf(err));
    }
  }, []);
  useEffect(() => {
    load();
  }, [load, session.workspace.id]);

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <p className="eyebrow">{session.workspace.name}</p>
          <h1>AI and usage</h1>
          <p className="muted">
            Which AI keys this workspace drafts with, and what it has used. With its own key, the workspace pays its provider
            directly{state?.shared ? " and is not held to the server's shared allowance" : ""}.
          </p>
        </div>
      </header>
      {error && <Notice kind="error">{error}</Notice>}
      {state && (
        <>
          {state.shared && state.shared_monthly_tokens != null && (
            <section className="card">
              <h2>Shared key allowance</h2>
              <p className="muted">
                This server lets each workspace use {n(state.shared_monthly_tokens)} tokens a month on the server's shared key. This workspace has used{" "}
                <strong>{n(state.shared_tokens_this_month)}</strong> this month.
              </p>
              <div className="progress big">
                <span style={{ width: `${Math.min(100, (state.shared_tokens_this_month / state.shared_monthly_tokens) * 100)}%` }} />
              </div>
            </section>
          )}
          {state.providers.map((p) => <ProviderCard key={p.id} p={p} isAdmin={isAdmin} onChange={load} />)}
        </>
      )}
      {isAdmin && <UsageCard key={session.workspace.id} />}
    </div>
  );
}

function ProviderCard({ p, isAdmin, onChange }: { p: ProviderState; isAdmin: boolean; onChange: () => void }) {
  const [editing, setEditing] = useState(false);
  const [key, setKey] = useState("");
  const [model, setModel] = useState(p.workspace_key?.model ?? "");
  const [check, setCheck] = useState<{ ok: boolean; message: string } | null>(null);
  const { busy, error, run } = useAction();
  const path = `/v1/workspace/ai/${p.id}`;

  const status =
    p.in_use === "workspace" ? "Using this workspace's key" : p.in_use === "server" ? "Using the server's shared key" : "Not available";

  return (
    <section className="card">
      <div className="card-head">
        <div>
          <h2>{p.name}</h2>
          <p className="muted">
            <span className={`status-pill ${p.in_use ? "passed" : ""}`}>{status}</span>
            {p.in_use === "workspace" && p.workspace_key && (
              <> · key {p.workspace_key.hint}, model {p.workspace_key.model || p.server_model} · set by {p.workspace_key.set_by_name}, {formatTime(p.workspace_key.set_at)}</>
            )}
            {p.in_use === "server" && <> · model {p.server_model}</>}
          </p>
          {p.workspace_key && !p.workspace_key.readable && (
            <Notice kind="error">The saved key cannot be read: the server's secret key changed. Enter the key again.</Notice>
          )}
        </div>
        {isAdmin && !editing && (
          <div className="doc-tools">
            {p.in_use && (
              <button className="btn ghost" disabled={busy}
                onClick={() => run(async () => setCheck(await api("POST", `${path}/check`)))}>
                Check key
              </button>
            )}
            <button className="btn ghost" onClick={() => setEditing(true)}>{p.workspace_key ? "Change" : "Add workspace key"}</button>
            {p.workspace_key && (
              <button className="btn ghost danger" disabled={busy}
                onClick={() => confirm(`Remove this workspace's ${p.name} key?`) && run(async () => { await api("DELETE", path); onChange(); })}>
                Remove
              </button>
            )}
          </div>
        )}
      </div>
      {check && <Notice kind={check.ok ? "ok" : "error"}>{check.message}</Notice>}
      {error && <Notice kind="error">{error}</Notice>}
      {editing && (
        <form
          className="stack"
          onSubmit={(e) => {
            e.preventDefault();
            run(async () => {
              await api("PUT", path, { key: key || undefined, model });
              setKey("");
              setEditing(false);
              setCheck(null);
              onChange();
            });
          }}
        >
          <label className="field">
            <span className="field-label">{p.workspace_key ? "New key (leave empty to keep the current one)" : "API key"}</span>
            <input type="password" value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" spellCheck={false}
              required={!p.workspace_key} placeholder={p.id === "openrouter" ? "sk-or-v1-…" : "AIza…"} />
            <span className="field-hint">
              Get one at {WHERE[p.id]} Peter stores it encrypted and never shows it again; only its last four characters are kept visible.
            </span>
          </label>
          <label className="field">
            <span className="field-label">Model (optional)</span>
            <input value={model} onChange={(e) => setModel(e.target.value)} maxLength={100} placeholder={p.server_model} spellCheck={false} />
            <span className="field-hint">Empty uses {p.server_model}.</span>
          </label>
          <div className="row-actions">
            <button className="btn primary" disabled={busy}>Save</button>
            <button type="button" className="btn ghost" onClick={() => setEditing(false)}>Cancel</button>
          </div>
        </form>
      )}
    </section>
  );
}

function UsageCard() {
  const [days, setDays] = useState(30);
  const [usage, setUsage] = useState<Usage | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api<Usage>("GET", `/v1/workspace/ai/usage?days=${days}`).then(setUsage).catch((e) => setError(messageOf(e)));
  }, [days]);

  const calls = usage?.totals.reduce((a, t) => a + t.calls, 0) ?? 0;
  const tokens = usage?.totals.reduce((a, t) => a + t.prompt_tokens + t.completion_tokens, 0) ?? 0;
  const costs = usage?.totals.filter((t) => t.cost_usd != null) ?? [];
  const cost = costs.length ? costs.reduce((a, t) => a + (t.cost_usd ?? 0), 0) : null;
  const failed = usage?.totals.reduce((a, t) => a + t.failed, 0) ?? 0;

  return (
    <section className="card">
      <div className="card-head">
        <div>
          <h2>Usage</h2>
          <p className="muted">Every AI draft, whether it worked or not. Cost is what OpenRouter reports; Gemini does not report cost.</p>
        </div>
        <label className="field compact">
          <span className="field-label">Period</span>
          <select value={days} onChange={(e) => setDays(Number(e.target.value))}>
            <option value={7}>Last 7 days</option>
            <option value={30}>Last 30 days</option>
            <option value={90}>Last 90 days</option>
            <option value={365}>Last year</option>
          </select>
        </label>
      </div>
      {error && <Notice kind="error">{error}</Notice>}
      {usage && (
        <>
          <div className="trace-stats">
            <div className="stat"><span className="n">{n(calls)}</span><span className="l">AI drafts</span></div>
            <div className={`stat ${failed ? "bad" : ""}`}><span className="n">{n(failed)}</span><span className="l">failed</span></div>
            <div className="stat"><span className="n">{n(tokens)}</span><span className="l">tokens</span></div>
            <div className="stat"><span className="n">{usd(cost)}</span><span className="l">cost reported</span></div>
          </div>
          {calls === 0 ? (
            <p className="muted">No AI drafts in this period. Sample drafts are not counted.</p>
          ) : (
            <>
              <h3 className="sub">By key</h3>
              <div className="table-wrap">
                <table>
                  <thead><tr><th>Provider</th><th>Key</th><th>Drafts</th><th>Tokens in / out</th><th>Cost</th></tr></thead>
                  <tbody>
                    {usage.totals.map((t) => (
                      <tr key={`${t.provider}-${t.key_source}`}>
                        <td>{t.provider}</td>
                        <td>{t.key_source === "workspace" ? "Workspace" : "Server (shared)"}</td>
                        <td>{n(t.calls)}{t.failed ? <span className="muted"> ({t.failed} failed)</span> : null}</td>
                        <td>{n(t.prompt_tokens)} / {n(t.completion_tokens)}</td>
                        <td>{usd(t.cost_usd)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <h3 className="sub">By person</h3>
              <div className="table-wrap">
                <table>
                  <thead><tr><th>Person</th><th>Drafts</th><th>Tokens</th><th>Cost</th></tr></thead>
                  <tbody>
                    {usage.people.map((p) => (
                      <tr key={p.user_id}><td>{p.name}</td><td>{n(p.calls)}</td><td>{n(p.tokens)}</td><td>{usd(p.cost_usd)}</td></tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <h3 className="sub">Latest</h3>
              <div className="table-wrap">
                <table>
                  <thead><tr><th>When</th><th>Who</th><th>What</th><th className="hide-sm">Model</th><th>Tokens</th><th>Cost</th></tr></thead>
                  <tbody>
                    {usage.recent.map((r, i) => (
                      <tr key={i}>
                        <td className="small nowrap">{formatTime(r.created_at)}</td>
                        <td className="small">{r.user_name}</td>
                        <td className="small">
                          {r.session_id ? <Link to={`/sessions/${r.session_id}${r.step_id ? `?step=${r.step_id}` : ""}`}>{r.session_title}</Link> : "—"}
                          {!r.ok && <div className="error-text" title={r.error ?? ""}>Failed: {r.error?.slice(0, 80)}</div>}
                        </td>
                        <td className="hide-sm small">{r.model || r.provider}{r.key_source === "workspace" ? " · own key" : ""}</td>
                        <td className="small">{r.prompt_tokens != null ? n((r.prompt_tokens ?? 0) + (r.completion_tokens ?? 0)) : "—"}</td>
                        <td className="small">{usd(r.cost_usd)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </>
          )}
        </>
      )}
    </section>
  );
}
