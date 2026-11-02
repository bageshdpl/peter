import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api, downloadFile, formatTime, type DocumentVersion, type DocumentView, type Provider, type Step } from "./api";
import type { SessionEvents } from "./events";
import { Markdown } from "./markdown";
import { Zoom } from "./observe";
import { CommentsPanel, ExportButtons, SignaturePanel } from "./review";
import { messageOf, Notice } from "./ui";

// Fetched each time a document opens: an admin may have added a workspace key.
function useProviders() {
  const [providers, setProviders] = useState<Provider[] | null>(null);
  useEffect(() => {
    api<{ providers: Provider[] }>("GET", "/v1/ai/providers").then((r) => setProviders(r.providers)).catch(() => setProviders([]));
  }, []);
  return providers;
}

const PROVIDER_KEY = "peter.provider";

export function DocumentPanel(props: {
  sessionId: string;
  step: Step;
  steps: Step[];
  projectId: string | null;
  canWrite: boolean;
  /** Commenting is open to every role, viewers included, unless archived. */
  canComment: boolean;
  /** Members and above may resolve anyone's comment. */
  canResolveAny: boolean;
  userId: string;
  events: SessionEvents;
  onGoToStep: (stepId: string) => void;
}) {
  const { sessionId, step, steps, projectId, canWrite, canComment, canResolveAny, userId, events, onGoToStep } = props;
  const [quote, setQuote] = useState("");
  const providers = useProviders();
  const [view, setView] = useState<DocumentView | null>(null);
  const [shown, setShown] = useState<DocumentVersion | null>(null); // An older version being looked at.
  const [error, setError] = useState<string | null>(null);
  const [provider, setProvider] = useState<string>(() => {
    try {
      return localStorage.getItem(PROVIDER_KEY) ?? "";
    } catch {
      return "";
    }
  });
  const [instructions, setInstructions] = useState("");
  const [showInstructions, setShowInstructions] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [zoom, setZoom] = useState<{ id: string; title: string } | null>(null);
  const path = `/v1/sessions/${sessionId}/documents/${step.id}`;

  const load = useCallback(async () => {
    try {
      setView(await api<DocumentView>("GET", path));
      setError(null);
    } catch (err) {
      setError(messageOf(err));
    }
  }, [path]);

  useEffect(() => {
    setView(null);
    setShown(null);
    setEditing(null);
    load();
  }, [load]);

  const { subscribe } = events;
  useEffect(
    () =>
      subscribe((name, data) => {
        if (name === "open" || name === "resync") load();
        if (name === "document" && (data as { step_id?: string })?.step_id === step.id) load();
      }),
    [subscribe, load, step.id],
  );

  // Pick a provider: the last one used if still offered, else the first AI one.
  useEffect(() => {
    if (!providers?.length) return;
    if (!providers.some((p) => p.id === provider)) setProvider((providers.find((p) => p.is_ai) ?? providers[0]).id);
  }, [providers, provider]);

  async function startDraft() {
    setBusy(true);
    setError(null);
    try {
      localStorage.setItem(PROVIDER_KEY, provider);
    } catch {
      // Only a convenience.
    }
    try {
      await api("POST", `${path}/draft`, { provider, instructions });
      setShown(null);
      await load();
    } catch (err) {
      setError(messageOf(err));
    } finally {
      setBusy(false);
    }
  }

  async function save() {
    if (editing === null || !view?.document) return;
    setBusy(true);
    try {
      setView(await api<DocumentView>("PUT", path, { body: editing, base_version: view.document.current_version }));
      setEditing(null);
      setShown(null);
      setError(null);
    } catch (err) {
      setError(messageOf(err));
    } finally {
      setBusy(false);
    }
  }

  async function showVersion(v: number) {
    if (!view?.document || v === view.document.current_version) return setShown(null);
    try {
      setShown(await api<DocumentVersion>("GET", `${path}/versions/${v}`));
    } catch (err) {
      setError(messageOf(err));
    }
  }

  if (!view) return <section className="card"><p className="muted">Loading…</p></section>;
  if (!view.draftable) return null;

  const doc = view.document;
  const version = shown ?? view.current ?? null;
  const drafting = doc?.status === "drafting";
  const chosen = providers?.find((p) => p.id === provider);
  const stepTitle = (id: string) => steps.find((s) => s.id === id)?.label ?? id;

  // The server's check decides what is missing, for every kind of citation.
  const missing = new Set(version?.citations.unknown ?? []);
  const chip = (id: string, key: string) => {
    if (missing.has(id)) {
      return (
        <span key={key} className="cite missing" title="Cites evidence that does not exist">
          {id}
        </span>
      );
    }
    if (id.startsWith("O")) {
      const o = version?.observations[id];
      return (
        <button key={key} className={`cite cite-o${o ? "" : " missing"}`} title={o ? `Step ${o.seq}: ${o.label}` : "No such step"}
          onClick={() => o?.has_screenshot && setZoom({ id: o.id, title: `Step ${o.seq}: ${o.label}` })}>
          {id}
        </button>
      );
    }
    if (id.startsWith("S")) {
      const s = version?.citations.sources?.[id];
      return (
        <button key={key} className={`cite cite-s${s ? "" : " missing"}`} title={s?.filename ?? "No such file"}
          onClick={() => s && projectId && downloadFile(`/v1/projects/${projectId}/sources/${s.id}/file`, s.filename).catch(() => {})}>
          {id}
        </button>
      );
    }
    const target = id.slice(2);
    if (id.startsWith("B:")) {
      const b = version?.citations.baseline;
      const label = b?.documents[target] ?? target;
      return (
        <Link key={key} className="cite cite-b" title={b ? `${label} in the earlier validation "${b.title}"` : "Earlier validation"}
          to={b ? `/sessions/${b.session_id}?step=${target}` : "#"}>
          {label} (earlier)
        </Link>
      );
    }
    return (
      <button key={key} className="cite cite-d" title={stepTitle(target)} onClick={() => onGoToStep(target)}>
        {stepTitle(target)}
      </button>
    );
  };

  const cit = version?.citations;
  const counts = {
    steps: cit?.cited?.filter((c) => c.startsWith("O")).length ?? 0,
    files: cit?.cited?.filter((c) => c.startsWith("S")).length ?? 0,
    docs: cit?.cited?.filter((c) => c.startsWith("D:")).length ?? 0,
    baseline: cit?.cited?.filter((c) => c.startsWith("B:")).length ?? 0,
  };

  return (
    <section className="card doc-card">
      <div className="card-head">
        <div>
          <h2>{view.title}</h2>
          {version && (
            <p className="muted small">
              Version {version.version}
              {shown && " (older version)"} ·{" "}
              {version.origin === "ai"
                ? `Drafted with ${version.provider === "sample" ? "the sample drafter (no AI)" : `${version.provider}, ${version.model}`}`
                : version.origin === "generated" ? "Built by Peter from the session's documents (no AI)" : "Edited"}{" "}
              by {version.created_by_name}, {formatTime(version.created_at)}
            </p>
          )}
        </div>
        <div className="doc-tools">
          {doc && (view.versions?.length ?? 0) > 1 && (
            <select aria-label="Version" value={shown?.version ?? doc.current_version} onChange={(e) => showVersion(Number(e.target.value))}>
              {view.versions!.map((v) => (
                <option key={v.version} value={v.version}>
                  v{v.version} · {v.origin === "ai" ? v.provider : v.origin === "generated" ? "built" : "edit"} · {v.created_by_name}
                </option>
              ))}
            </select>
          )}
          {version && editing === null && (
            <ExportButtons base={path} title={view.title ?? "Document"} version={version.version} hasTables={/^\s*\|?\s*:?-{3,}/m.test(version.body)} />
          )}
        </div>
      </div>

      {error && <Notice kind="error">{error}</Notice>}
      {doc?.status === "failed" && doc.error && <Notice kind="error">The last draft failed: {doc.error}</Notice>}

      {canWrite && editing === null && !shown && (
        <div className="draft-bar">
          {providers && providers.length === 0 ? (
            <span className="muted">No AI provider is set up on the server. See the README to add an OpenRouter or Gemini key.</span>
          ) : (
            <>
              <label className="field compact">
                <span className="field-label">Draft with</span>
                <select value={provider} onChange={(e) => setProvider(e.target.value)} disabled={drafting || busy}>
                  {providers?.map((p) => (
                    <option key={p.id} value={p.id}>{p.is_ai ? `${p.name} · ${p.model}${p.key_source === "workspace" ? " (workspace key)" : ""}` : p.name}</option>
                  ))}
                </select>
              </label>
              <button className="btn primary" disabled={drafting || busy || !provider} onClick={startDraft}>
                {drafting ? "Drafting…" : version ? "Redraft" : "Draft with AI"}
              </button>
              <button className="btn ghost" type="button" onClick={() => setShowInstructions((v) => !v)}>
                {showInstructions ? "Hide instructions" : "Add instructions"}
              </button>
              {version && (
                <button className="btn ghost" disabled={drafting} onClick={() => setEditing(version.body)}>Edit</button>
              )}
            </>
          )}
          {showInstructions && (
            <label className="field instructions">
              <span className="field-label">Extra instructions for this draft (optional)</span>
              <textarea rows={2} maxLength={2000} value={instructions} onChange={(e) => setInstructions(e.target.value)}
                placeholder="For example: the system is used only at the Dublin site; use the company's URS numbering" />
            </label>
          )}
          {chosen && !chosen.is_ai && (
            <p className="muted small">The sample drafter lists the evidence without AI. Add an OpenRouter or Gemini key on the server for real drafts.</p>
          )}
        </div>
      )}

      {drafting && (
        <div className="drafting">
          <span className="spinner" aria-hidden />
          Drafting {view.title?.toLowerCase()} from the session's evidence. This can take a minute; you can leave this page.
        </div>
      )}

      {editing !== null ? (
        <div className="editor">
          <p className="muted small">
            Markdown. Cite evidence as [O12] (observed step), [S2] (source file), [D:requirements] (earlier
            document) or [B:testing] (a document from the earlier validation this session builds on). Mark missing
            evidence as [GAP: what is missing].
          </p>
          <textarea value={editing} onChange={(e) => setEditing(e.target.value)} rows={24} aria-label="Document text" />
          <div className="form-actions">
            <button className="btn ghost" onClick={() => setEditing(null)}>Cancel</button>
            <button className="btn primary" disabled={busy} onClick={save}>Save as new version</button>
          </div>
        </div>
      ) : version ? (
        <>
          <div className="cite-summary">
            <span>Cites {counts.steps} observed {counts.steps === 1 ? "step" : "steps"}, {counts.files} {counts.files === 1 ? "file" : "files"}, {counts.docs} earlier {counts.docs === 1 ? "document" : "documents"}
              {counts.baseline > 0 && <>, {counts.baseline} from the earlier validation</>}.</span>
            {(cit?.gaps ?? 0) > 0 && <span className="warn">{cit!.gaps} {cit!.gaps === 1 ? "gap" : "gaps"} need evidence.</span>}
            {(cit?.unknown?.length ?? 0) > 0 && (
              <span className="bad">Cites evidence that does not exist: {cit!.unknown!.join(", ")}. Check those statements.</span>
            )}
          </div>
          {/* Selecting text in the document sets the quote for a new comment. */}
          <div
            onMouseUp={() => {
              const text = window.getSelection()?.toString().trim() ?? "";
              if (text && canComment) setQuote(text.slice(0, 1000));
            }}
          >
            <Markdown text={version.body} handlers={{ chip }} />
          </div>
          {!shown && (
            <SignaturePanel base={path} title={view.title ?? "Document"} version={version.version}
              signatures={view.signatures ?? []} canSign={canWrite} onSigned={load} />
          )}
          <CommentsPanel base={path} stepId={step.id} userId={userId} canResolveAny={canResolveAny}
            canComment={canComment} quote={quote} clearQuote={() => setQuote("")} events={events} />
        </>
      ) : (
        !drafting && <p className="muted">Nothing drafted yet for this step.</p>
      )}

      {zoom && (
        <Zoom path={`/v1/sessions/${sessionId}/observations/${zoom.id}/screenshot`} title={zoom.title} onClose={() => setZoom(null)} />
      )}
    </section>
  );
}
