import { useCallback, useEffect, useRef, useState } from "react";
import { api, formatTime, uploadFile, type Provider } from "./api";
import { Markdown } from "./markdown";
import { messageOf, Notice, useAction } from "./ui";

interface StyleData {
  guide: { guide: string; source: "ai" | "person"; model: string | null; updated_by_name: string; updated_at: number } | null;
  samples: { id: string; filename: string; sha256: string; chars: number; created_by_name: string; created_at: number }[];
  max_samples: number;
}

/** The Library's Writing style tab: past documents, and the guide drafts follow. */
export function WritingStyle({ canEdit }: { canEdit: boolean }) {
  const [data, setData] = useState<StyleData | null>(null);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [provider, setProvider] = useState("");
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const { busy, error, setError, run } = useAction();

  const load = useCallback(async () => {
    try {
      const [d, p] = await Promise.all([api<StyleData>("GET", "/v1/library/style"), api<{ providers: Provider[] }>("GET", "/v1/ai/providers")]);
      setData(d);
      const ai = p.providers.filter((x) => x.is_ai);
      setProviders(ai);
      setProvider((cur) => cur || ai[0]?.id || "");
    } catch (err) {
      setError(messageOf(err));
    }
  }, [setError]);
  useEffect(() => {
    load();
  }, [load]);

  if (!data) return <section className="card">{error ? <Notice kind="error">{error}</Notice> : <p className="muted">Loading…</p>}</section>;
  const g = data.guide;

  return (
    <>
      <section className="card">
        <div className="card-head">
          <div>
            <h2>Writing style</h2>
            <p className="muted">
              How your company writes validation documents: wording, structure, test-step format and level of detail. Every AI draft
              follows it. The rules on evidence and citations always come first.
            </p>
          </div>
          {canEdit && !editing && (
            <div className="doc-tools">
              <button className="btn ghost" onClick={() => { setText(g?.guide ?? ""); setEditing(true); }}>{g ? "Edit" : "Write by hand"}</button>
            </div>
          )}
        </div>
        {message && <Notice kind="ok">{message}</Notice>}
        {error && <Notice kind="error">{error}</Notice>}
        {editing ? (
          <form
            className="stack"
            onSubmit={(e) => {
              e.preventDefault();
              run(async () => {
                await api("PUT", "/v1/library/style", { guide: text });
                setEditing(false);
                setMessage(text.trim() ? "Saved. New drafts follow it." : "Cleared. Drafts no longer follow a style guide.");
                await load();
              });
            }}
          >
            <label className="field">
              <span className="field-label">Style guide (Markdown)</span>
              <textarea rows={14} value={text} onChange={(e) => setText(e.target.value)} maxLength={6000}
                placeholder={"## Test scripts\n- Each step has Action, Expected result, Actual result and Pass/Fail.\n- Expected results are written in the present tense."} />
              <span className="field-hint">{text.length} of 6000 characters. Leave it empty to stop using a style guide.</span>
            </label>
            <div className="row-actions">
              <button className="btn primary" disabled={busy}>Save</button>
              <button type="button" className="btn ghost" onClick={() => setEditing(false)}>Cancel</button>
            </div>
          </form>
        ) : g ? (
          <>
            <p className="small muted">
              {g.source === "ai" ? `Learnt from the samples by ${g.model ?? "AI"}` : "Written by hand"} · {g.updated_by_name}, {formatTime(g.updated_at)}
            </p>
            <div className="style-guide"><Markdown text={g.guide} handlers={{ chip: (id, key) => <span key={key}>[{id}]</span> }} /></div>
          </>
        ) : (
          <p className="muted">No style guide yet. Add a few past documents below and let Peter learn it, or write one by hand.</p>
        )}
      </section>

      <section className="card">
        <div className="card-head">
          <div>
            <h2>Sample documents</h2>
            <p className="muted">
              Up to {data.max_samples} approved documents your company wrote before (Word, PDF, text or Markdown). Peter keeps only their
              text, and the AI is told to describe the style without copying their facts.
            </p>
          </div>
          {canEdit && (
            <div className="doc-tools">
              <input ref={fileRef} type="file" hidden accept=".docx,.pdf,.txt,.md"
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  e.target.value = "";
                  if (file) run(async () => { await uploadFile("/v1/library/style/samples", file); setMessage(null); await load(); });
                }} />
              <button className="btn ghost" disabled={busy || data.samples.length >= data.max_samples} onClick={() => fileRef.current?.click()}>
                Add a document
              </button>
            </div>
          )}
        </div>
        {data.samples.length === 0 ? (
          <p className="muted">No samples yet.</p>
        ) : (
          <div className="table-wrap">
            <table>
              <thead><tr><th>File</th><th className="hide-sm">Text read</th><th className="hide-sm">Added</th>{canEdit && <th aria-label="Actions" />}</tr></thead>
              <tbody>
                {data.samples.map((s) => (
                  <tr key={s.id}>
                    <td>{s.filename}</td>
                    <td className="hide-sm small">{s.chars.toLocaleString()} characters</td>
                    <td className="hide-sm small muted">{s.created_by_name}, {formatTime(s.created_at)}</td>
                    {canEdit && (
                      <td>
                        <button className="btn ghost danger" disabled={busy}
                          onClick={() => run(async () => { await api("DELETE", `/v1/library/style/samples/${s.id}`); await load(); })}>
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
        {canEdit && data.samples.length > 0 && (
          <div className="find-controls">
            {providers.length === 0 ? (
              <p className="muted">Learning a style needs an AI key. Add one under AI and usage, or write the guide by hand.</p>
            ) : (
              <>
                <label className="field">
                  <span className="field-label">Learn with</span>
                  <select value={provider} onChange={(e) => setProvider(e.target.value)}>
                    {providers.map((p) => <option key={p.id} value={p.id}>{p.name} · {p.model}</option>)}
                  </select>
                </label>
                <button className="btn primary" disabled={busy || !provider}
                  onClick={() => (!g || confirm("Replace the current style guide with a new one learnt from the samples?")) &&
                    run(async () => {
                      setMessage(null);
                      await api("POST", "/v1/library/style/learn", { provider });
                      setMessage("Learnt. Read it through and edit anything that is not right.");
                      await load();
                    })}>
                  {busy ? "Learning…" : g ? "Learn again" : "Learn the style"}
                </button>
              </>
            )}
          </div>
        )}
      </section>
    </>
  );
}
